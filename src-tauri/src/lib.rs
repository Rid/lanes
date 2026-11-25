//! Lanes — a tiny macOS menu-bar launcher.
//!
//! The whole app is a status-bar item that drops down a small panel. No Dock icon,
//! no main window in the usual sense, no network, no token handling. Clicking a lane
//! spawns the app's binary with that lane's isolated `--user-data-dir`.

mod apps;
mod launch;
mod profiles;

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    ActivationPolicy, LogicalPosition, LogicalSize, Manager, Runtime, WindowEvent,
};

/// Tracks when the panel was last auto-hidden (on blur), so a click on the tray
/// icon that *caused* that blur doesn't immediately re-open the panel.
#[derive(Default)]
struct PanelState {
    last_hidden: Mutex<Option<Instant>>,
}

const PANEL_WIDTH: f64 = 300.0;

// ---- commands (the only privileged surface exposed to the webview) ----

#[tauri::command]
fn known_apps() -> Vec<apps::AppInfo> {
    apps::list()
}

#[tauri::command]
fn list_profiles() -> Vec<profiles::Profile> {
    profiles::load().profiles
}

#[tauri::command]
fn add_profile(app: String, label: String, color: String) -> Result<profiles::Profile, String> {
    profiles::add(&app, &label, &color)
}

#[tauri::command]
fn rename_profile(id: String, label: String) -> Result<(), String> {
    profiles::rename(&id, &label)
}

#[tauri::command]
fn set_profile_color(id: String, color: String) -> Result<(), String> {
    profiles::set_color(&id, &color)
}

#[tauri::command]
fn remove_profile(id: String, delete_data: bool) -> Result<(), String> {
    profiles::remove(&id, delete_data)
}

/// The set of data dirs that currently have a live app instance, so the UI can
/// mark which lanes are open. Covers two cases:
///   1. Lanes-launched instances — matched by their exact `--user-data-dir=<dir>`.
///   2. A *default* session started normally (Dock/Finder) — a main app binary
///      running with NO `--user-data-dir` flag maps to the app's default data dir.
fn compute_running_dirs() -> Vec<String> {
    let lines = launch::command_lines();
    // Pad each line so an end-of-line match still has a trailing space to test
    // against, and so `claude-work` can't prefix-match `claude-work-2`.
    let padded: Vec<String> = lines.iter().map(|l| format!("{l} ")).collect();
    let mut running: Vec<String> = Vec::new();

    for p in profiles::load().profiles {
        let needle = format!("--user-data-dir={} ", p.data_dir);
        if padded.iter().any(|l| l.contains(&needle)) && !running.contains(&p.data_dir) {
            running.push(p.data_dir);
        }
    }

    for a in apps::KNOWN_APPS {
        let Some(bin) = a.binary_path() else { continue };
        let prefix = format!("{} ", bin.to_string_lossy());
        let default_running = padded
            .iter()
            .any(|l| l.starts_with(&prefix) && !l.contains("--user-data-dir="));
        if default_running {
            let dd = a.default_data_dir().to_string_lossy().to_string();
            if !running.contains(&dd) {
                running.push(dd);
            }
        }
    }
    running
}

#[tauri::command]
fn running_dirs() -> Vec<String> {
    compute_running_dirs()
}

/// An existing default session the user could adopt as a lane.
#[derive(serde::Serialize)]
struct Adoptable {
    key: String,
    name: String,
    monogram: String,
    default_color: String,
    default_dir: String,
    running: bool,
}

/// Installed apps whose default session exists (or is running) but hasn't been
/// added as a lane yet — i.e. "you're already signed into Claude, adopt it."
#[tauri::command]
fn adoptable_apps() -> Vec<Adoptable> {
    let profiles = profiles::load().profiles;
    let running = compute_running_dirs();
    apps::KNOWN_APPS
        .iter()
        .filter_map(|a| {
            a.binary_path()?;
            let dd = a.default_data_dir();
            let dd_str = dd.to_string_lossy().to_string();
            // Already added, or the user has never used this app → nothing to adopt.
            if profiles.iter().any(|p| p.data_dir == dd_str) {
                return None;
            }
            let is_running = running.contains(&dd_str);
            if !dd.exists() && !is_running {
                return None;
            }
            Some(Adoptable {
                key: a.key.into(),
                name: a.name.into(),
                monogram: a.monogram.into(),
                default_color: a.default_color.into(),
                default_dir: dd_str,
                running: is_running,
            })
        })
        .collect()
}

#[tauri::command]
fn adopt_profile(app: String, label: String) -> Result<profiles::Profile, String> {
    profiles::adopt(&app, &label)
}

#[tauri::command]
fn launch_profile<R: Runtime>(app: tauri::AppHandle<R>, id: String) -> Result<(), String> {
    let store = profiles::load();
    let profile = store
        .profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or("Profile not found.")?;
    let app_def = apps::find(&profile.app).ok_or("Unknown app for this profile.")?;
    let binary = app_def
        .binary_path()
        .ok_or_else(|| format!("{} isn't installed.", app_def.name))?;

    launch::launch(&binary, Path::new(&profile.data_dir)).map_err(|e| e.to_string())?;

    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    Ok(())
}

/// Resize the panel to fit its content (width stays fixed; the webview reports its
/// natural height after rendering).
#[tauri::command]
fn set_panel_height<R: Runtime>(app: tauri::AppHandle<R>, height: f64) {
    if let Some(win) = app.get_webview_window("main") {
        let h = height.clamp(120.0, 640.0);
        let _ = win.set_size(LogicalSize::new(PANEL_WIDTH, h));
    }
}

#[tauri::command]
fn hide_panel<R: Runtime>(app: tauri::AppHandle<R>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn quit<R: Runtime>(app: tauri::AppHandle<R>) {
    app.exit(0);
}

// ---- tray + panel positioning ----

/// The display holding `point` (physical pixels at that display's own scale, as
/// the tray rect is reported), as `(scale, left, right)` with edges in logical
/// points. macOS screen coordinates are global logical points, so a pixel value
/// only maps onto a display when read at that display's scale.
fn display_at<R: Runtime>(
    win: &tauri::WebviewWindow<R>,
    point: tauri::Position,
) -> Option<(f64, f64, f64)> {
    win.available_monitors().ok()?.iter().find_map(|m| {
        let scale = m.scale_factor();
        let p = point.to_logical::<f64>(scale);
        let origin = m.position().to_logical::<f64>(scale);
        let size = m.size().to_logical::<f64>(scale);
        let inside = (origin.x..origin.x + size.width).contains(&p.x)
            && (origin.y..origin.y + size.height).contains(&p.y);
        inside.then_some((scale, origin.x, origin.x + size.width))
    })
}

fn show_panel<R: Runtime>(app: &tauri::AppHandle<R>, rect: tauri::Rect) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    // Every display has its own menu bar, so the panel belongs on the display the
    // clicked icon is on. (Clamping to the primary display's width used to throw
    // it onto the primary display when the icon was clicked on another one.)
    let display = display_at(&win, rect.position);
    let scale = display.map_or_else(|| win.scale_factor().unwrap_or(1.0), |d| d.0);
    let icon_pos = rect.position.to_logical::<f64>(scale);
    let icon_size = rect.size.to_logical::<f64>(scale);

    // Center the panel under the tray icon, hanging just below the menu bar.
    let mut x = icon_pos.x + icon_size.width / 2.0 - PANEL_WIDTH / 2.0;
    let y = icon_pos.y + icon_size.height;

    // Keep it fully on that display.
    if let Some((_, left, right)) = display {
        x = x.min(right - PANEL_WIDTH - 8.0).max(left + 8.0);
    }

    let _ = win.set_position(LogicalPosition::new(x, y));
    let _ = win.show();
    let _ = win.set_focus();
}

fn on_tray_click<R: Runtime>(app: &tauri::AppHandle<R>, rect: tauri::Rect) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let visible = win.is_visible().unwrap_or(false);
    let recently_hidden = {
        let state = app.state::<PanelState>();
        let guard = state.last_hidden.lock().unwrap();
        guard
            .map(|t| t.elapsed() < Duration::from_millis(250))
            .unwrap_or(false)
    };

    if visible {
        let _ = win.hide();
    } else if !recently_hidden {
        show_panel(app, rect);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(PanelState::default())
        .invoke_handler(tauri::generate_handler![
            known_apps,
            list_profiles,
            add_profile,
            rename_profile,
            set_profile_color,
            remove_profile,
            running_dirs,
            adoptable_apps,
            adopt_profile,
            launch_profile,
            set_panel_height,
            hide_panel,
            quit,
        ])
        .setup(|app| {
            // Menu-bar only: no Dock icon, no app-switcher entry.
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(ActivationPolicy::Accessory);

            // Deliberately NO menu attached to the status item. With an NSMenu attached,
            // newer macOS builds (seen on macOS 27) show it on *every* click regardless of
            // show_menu_on_left_click(false) — the click never reaches our handler and the
            // panel never opens (the user just sees a menu with "Quit"). Any click now
            // toggles the panel; Quit lives in the panel footer.
            //
            // Monochrome template icon → renders in the menu-bar colour and adapts
            // to light/dark automatically.
            let tray_icon = tauri::include_image!("icons/tray.png");

            TrayIconBuilder::with_id("lanes-tray")
                .icon(tray_icon)
                .icon_as_template(true)
                .tooltip("Lanes")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        on_tray_click(tray.app_handle(), rect);
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            // Dismiss the panel when it loses focus (click outside / second tray click).
            if let WindowEvent::Focused(false) = event {
                if window.label() == "main" {
                    let app = window.app_handle();
                    if let Some(state) = app.try_state::<PanelState>() {
                        *state.last_hidden.lock().unwrap() = Some(Instant::now());
                    }
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Lanes")
        .run(|_app, _event| {});
}
