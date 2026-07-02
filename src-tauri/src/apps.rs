//! Known-apps table.
//!
//! This is just *data*. Each entry maps an app key to its display name and the
//! executable inside its `.app` bundle. To support a new app once a working
//! isolation path is confirmed, add one row here — nothing else changes.
//!
//! v0 ships **Claude only**. The live spike proved Codex ignores `--user-data-dir`
//! (it writes to a fixed location, so two instances share one session) and ChatGPT
//! is a native, non-Electron app. Neither isolates, so neither is listed.

use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct KnownApp {
    pub key: &'static str,
    pub name: &'static str,
    pub monogram: &'static str,
    pub default_color: &'static str,
    /// Bundle directory name under an Applications folder, e.g. "Claude.app".
    pub bundle: &'static str,
    /// Executable name inside `Contents/MacOS`.
    pub binary: &'static str,
    /// The app's *default* Electron user-data folder name under
    /// `~/Library/Application Support`. This is where a normal launch (Dock/Finder)
    /// keeps its session — the "account you're already signed into."
    pub default_dir_name: &'static str,
}

pub const KNOWN_APPS: &[KnownApp] = &[KnownApp {
    key: "claude",
    name: "Claude",
    monogram: "C",
    default_color: "#008080",
    bundle: "Claude.app",
    binary: "Claude",
    default_dir_name: "Claude",
}];

pub fn find(key: &str) -> Option<&'static KnownApp> {
    KNOWN_APPS.iter().find(|a| a.key == key)
}

/// Where apps may be installed, in priority order.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join("Applications"));
    }
    dirs
}

impl KnownApp {
    /// Resolve the executable inside the `.app` bundle, if the app is installed.
    pub fn binary_path(&self) -> Option<PathBuf> {
        for dir in search_dirs() {
            let p = dir
                .join(self.bundle)
                .join("Contents/MacOS")
                .join(self.binary);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }

    pub fn installed(&self) -> bool {
        self.binary_path().is_some()
    }

    /// `~/Library/Application Support/<default_dir_name>` — the session a normal
    /// launch uses. Lanes can *point at* this dir to adopt the existing account;
    /// it never reads what's inside.
    pub fn default_data_dir(&self) -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        home.join("Library/Application Support")
            .join(self.default_dir_name)
    }
}

/// Shape sent to the UI for the "add profile" form.
#[derive(Serialize)]
pub struct AppInfo {
    pub key: String,
    pub name: String,
    pub monogram: String,
    pub default_color: String,
    pub installed: bool,
}

pub fn list() -> Vec<AppInfo> {
    KNOWN_APPS
        .iter()
        .map(|a| AppInfo {
            key: a.key.into(),
            name: a.name.into(),
            monogram: a.monogram.into(),
            default_color: a.default_color.into(),
            installed: a.installed(),
        })
        .collect()
}
