//! User settings, persisted as JSON in the platform config dir
//! (`%APPDATA%\io.github.aiusagewidget\config.json` on Windows).
//! Secrets never live here; see `secrets.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use usage_core::ProviderKind;

use crate::providers;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub refresh_minutes: u32,
    pub autostart: bool,
    /// `edge` (a drop hanging from the top of the screen) or `desktop` (behind all windows).
    pub placement: String,
    /// Keep the widget above other windows instead of on the desktop layer.
    pub above_windows: bool,
    /// `auto`, `dark` or `light`.
    pub theme: String,
    /// Background opacity of the widget, 0.4–1.0.
    pub opacity: f64,
    pub providers: Vec<ProviderConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_minutes: 5,
            autostart: false,
            placement: "edge".into(),
            above_windows: false,
            theme: "auto".into(),
            opacity: 0.92,
            providers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub label: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Overrides the default CLI credentials location (subscription providers).
    #[serde(default)]
    pub credentials_path: Option<String>,
    /// Monthly budget in USD the spend bar fills towards (API providers).
    #[serde(default)]
    pub monthly_budget: Option<f64>,
}

fn yes() -> bool {
    true
}

impl Config {
    /// First-run config: add a subscription provider for every CLI that's already signed in.
    fn detected() -> Self {
        let mut config = Self::default();
        for (kind, label) in
            [(ProviderKind::ClaudeSubscription, "Claude"), (ProviderKind::CodexSubscription, "ChatGPT · Codex")]
        {
            if providers::default_credentials_path(kind).is_some_and(|p| p.exists()) {
                config.providers.push(ProviderConfig {
                    id: format!("{}-default", serde_json::to_value(kind).unwrap().as_str().unwrap()),
                    kind,
                    label: label.into(),
                    enabled: true,
                    credentials_path: None,
                    monthly_budget: None,
                });
            }
        }
        config
    }

    fn sanitized(mut self) -> Self {
        self.refresh_minutes = self.refresh_minutes.clamp(2, 24 * 60);
        self.opacity = self.opacity.clamp(0.4, 1.0);
        if self.placement != "desktop" {
            self.placement = "edge".into();
        }
        for p in &mut self.providers {
            p.label = p.label.trim().to_string();
            p.credentials_path = p.credentials_path.take().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            p.monthly_budget = p.monthly_budget.filter(|b| b.is_finite() && *b > 0.0);
        }
        self
    }
}

pub struct ConfigStore {
    path: PathBuf,
    current: Mutex<Config>,
}

impl ConfigStore {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("config.json");
        let config = match fs::read_to_string(&path) {
            // Notepad and PowerShell 5 write a UTF-8 BOM, which serde_json rejects.
            Ok(text) => serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')).unwrap_or_else(|e| {
                eprintln!("config.json is invalid ({e}); starting from defaults");
                Config::default()
            }),
            Err(_) => Config::detected(),
        };
        let store = Self { path, current: Mutex::new(config.sanitized()) };
        if !store.path.exists() {
            let _ = store.write(&store.get());
        }
        store
    }

    pub fn get(&self) -> Config {
        self.current.lock().unwrap().clone()
    }

    pub fn save(&self, config: Config) -> Result<Config, String> {
        let config = config.sanitized();
        self.write(&config)?;
        *self.current.lock().unwrap() = config.clone();
        Ok(config)
    }

    fn write(&self, config: &Config) -> Result<(), String> {
        write_json_atomic(&self.path, config)
    }
}

/// Where the widget sits, kept apart from `Config` so the settings window
/// never overwrites a position the user dragged to in the meantime.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowState {
    /// Desktop mode: top-left corner in physical pixels.
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// Edge mode: distance from the right edge of the screen to the drop's centre, in logical pixels.
    pub edge_offset: Option<f64>,
}

pub fn load_window_state(dir: &Path) -> WindowState {
    fs::read_to_string(dir.join("window.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_window_state(dir: &Path, state: &WindowState) -> Result<(), String> {
    write_json_atomic(&dir.join("window.json"), state)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}
