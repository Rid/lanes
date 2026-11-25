//! Lanes — a tiny macOS menu-bar launcher.
//!
//! The menu-bar item opens a native menu of lanes (see `menu.rs`); choosing one
//! spawns the app's binary with that lane's isolated `--user-data-dir`. Lanes are
//! added, renamed and removed in a small manager window. No Dock icon, no network,
//! no token handling.

mod apps;
mod copy;
mod launch;
#[cfg(target_os = "macos")]
mod menu;
mod profiles;

use std::path::Path;

use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, RunEvent, Runtime, WebviewUrl,
    WebviewWindowBuilder,
};

/// Label of the manager window (the only window), as used in capabilities.
const MANAGER: &str = "manage";
const MANAGER_WIDTH: f64 = 360.0;

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

/// The set of data dirs that currently have a live app instance, so the menu can
/// mark which lanes are open. Covers two cases:
///   1. Lanes-launched instances — matched by their exact `--user-data-dir=<dir>`.
///   2. A *default* session started normally (Dock/Finder) — a main app binary
///      running with NO `--user-data-dir` flag maps to the app's default data dir.
pub(crate) fn compute_running_dirs() -> Vec<String> {
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

/// An existing default session the user could adopt as a lane.
#[derive(serde::Serialize)]
pub(crate) struct Adoptable {
    pub key: String,
    pub name: String,
    monogram: String,
    default_color: String,
    default_dir: String,
    running: bool,
}

/// Installed apps whose default session exists (or is running) but hasn't been
/// added as a lane yet — i.e. "you're already signed into Claude, adopt it."
pub(crate) fn adoptable(profiles: &[profiles::Profile], running: &[String]) -> Vec<Adoptable> {
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
fn adoptable_apps() -> Vec<Adoptable> {
    adoptable(&profiles::load().profiles, &compute_running_dirs())
}

#[tauri::command]
fn adopt_profile(app: String, label: String) -> Result<profiles::Profile, String> {
    profiles::adopt(&app, &label)
}

/// Copy one Claude lane's setup (settings, MCP servers, plugins, Code and Cowork
/// sessions) into another. Never its sign-in: see `copy.rs`. Async so the copy
/// runs off the main thread and the menu stays responsive.
#[tauri::command]
async fn copy_lane(from: String, to: String) -> Result<copy::Summary, String> {
    let store = profiles::load();
    let find = |id: &str| store.profiles.iter().find(|p| p.id == id).ok_or("Lane not found.");
    let (src, dst) = (find(&from)?, find(&to)?);
    if src.id == dst.id {
        return Err("Choose two different lanes.".into());
    }
    if src.app != "claude" || dst.app != "claude" {
        return Err("Copying works between Claude lanes only.".into());
    }
    if compute_running_dirs().contains(&dst.data_dir) {
        return Err(format!("Quit Claude in “{}” first, then copy again.", dst.label));
    }
    let not_signed_in = |label: &str| {
        format!("Lanes can't find the account “{label}” is signed in to. Open it from the menu, sign in, quit it, then copy again.")
    };
    let src_id = copy::identity(Path::new(&src.data_dir)).ok_or_else(|| not_signed_in(&src.label))?;
    let dst_id = copy::identity(Path::new(&dst.data_dir)).ok_or_else(|| not_signed_in(&dst.label))?;

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let backup = profiles::lanes_root()
        .join("backups")
        .join(format!("{}-{stamp}", dst.id));
    copy::copy_setup(
        Path::new(&src.data_dir),
        &src_id,
        Path::new(&dst.data_dir),
        &dst_id,
        &backup,
    )
    .map_err(|e| format!("The copy stopped partway: {e}"))
}

/// Resize the manager window to fit its content (width stays fixed; the webview
/// reports its natural height after rendering).
#[tauri::command]
fn fit_manager<R: Runtime>(app: AppHandle<R>, height: f64) {
    if let Some(win) = app.get_webview_window(MANAGER) {
        let _ = win.set_size(LogicalSize::new(MANAGER_WIDTH, height.clamp(160.0, 640.0)));
    }
}

#[tauri::command]
fn close_manager<R: Runtime>(app: AppHandle<R>) {
    if let Some(win) = app.get_webview_window(MANAGER) {
        let _ = win.close();
    }
}

// ---- actions the menu takes ----

/// Launch a lane's app pointed at its data dir.
pub(crate) fn launch_lane(id: &str) -> Result<(), String> {
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
    Ok(())
}

/// Show the manager window on `view` ("manage", "add", "copy" or "adopt:<app>"),
/// creating it if needed and bringing it to the front.
pub(crate) fn open_manager<R: Runtime>(app: &AppHandle<R>, view: &str) {
    if let Some(win) = app.get_webview_window(MANAGER) {
        let _ = app.emit_to(MANAGER, "lanes://view", view);
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    // A new window reads its first view from here, before panel.js runs.
    let init = format!("window.__LANES_VIEW = {};", serde_json::json!(view));
    let builder = WebviewWindowBuilder::new(app, MANAGER, WebviewUrl::App("index.html".into()))
        .title("Lanes")
        .inner_size(MANAGER_WIDTH, 420.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .initialization_script(init);
    // The header sits in the title bar, with the traffic lights centred in it.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(18.0, 20.0));
    if let Ok(win) = builder.build() {
        let _ = win.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            known_apps,
            list_profiles,
            add_profile,
            rename_profile,
            set_profile_color,
            remove_profile,
            adoptable_apps,
            adopt_profile,
            copy_lane,
            fit_manager,
            close_manager,
        ])
        .setup(|app| {
            // Menu-bar only: no Dock icon, no app-switcher entry.
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                menu::install(app.handle().clone());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Lanes")
        .run(|_app, event| {
            // The manager is the only window, and closing it must not quit a
            // menu-bar app. An explicit quit (the menu's Quit) carries an exit code.
            if let RunEvent::ExitRequested { api, code: None, .. } = event {
                api.prevent_exit();
            }
        });
}
