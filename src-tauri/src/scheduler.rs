//! Background refresh. Runs in Rust rather than on a page timer because
//! WebView2 throttles timers in windows that sit behind others, which is
//! exactly where a desktop widget lives.
//!
//! Each provider has its own due time (see `usage_core::schedule`), results
//! are cached on disk so restarts don't refetch fresh data, and a sleeping
//! machine triggers a refresh when it wakes up.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use usage_core::{schedule, ErrorCode, ProviderReport};

use crate::config::{Config, ProviderConfig};
use crate::{providers, AppState};

pub const USAGE_UPDATED: &str = "usage-updated";
const TICK: Duration = Duration::from_secs(20);
/// A gap this long between ticks means the machine was asleep.
const RESUME_GAP: i64 = 120;

#[derive(Default, Serialize, Deserialize)]
struct Entry {
    report: Option<ProviderReport>,
    /// Unix seconds.
    next_due: i64,
    failures: u32,
    /// Login file and its modification time when the login last failed.
    #[serde(skip)]
    watch: Option<(PathBuf, Option<SystemTime>)>,
}

impl Entry {
    fn last_error(&self) -> Option<ErrorCode> {
        self.report.as_ref()?.error.as_ref().map(|e| e.code)
    }
}

pub struct Scheduler {
    path: PathBuf,
    entries: Mutex<HashMap<String, Entry>>,
    wake: Notify,
    /// Serialises fetch rounds between the loop and manual refreshes.
    round: AsyncMutex<()>,
}

impl Scheduler {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("usage-cache.json");
        let mut entries: HashMap<String, Entry> =
            fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        // File watches don't survive a restart, so re-check failed logins right away.
        for entry in entries.values_mut() {
            if entry.last_error().is_some() {
                entry.next_due = 0;
            }
        }
        Self { path, entries: Mutex::new(entries), wake: Notify::new(), round: AsyncMutex::new(()) }
    }

    /// Reports for the enabled providers, in Settings order.
    pub fn snapshot(&self, config: &Config) -> Vec<ProviderReport> {
        let entries = self.entries.lock().unwrap();
        config
            .providers
            .iter()
            .filter(|p| p.enabled)
            .filter_map(|p| {
                let mut report = entries.get(&p.id)?.report.clone()?;
                report.label = p.label.clone();
                Some(report)
            })
            .collect()
    }

    /// Makes providers due now: all of them, or one by id. Providers that are
    /// being rate limited keep their back-off unless named explicitly.
    pub fn expire(&self, id: Option<&str>) {
        let mut entries = self.entries.lock().unwrap();
        for (key, entry) in entries.iter_mut() {
            let named = id == Some(key.as_str());
            if named || (id.is_none() && entry.last_error() != Some(ErrorCode::RateLimited)) {
                entry.next_due = 0;
            }
        }
        drop(entries);
        self.wake.notify_one();
    }

    fn due(&self, config: &Config, now: i64) -> Vec<ProviderConfig> {
        let entries = self.entries.lock().unwrap();
        config
            .providers
            .iter()
            .filter(|p| p.enabled)
            .filter(|p| match entries.get(&p.id) {
                None => true,
                Some(e) => now >= e.next_due || e.watch.as_ref().is_some_and(|(path, seen)| modified(path) != *seen),
            })
            .cloned()
            .collect()
    }

    fn record(&self, config: &Config, reports: Vec<ProviderReport>, now: i64) {
        let mut entries = self.entries.lock().unwrap();
        for fresh in reports {
            let Some(provider) = config.providers.iter().find(|p| p.id == fresh.id) else { continue };
            let entry = entries.entry(fresh.id.clone()).or_default();
            let code = fresh.error.as_ref().map(|e| e.code);
            entry.failures = if code.is_some() { entry.failures.saturating_add(1) } else { 0 };

            let next = schedule::next(provider.kind, config.refresh_minutes, code, entry.failures);
            entry.next_due = now + next.delay;
            entry.watch = if next.watch_credentials {
                providers::credentials_path(provider).map(|path| {
                    let seen = modified(&path);
                    (path, seen)
                })
            } else {
                None
            };
            entry.report = Some(schedule::merge(entry.report.as_ref(), fresh));
        }
        entries.retain(|id, _| config.providers.iter().any(|p| &p.id == id));
    }

    fn persist(&self) {
        let text = match serde_json::to_string(&*self.entries.lock().unwrap()) {
            Ok(text) => text,
            Err(_) => return,
        };
        let tmp = self.path.with_extension("json.tmp");
        if fs::write(&tmp, text).and_then(|_| fs::rename(&tmp, &self.path)).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }

    /// Fetches whatever is due and pushes the result to every window.
    pub async fn run_round(&self, app: &AppHandle) {
        let _round = self.round.lock().await;
        let state = app.state::<AppState>();
        let config = state.config.get();
        let due = self.due(&config, now());
        if due.is_empty() {
            return;
        }
        let reports = providers::fetch_all(&state.client, due).await;
        self.record(&config, reports, now());
        self.persist();
        self.publish(app, &config);
    }

    pub fn publish(&self, app: &AppHandle, config: &Config) {
        let _ = app.emit(USAGE_UPDATED, self.snapshot(config));
    }
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_tick = now();
        loop {
            let state = app.state::<AppState>();
            let tick = now();
            if tick - last_tick > RESUME_GAP {
                state.scheduler.expire(None);
            }
            last_tick = tick;

            state.scheduler.run_round(&app).await;

            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                _ = state.scheduler.wake.notified() => {}
            }
        }
    });
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
