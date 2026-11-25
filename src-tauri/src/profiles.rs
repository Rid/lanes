//! Profile model + persistence.
//!
//! Profiles live in a small JSON file under the app's own config dir. Each profile
//! owns a directory that is handed to the launched app as its `--user-data-dir`.
//! Lanes only ever *chooses* that directory — it never reads, copies, or writes the
//! session/credential material the app stores inside it.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub id: String,
    pub app: String,
    pub label: String,
    pub color: String,
    #[serde(rename = "dataDir")]
    pub data_dir: String,
    /// True when this lane points at the app's *default* data dir (an adopted
    /// existing session). Its data folder is never offered for deletion.
    #[serde(rename = "isDefault", default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Store {
    pub profiles: Vec<Profile>,
}

/// `~/Library/Application Support/Lanes` — brand-named, not the bundle id, to match
/// the install story shown to users.
pub fn lanes_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library/Application Support/Lanes")
}

pub fn profiles_dir() -> PathBuf {
    lanes_root().join("profiles")
}

fn config_path() -> PathBuf {
    lanes_root().join("lanes.json")
}

/// Read the store. A missing file is an empty store, but a file that exists and
/// can't be read or parsed is an error: every change goes load → modify → save, so
/// treating it as empty would overwrite (and lose) all existing profiles.
fn read_store() -> Result<Store, String> {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s)
            .map_err(|e| format!("Couldn't parse {}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(format!("Couldn't read {}: {e}", path.display())),
    }
}

/// Best-effort read for display only. Never save what this returns.
pub fn load() -> Store {
    read_store().unwrap_or_default()
}

/// Write the store atomically (temp file + rename) so a crash mid-write can't
/// corrupt the profile list.
pub fn save(store: &Store) -> io::Result<()> {
    fs::create_dir_all(lanes_root())?;
    let json = serde_json::to_string_pretty(store).map_err(io::Error::other)?;
    let tmp = config_path().with_extension("json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, config_path())?;
    Ok(())
}

/// Turn a label like "Work Account" into a slug like "work-account".
fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("profile");
    }
    out
}

fn unique_id(store: &Store, base: &str) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while store.profiles.iter().any(|p| p.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// Create a profile, its data directory, and persist. Returns the new profile.
pub fn add(app_key: &str, label: &str, color: &str) -> Result<Profile, String> {
    if crate::apps::find(app_key).is_none() {
        return Err(format!("Unknown app: {app_key}"));
    }
    let label = label.trim();
    if label.is_empty() {
        return Err("Profile name can't be empty.".into());
    }
    let mut store = read_store()?;
    let base = format!("{}-{}", app_key, slugify(label));
    let id = unique_id(&store, &base);
    let dir = profiles_dir().join(&id);
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create data dir: {e}"))?;

    let profile = Profile {
        id,
        app: app_key.to_string(),
        label: label.to_string(),
        color: color.to_string(),
        data_dir: dir.to_string_lossy().to_string(),
        is_default: false,
    };
    store.profiles.push(profile.clone());
    save(&store).map_err(|e| format!("Couldn't save profiles: {e}"))?;
    Ok(profile)
}

/// Adopt the app's existing default session as a lane. Points at the app's
/// default data dir (which already exists) — no new directory is created, and the
/// dir is never a deletion target.
pub fn adopt(app_key: &str, label: &str) -> Result<Profile, String> {
    let app = crate::apps::find(app_key).ok_or_else(|| format!("Unknown app: {app_key}"))?;
    let label = label.trim();
    if label.is_empty() {
        return Err("Profile name can't be empty.".into());
    }
    let default_dir = app.default_data_dir().to_string_lossy().to_string();

    let mut store = read_store()?;
    if store.profiles.iter().any(|p| p.data_dir == default_dir) {
        return Err("This session is already added as a lane.".into());
    }
    let base = format!("{}-{}", app_key, slugify(label));
    let id = unique_id(&store, &base);

    let profile = Profile {
        id,
        app: app_key.to_string(),
        label: label.to_string(),
        color: app.default_color.to_string(),
        data_dir: default_dir,
        is_default: true,
    };
    store.profiles.push(profile.clone());
    save(&store).map_err(|e| format!("Couldn't save profiles: {e}"))?;
    Ok(profile)
}

pub fn rename(id: &str, label: &str) -> Result<(), String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("Profile name can't be empty.".into());
    }
    let mut store = read_store()?;
    let p = store
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("Profile not found.")?;
    p.label = label.to_string();
    save(&store).map_err(|e| format!("Couldn't save profiles: {e}"))
}

pub fn set_color(id: &str, color: &str) -> Result<(), String> {
    let mut store = read_store()?;
    let p = store
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("Profile not found.")?;
    p.color = color.to_string();
    save(&store).map_err(|e| format!("Couldn't save profiles: {e}"))
}

/// Only a folder directly inside our profiles dir may be deleted. `starts_with`
/// alone also accepted the profiles dir itself (every lane's data) and `..`
/// escapes such as `profiles/../..`, either of which a damaged or hand-edited
/// lanes.json could contain.
fn is_deletable(dir: &Path) -> bool {
    dir.parent() == Some(profiles_dir().as_path())
        && matches!(dir.components().next_back(), Some(Component::Normal(_)))
}

/// Remove a profile. If `delete_data`, also delete its data directory — but only
/// if it's safely inside our profiles dir (never an arbitrary path).
pub fn remove(id: &str, delete_data: bool) -> Result<(), String> {
    let mut store = read_store()?;
    let idx = store
        .profiles
        .iter()
        .position(|p| p.id == id)
        .ok_or("Profile not found.")?;
    let profile = store.profiles.remove(idx);
    save(&store).map_err(|e| format!("Couldn't save profiles: {e}"))?;

    if delete_data {
        let dir = PathBuf::from(&profile.data_dir);
        if is_deletable(&dir) && dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("Profile removed, but couldn't delete data dir: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Work"), "work");
        assert_eq!(slugify("Work Account!"), "work-account");
        assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
        assert_eq!(slugify("@@@"), "profile");
    }

    #[test]
    fn unique_id_disambiguates() {
        let store = Store {
            profiles: vec![Profile {
                id: "claude-work".into(),
                app: "claude".into(),
                label: "Work".into(),
                color: "#008080".into(),
                data_dir: "/tmp/x".into(),
                is_default: false,
            }],
        };
        assert_eq!(unique_id(&store, "claude-work"), "claude-work-2");
        assert_eq!(unique_id(&store, "claude-personal"), "claude-personal");
    }

    #[test]
    fn only_lane_folders_are_deletable() {
        let root = profiles_dir();
        assert!(is_deletable(&root.join("claude-work")));
        assert!(!is_deletable(&root));
        assert!(!is_deletable(&root.join("..")));
        assert!(!is_deletable(&root.join("../..")));
        assert!(!is_deletable(&root.join("claude-work/nested")));
        assert!(!is_deletable(&lanes_root()));
        assert!(!is_deletable(Path::new("/tmp/claude-work")));
    }
}
