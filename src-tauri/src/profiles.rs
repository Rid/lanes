//! Profile model + persistence.
//!
//! Profiles live in a small JSON file under the app's own config dir. Each profile
//! owns a directory that is handed to the launched app as its `--user-data-dir`.
//! Lanes only ever *chooses* that directory — it never reads, copies, or writes the
//! session/credential material the app stores inside it.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub id: String,
    pub app: String,
    pub label: String,
    pub color: String,
    #[serde(rename = "dataDir")]
    pub data_dir: String,
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

pub fn load() -> Store {
    match fs::read_to_string(config_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Store::default(),
    }
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
    let mut store = load();
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
    let mut store = load();
    let p = store
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("Profile not found.")?;
    p.label = label.to_string();
    save(&store).map_err(|e| format!("Couldn't save profiles: {e}"))
}

pub fn set_color(id: &str, color: &str) -> Result<(), String> {
    let mut store = load();
    let p = store
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("Profile not found.")?;
    p.color = color.to_string();
    save(&store).map_err(|e| format!("Couldn't save profiles: {e}"))
}

/// Remove a profile. If `delete_data`, also delete its data directory — but only
/// if it's safely inside our profiles dir (never an arbitrary path).
pub fn remove(id: &str, delete_data: bool) -> Result<(), String> {
    let mut store = load();
    let idx = store
        .profiles
        .iter()
        .position(|p| p.id == id)
        .ok_or("Profile not found.")?;
    let profile = store.profiles.remove(idx);
    save(&store).map_err(|e| format!("Couldn't save profiles: {e}"))?;

    if delete_data {
        let dir = PathBuf::from(&profile.data_dir);
        let safe = dir.starts_with(profiles_dir());
        if safe && dir.exists() {
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
            }],
        };
        assert_eq!(unique_id(&store, "claude-work"), "claude-work-2");
        assert_eq!(unique_id(&store, "claude-personal"), "claude-personal");
    }
}
