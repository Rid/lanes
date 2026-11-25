//! Copy one lane's Claude setup into another lane.
//!
//! Claude Desktop keeps a lane's local state in its data dir: settings and MCP
//! servers, plugins, and the Code and Cowork session list. This copies those
//! from one lane into another, so both look the same once each is signed in.
//!
//! It never copies sign-in. Claude keeps its tokens in `config.json`, which is
//! read here only for the account and organisation IDs and is never copied, and
//! in Chromium's cookies and storage, which are not on the lists below. Chats
//! and projects live on Anthropic's servers with each account, so no local copy
//! can move them.
//!
//! Claude files sessions and per-account preferences under the signed-in
//! account and organisation, so the copy re-files them under the target lane's
//! IDs: in folder names, and where an ID is a JSON key or value. Paths (strings
//! starting with `/`) are left alone, so copied sessions keep working in their
//! original folders, which both lanes then share.
//!
//! This follows Claude's internal file layout, which an update can change.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

/// Folders copied whole, with IDs in their names re-filed.
const FOLDERS: &[&str] = &[
    "claude-code-sessions",
    "local-agent-mode-sessions",
    "side-session-notes",
    "git-shadow",
    "plugin-list",
    "Claude Extensions",
    "Claude Extensions Settings",
];

/// Settings files copied with IDs re-filed. The target's own copies are backed
/// up first, since these are replaced rather than merged.
const FILES: &[&str] = &[
    "claude_desktop_config.json",
    "ccd-session-presets.json",
    "cowork-enabled-cli-ops.json",
    "cowork-memory-resets.json",
    "git-worktrees.json",
];

/// Where Claude lists the Code sessions it shows in the sidebar.
const SESSIONS: &str = "claude-code-sessions";

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub account: String,
    pub org: String,
}

#[derive(serde::Serialize, Debug, Default, PartialEq)]
pub struct Summary {
    pub sessions: usize,
    pub files: usize,
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// The account and organisation a lane last signed in to, from its
/// `config.json`. Only these two IDs are read; the file's sign-in tokens are
/// never touched. `None` if it hasn't signed in, or if it lists more than one
/// organisation and so can't say which is current.
pub fn identity(data_dir: &Path) -> Option<Identity> {
    let text = fs::read_to_string(data_dir.join("config.json")).ok()?;
    let config: Value = serde_json::from_str(&text).ok()?;
    let account = config.get("lastKnownAccountUuid")?.as_str()?.to_string();
    let mut orgs = config
        .as_object()?
        .keys()
        .filter_map(|k| k.strip_prefix("dxt:allowlistEnabled:"))
        .filter(|o| is_uuid(o));
    let org = orgs.next()?.to_string();
    if orgs.next().is_some() || !is_uuid(&account) {
        return None;
    }
    Some(Identity { account, org })
}

/// Swaps the source lane's account and organisation IDs for the target's.
struct Refile {
    pairs: [(String, String); 2],
}

impl Refile {
    fn new(from: &Identity, to: &Identity) -> Self {
        Refile {
            pairs: [
                (from.account.clone(), to.account.clone()),
                (from.org.clone(), to.org.clone()),
            ],
        }
    }

    fn applies_to(&self, s: &str) -> bool {
        self.pairs.iter().any(|(a, b)| a != b && s.contains(a.as_str()))
    }

    fn text(&self, s: &str) -> String {
        self.pairs
            .iter()
            .fold(s.to_string(), |out, (a, b)| out.replace(a.as_str(), b))
    }

    /// Re-file IDs used as keys or values. Paths are left alone: they point at
    /// the source lane's folders, which the target shares.
    fn json(&self, v: &mut Value) -> bool {
        match v {
            Value::String(s) if !s.starts_with('/') && self.applies_to(s) => {
                *s = self.text(s);
                true
            }
            Value::Array(items) => {
                // Every item, not just up to the first change.
                let mut changed = false;
                for item in items {
                    changed |= self.json(item);
                }
                changed
            }
            Value::Object(map) => {
                let mut changed = false;
                for (key, mut value) in std::mem::take(map) {
                    changed |= self.json(&mut value);
                    let key = if !key.starts_with('/') && self.applies_to(&key) {
                        changed = true;
                        self.text(&key)
                    } else {
                        key
                    };
                    map.insert(key, value);
                }
                changed
            }
            _ => false,
        }
    }
}

/// Copy `from`'s Claude setup into `to`. `to`'s Claude must not be running.
/// Settings files it replaces are first saved under `backup_dir`.
pub fn copy_setup(
    from: &Path,
    from_id: &Identity,
    to: &Path,
    to_id: &Identity,
    backup_dir: &Path,
) -> io::Result<Summary> {
    let refile = Refile::new(from_id, to_id);
    let mut summary = Summary::default();

    for name in FILES {
        let src = from.join(name);
        if !src.is_file() {
            continue;
        }
        let dst = to.join(name);
        if dst.is_file() {
            fs::create_dir_all(backup_dir)?;
            fs::copy(&dst, backup_dir.join(name))?;
        }
        copy_file(&src, &dst, &refile, *name == "git-worktrees.json")?;
        summary.files += 1;
    }

    for name in FOLDERS {
        let src = from.join(name);
        if src.is_dir() {
            let dst = to.join(name);
            fs::create_dir_all(&dst)?;
            copy_folder(&src, &dst, &refile, *name == SESSIONS, &mut summary)?;
        }
    }
    Ok(summary)
}

/// Merge `src` into `dst`, re-filing IDs in names. Symlinks are skipped.
fn copy_folder(
    src: &Path,
    dst: &Path,
    refile: &Refile,
    count_sessions: bool,
    summary: &mut Summary,
) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".DS_Store" {
            continue;
        }
        let target = dst.join(refile.text(&name));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
            copy_folder(&entry.path(), &target, refile, count_sessions, summary)?;
        } else if kind.is_file() {
            copy_file(&entry.path(), &target, refile, false)?;
            summary.files += 1;
            if count_sessions && name.starts_with("local_") && name.ends_with(".json") {
                summary.sessions += 1;
            }
        }
    }
    Ok(())
}

/// Copy one file, re-filing IDs inside it if it's JSON that mentions them.
/// `drop_gc` removes `git-worktrees.json`'s clean-up bookkeeping, so the target
/// lane starts its own instead of acting on the source's.
fn copy_file(src: &Path, dst: &Path, refile: &Refile, drop_gc: bool) -> io::Result<()> {
    let is_json = src.extension().is_some_and(|e| e == "json");
    if !is_json {
        fs::copy(src, dst)?;
        return Ok(());
    }
    let raw = fs::read(src)?;
    let parsed = std::str::from_utf8(&raw)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    let Some(mut value) = parsed else {
        fs::write(dst, &raw)?; // not valid JSON: copy as it is
        return Ok(());
    };
    let mut changed = false;
    if drop_gc {
        changed |= value
            .as_object_mut()
            .is_some_and(|m| m.remove("untrackedDirGc").is_some());
    }
    changed |= refile.json(&mut value);
    if changed {
        fs::write(dst, serde_json::to_vec(&value).map_err(io::Error::other)?)
    } else {
        fs::write(dst, &raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    const SA: &str = "11111111-1111-1111-1111-111111111111";
    const SO: &str = "22222222-2222-2222-2222-222222222222";
    const TA: &str = "33333333-3333-3333-3333-333333333333";
    const TO: &str = "44444444-4444-4444-4444-444444444444";

    fn ids(account: &str, org: &str) -> Identity {
        Identity { account: account.into(), org: org.into() }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lanes-copy-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: PathBuf, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn config(account: &str, org: &str) -> String {
        json!({
            "lastKnownAccountUuid": account,
            format!("dxt:allowlistEnabled:{org}"): true,
            "oauth:tokenCache": "SECRET",
        })
        .to_string()
    }

    #[test]
    fn identity_reads_account_and_single_org() {
        let dir = scratch("identity");
        assert_eq!(identity(&dir), None);
        write(dir.join("config.json"), &config(SA, SO));
        assert_eq!(identity(&dir), Some(ids(SA, SO)));

        let mut two_orgs: Value = serde_json::from_str(&config(SA, SO)).unwrap();
        two_orgs[format!("dxt:allowlistEnabled:{TO}")] = json!(true);
        write(dir.join("config.json"), &two_orgs.to_string());
        assert_eq!(identity(&dir), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refile_swaps_ids_in_keys_and_values_but_not_paths() {
        let refile = Refile::new(&ids(SA, SO), &ids(TA, TO));
        let path = format!("/Users/x/Claude/scratch-workspaces/{SA}/{SO}/work");
        let mut v = json!({
            "byAccount": { SA: 1 },
            format!("{SO}:{SA}"): "x",
            "owner": SA,
            "cwd": path,
            path.clone(): true,
            "list": [SO, "keep"],
        });
        assert!(refile.json(&mut v));
        assert_eq!(
            v,
            json!({
                "byAccount": { TA: 1 },
                format!("{TO}:{TA}"): "x",
                "owner": TA,
                "cwd": path,
                path.clone(): true,
                "list": [TO, "keep"],
            })
        );
        let mut untouched = json!({ "cwd": path });
        assert!(!refile.json(&mut untouched));
    }

    #[test]
    fn copies_setup_refiled_without_sign_in() {
        let root = scratch("copy");
        let (from, to, backup) = (root.join("from"), root.join("to"), root.join("backup"));

        // Source: signed in, with sessions, settings and sign-in material.
        write(from.join("config.json"), &config(SA, SO));
        write(from.join("Cookies"), "SECRET");
        write(from.join("Local Storage/leveldb/000003.log"), "SECRET");
        let session = format!(r#"{{"cwd":"/Users/x/scratch-workspaces/{SA}/{SO}/s1","title":"Fix"}}"#);
        write(from.join(format!("{SESSIONS}/{SA}/{SO}/local_abc.json")), &session);
        write(from.join(format!("{SESSIONS}/{SA}/{SO}/deleted_def")), "");
        write(from.join(format!("local-agent-mode-sessions/skills-plugin/{SO}/{SA}/skills/a.md")), "skill");
        write(from.join("side-session-notes/local_abc/note.md"), "note");
        write(
            from.join("claude_desktop_config.json"),
            &json!({ "mcpServers": { "x": {} }, "preferences": { "byAccount": { SA: true } } }).to_string(),
        );
        write(
            from.join("git-worktrees.json"),
            &json!({ "worktrees": { "w": {} }, "untrackedDirGc": { "cwds": {} } }).to_string(),
        );

        // Target: signed in to another account, with its own settings file.
        write(to.join("config.json"), &config(TA, TO));
        write(to.join("claude_desktop_config.json"), r#"{"mine":true}"#);

        let summary = copy_setup(&from, &ids(SA, SO), &to, &ids(TA, TO), &backup).unwrap();
        assert_eq!(summary.sessions, 1);

        // Sessions re-filed under the target's account; their paths still shared.
        let copied = fs::read_to_string(to.join(format!("{SESSIONS}/{TA}/{TO}/local_abc.json"))).unwrap();
        assert_eq!(copied, session);
        assert!(to.join(format!("{SESSIONS}/{TA}/{TO}/deleted_def")).exists());
        assert!(to.join(format!("local-agent-mode-sessions/skills-plugin/{TO}/{TA}/skills/a.md")).exists());
        assert!(to.join("side-session-notes/local_abc/note.md").exists());

        // Settings re-filed; the target's old file backed up; GC state dropped.
        let settings: Value =
            serde_json::from_str(&fs::read_to_string(to.join("claude_desktop_config.json")).unwrap()).unwrap();
        assert_eq!(settings["preferences"]["byAccount"][TA], json!(true));
        assert_eq!(fs::read_to_string(backup.join("claude_desktop_config.json")).unwrap(), r#"{"mine":true}"#);
        let worktrees: Value =
            serde_json::from_str(&fs::read_to_string(to.join("git-worktrees.json")).unwrap()).unwrap();
        assert!(worktrees.get("untrackedDirGc").is_none());
        assert!(worktrees.get("worktrees").is_some());

        // Sign-in never copied: the target keeps its own config.json.
        assert!(!to.join("Cookies").exists());
        assert!(!to.join("Local Storage").exists());
        assert_eq!(identity(&to), Some(ids(TA, TO)));
        assert!(!fs::read_to_string(to.join("config.json")).unwrap().contains(SA));

        fs::remove_dir_all(root).unwrap();
    }
}
