mod config;
mod layer;
mod providers;
mod scheduler;
mod secrets;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent, Wry,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use usage_core::{ProviderKind, ProviderReport};

use config::{Config, ConfigStore, WindowState};
use scheduler::Scheduler;

/// `edge`: a drop hanging from the top of the screen. `desktop`: a note anywhere on it.
/// Which layer it lives on is a separate setting (`above_windows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    Edge,
    Desktop,
}

impl Placement {
    fn from_config(value: &str) -> Self {
        if value == "desktop" {
            Self::Desktop
        } else {
            Self::Edge
        }
    }
}

const WIDGET: &str = "widget";
const SETTINGS: &str = "settings";
/// Desktop mode: distance from the screen edge on first launch, in logical pixels.
const EDGE_MARGIN: f64 = 16.0;
/// Edge mode: collapsed drop size, in logical pixels. Mirrors `DROP` in src/widget.ts.
const DROP_SIZE: (f64, f64) = (64.0, 26.0);
/// Edge mode: default distance from the right edge of the screen to the drop's centre.
const DEFAULT_EDGE_OFFSET: f64 = 240.0;

struct AppState {
    dir: PathBuf,
    config: ConfigStore,
    client: reqwest::Client,
    scheduler: Scheduler,
    tray: TrayItems,
    window: Mutex<WindowState>,
    /// Edge offset when a sideways drag started.
    drag_origin: Mutex<Option<f64>>,
    /// Set while a debounced save of `window` is pending.
    save_pending: Mutex<bool>,
}

struct TrayItems {
    toggle: MenuItem<Wry>,
    refresh: MenuItem<Wry>,
    settings: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Config {
    state.config.get()
}

#[tauri::command]
fn save_config(app: AppHandle, state: State<AppState>, config: Config) -> Result<Config, String> {
    let before = state.config.get();
    let config = state.config.save(config)?;
    apply_config(&app, &config);
    if before.placement != config.placement {
        apply_placement(&app, &config);
    } else if before.above_windows != config.above_windows {
        if let Some(widget) = app.get_webview_window(WIDGET) {
            layer::apply(&widget, config.above_windows);
        }
    }

    // Only refetch what the change can affect; a theme switch costs no requests.
    if before.refresh_minutes != config.refresh_minutes {
        state.scheduler.expire(None);
    } else {
        for p in &config.providers {
            if before.providers.iter().find(|b| b.id == p.id) != Some(p) {
                state.scheduler.expire(Some(&p.id));
            }
        }
    }
    app.emit("config-changed", &config).map_err(|e| e.to_string())?;
    state.scheduler.publish(&app, &config);
    Ok(config)
}

/// Last known reports; fresh ones arrive through the `usage-updated` event.
#[tauri::command]
fn get_usage(state: State<AppState>) -> Vec<ProviderReport> {
    state.scheduler.snapshot(&state.config.get())
}

#[tauri::command]
async fn refresh_now(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<ProviderReport>, String> {
    state.scheduler.expire(None);
    state.scheduler.run_round(&app).await;
    Ok(state.scheduler.snapshot(&state.config.get()))
}

#[tauri::command]
fn has_secret(provider_id: String) -> Result<bool, String> {
    Ok(secrets::get(&provider_id)?.is_some())
}

#[tauri::command]
fn set_secret(state: State<AppState>, provider_id: String, secret: String) -> Result<(), String> {
    secrets::set(&provider_id, secret.trim())?;
    state.scheduler.expire(Some(&provider_id));
    Ok(())
}

#[tauri::command]
fn delete_secret(state: State<AppState>, provider_id: String) -> Result<(), String> {
    secrets::delete(&provider_id)?;
    state.scheduler.expire(Some(&provider_id));
    Ok(())
}

#[tauri::command]
fn default_credentials_path(kind: ProviderKind) -> Option<String> {
    providers::default_credentials_path(kind).map(|p| p.display().to_string())
}

/// Must be async: building a webview from a sync command deadlocks on Windows
/// (the window appears but stays blank).
#[tauri::command]
async fn open_settings(app: AppHandle) {
    open_settings_window(&app);
}

#[derive(Deserialize)]
struct TrayLabels {
    toggle: String,
    refresh: String,
    settings: String,
    quit: String,
}

/// The tray menu is native, so the UI hands over its labels.
#[tauri::command]
fn set_tray_labels(state: State<AppState>, labels: TrayLabels) -> Result<(), String> {
    let t = &state.tray;
    for (item, text) in [
        (&t.toggle, labels.toggle),
        (&t.refresh, labels.refresh),
        (&t.settings, labels.settings),
        (&t.quit, labels.quit),
    ] {
        item.set_text(text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Edge mode: resizes the window (drop or open panel) and hangs it from the top of the screen.
#[tauri::command]
fn edge_layout(window: WebviewWindow, state: State<AppState>, width: f64, height: f64) {
    let offset = state.window.lock().unwrap().edge_offset.unwrap_or(DEFAULT_EDGE_OFFSET);
    place_on_edge(&window, width, height, offset);
}

#[tauri::command]
fn edge_drag_start(state: State<AppState>) {
    let offset = state.window.lock().unwrap().edge_offset.unwrap_or(DEFAULT_EDGE_OFFSET);
    *state.drag_origin.lock().unwrap() = Some(offset);
}

/// Edge mode: slides the drop along the top edge. `dx` is the pointer's travel in logical pixels.
#[tauri::command]
fn edge_drag(app: AppHandle, window: WebviewWindow, state: State<AppState>, dx: f64) {
    let Some(origin) = *state.drag_origin.lock().unwrap() else { return };
    let Ok(Some(monitor)) = window.primary_monitor() else { return };
    let scale = monitor.scale_factor();
    let screen_width = monitor.work_area().size.width as f64 / scale;
    let half = window.outer_size().map(|s| s.width as f64 / scale / 2.0).unwrap_or(DROP_SIZE.0 / 2.0);
    let offset = (origin - dx).clamp(half + 4.0, (screen_width - half - 4.0).max(half + 4.0));

    state.window.lock().unwrap().edge_offset = Some(offset);
    if let Ok(size) = window.outer_size() {
        place_on_edge(&window, size.width as f64 / scale, size.height as f64 / scale, offset);
    }
    save_window_state_soon(&app);
}

#[tauri::command]
fn edge_drag_end(state: State<AppState>) {
    *state.drag_origin.lock().unwrap() = None;
}

pub fn run() {
    tauri::Builder::default()
        // Must be registered first: a second launch just surfaces the running widget.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_widget(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            let config = ConfigStore::load(&dir);
            let tray = build_tray(app.handle())?;
            let initial = config.get();
            app.manage(AppState {
                dir: dir.clone(),
                config,
                client: providers::http_client(),
                scheduler: Scheduler::load(&dir),
                tray,
                window: Mutex::new(config::load_window_state(&dir)),
                drag_origin: Mutex::new(None),
                save_pending: Mutex::new(false),
            });

            apply_config(app.handle(), &initial);
            apply_placement(app.handle(), &initial);
            let widget = app.get_webview_window(WIDGET).expect("widget window is declared in tauri.conf.json");
            layer::show(&widget, initial.above_windows);
            scheduler::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != WIDGET {
                return;
            }
            match event {
                WindowEvent::Moved(pos) => remember_desktop_position(window.app_handle(), *pos),
                // Alt+F4 hides the widget instead of quitting; quitting lives in the tray menu.
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_usage,
            refresh_now,
            has_secret,
            set_secret,
            delete_secret,
            default_credentials_path,
            open_settings,
            set_tray_labels,
            edge_layout,
            edge_drag_start,
            edge_drag,
            edge_drag_end,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the application");
}

fn build_tray(app: &AppHandle) -> tauri::Result<TrayItems> {
    let items = TrayItems {
        toggle: MenuItem::with_id(app, "toggle", "Show / hide", true, None::<&str>)?,
        refresh: MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?,
        settings: MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?,
        quit: MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
    };
    let menu = Menu::with_items(
        app,
        &[&items.toggle, &items.refresh, &items.settings, &PredefinedMenuItem::separator(app)?, &items.quit],
    )?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("AI Usage Widget")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => toggle_widget(app),
            "refresh" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    state.scheduler.expire(None);
                    state.scheduler.run_round(&app).await;
                });
            }
            "settings" => open_settings_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_widget(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(items)
}

fn apply_config(app: &AppHandle, config: &Config) {
    let autolaunch = app.autolaunch();
    if autolaunch.is_enabled().unwrap_or(false) != config.autostart {
        let result = if config.autostart { autolaunch.enable() } else { autolaunch.disable() };
        if let Err(e) = result {
            eprintln!("could not update launch-at-startup: {e}");
        }
    }
}

/// Moves the widget into its layer and spot for the chosen placement. The UI
/// then sizes it (`edge_layout` or a plain resize) once it has rendered.
fn apply_placement(app: &AppHandle, config: &Config) {
    let Some(widget) = app.get_webview_window(WIDGET) else { return };
    let placement = Placement::from_config(&config.placement);
    layer::apply(&widget, config.above_windows);
    let state = app.state::<AppState>();
    let saved = *state.window.lock().unwrap();
    match placement {
        Placement::Edge => {
            place_on_edge(&widget, DROP_SIZE.0, DROP_SIZE.1, saved.edge_offset.unwrap_or(DEFAULT_EDGE_OFFSET));
        }
        Placement::Desktop => place_on_desktop(&widget, saved),
    }
}

fn place_on_edge(window: &WebviewWindow, width: f64, height: f64, offset: f64) {
    let Ok(Some(monitor)) = window.primary_monitor() else { return };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let (w, h) = ((width * scale).round() as i32, (height * scale).round() as i32);
    let left = area.position.x;
    let right = left + area.size.width as i32;
    let center = right - (offset * scale).round() as i32;
    let x = (center - w / 2).clamp(left, (right - w).max(left));
    let _ = window.set_size(PhysicalSize::new(w as u32, h as u32));
    let _ = window.set_position(PhysicalPosition::new(x, area.position.y));
}

/// Restores the last dragged position if it's still on a connected screen,
/// otherwise puts the note near the top-right corner of the primary screen.
fn place_on_desktop(widget: &WebviewWindow, saved: WindowState) {
    if let (Some(x), Some(y)) = (saved.x, saved.y) {
        let visible = widget.available_monitors().unwrap_or_default().iter().any(|m| {
            let (origin, size) = (m.position(), m.size());
            x >= origin.x - 40
                && y >= origin.y - 10
                && x < origin.x + size.width as i32 - 40
                && y < origin.y + size.height as i32 - 40
        });
        if visible {
            let _ = widget.set_position(PhysicalPosition::new(x, y));
            return;
        }
    }

    let Ok(Some(monitor)) = widget.primary_monitor() else { return };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let width = widget.outer_size().map(|s| s.width as i32).unwrap_or((272.0 * scale) as i32);
    let margin = (EDGE_MARGIN * scale).round() as i32;
    let x = area.position.x + area.size.width as i32 - width - margin;
    let _ = widget.set_position(PhysicalPosition::new(x, area.position.y + margin));
}

/// Desktop mode only: edge mode positions the window itself.
fn remember_desktop_position(app: &AppHandle, pos: PhysicalPosition<i32>) {
    // Windows parks minimized windows at (-32000, -32000).
    if pos.x <= -30_000 || pos.y <= -30_000 {
        return;
    }
    let state = app.state::<AppState>();
    if Placement::from_config(&state.config.get().placement) != Placement::Desktop {
        return;
    }
    {
        let mut window = state.window.lock().unwrap();
        window.x = Some(pos.x);
        window.y = Some(pos.y);
    }
    save_window_state_soon(app);
}

/// Dragging fires many events; write the window state once they settle.
fn save_window_state_soon(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut pending = state.save_pending.lock().unwrap();
        if *pending {
            return;
        }
        *pending = true;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        let state = app.state::<AppState>();
        *state.save_pending.lock().unwrap() = false;
        let snapshot = *state.window.lock().unwrap();
        if let Err(e) = config::save_window_state(&state.dir, &snapshot) {
            eprintln!("could not save widget position: {e}");
        }
    });
}

fn show_widget(app: &AppHandle) {
    if let Some(widget) = app.get_webview_window(WIDGET) {
        layer::show(&widget, app.state::<AppState>().config.get().above_windows);
    }
}

fn toggle_widget(app: &AppHandle) {
    let Some(widget) = app.get_webview_window(WIDGET) else { return };
    if widget.is_visible().unwrap_or(false) {
        let _ = widget.hide();
    } else {
        show_widget(app);
    }
}

fn open_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("settings.html".into()))
        .title("AI Usage Widget · Settings")
        .inner_size(520.0, 700.0)
        .min_inner_size(440.0, 480.0)
        .center()
        .build();
    if let Err(e) = built {
        eprintln!("could not open settings: {e}");
    }
}
