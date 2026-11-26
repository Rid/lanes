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
//! Copying again into a lane that's been used since is safe: a session the
//! target has used more recently is kept, settings files are merged rather than
//! replaced, and any file a copy changes is backed up first.
//!
//! This follows Claude's internal file layout, which an update can change.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

/// Settings files merged into the target's own, with IDs re-filed.
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
    /// Sessions the target now has from the source (copied or already the same).
    pub sessions: usize,
    /// Sessions left as they were because the target had used them more recently.
    pub kept_newer: usize,
    /// Files written into the target.
    pub files: usize,
    /// claude.ai connectors the source uses that the target hasn't connected.
    /// These live with each account on claude.ai, so they can't be copied.
    pub connect: Vec<Connector>,
}

#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct Connector {
    pub name: String,
    pub url: String,
}

/// Connectors Claude provides to every account, which never need connecting.
fn built_in(url: &str) -> bool {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', ':'])
        .next()
        .unwrap_or("");
    ["anthropic.com", "claude.ai", "claude.com", "claudemcpcontent.com"]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// The claude.ai connectors a lane currently has, as recorded in its most
/// recently active Code session (Claude saves each connector's name, address
/// and tools there; no tokens). Empty if it hasn't run a Code session.
pub fn connectors(data_dir: &Path) -> Vec<Connector> {
    let Ok(accounts) = fs::read_dir(data_dir.join(SESSIONS)) else {
        return Vec::new();
    };
    let mut newest: Option<(i64, Vec<Connector>)> = None;
    let orgs = accounts.flatten().filter_map(|a| fs::read_dir(a.path()).ok()).flatten().flatten();
    for file in orgs.filter_map(|o| fs::read_dir(o.path()).ok()).flatten().flatten() {
        let name = file.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("local_") && name.ends_with(".json")) {
            continue;
        }
        let Some(session) = fs::read(file.path()).ok().and_then(|b| parse(&b)) else {
            continue;
        };
        let (Some(at), Some(list)) = (
            session.get("lastActivityAt").and_then(Value::as_i64),
            session.get("remoteMcpServersConfig").and_then(Value::as_array),
        ) else {
            continue;
        };
        if newest.as_ref().is_some_and(|(seen, _)| *seen >= at) {
            continue;
        }
        let found = list
            .iter()
            .filter_map(|s| {
                Some(Connector {
                    name: s.get("name")?.as_str()?.to_string(),
                    url: s.get("url")?.as_str()?.to_string(),
                })
            })
            .collect();
        newest = Some((at, found));
    }
    newest.map(|(_, list)| list).unwrap_or_default()
}

/// Connectors `from` uses that `to` hasn't connected, matched by address.
fn to_connect(from: &Path, to: &Path) -> Vec<Connector> {
    let have: Vec<String> = connectors(to).into_iter().map(|c| c.url).collect();
    connectors(from)
        .into_iter()
        .filter(|c| !built_in(&c.url) && !have.contains(&c.url))
        .collect()
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

fn parse(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(bytes).ok()
}

/// Merge the source's settings (`theirs`) into the target's (`ours`): what
/// only the target has is kept, and the source wins where both have a value.
/// Lists of items with an `id` merge by id; lists of plain values combine.
fn merge(ours: Value, theirs: Value) -> Value {
    match (ours, theirs) {
        (Value::Object(mut ours), Value::Object(theirs)) => {
            for (key, value) in theirs {
                let merged = match ours.remove(&key) {
                    Some(old) => merge(old, value),
                    None => value,
                };
                ours.insert(key, merged);
            }
            Value::Object(ours)
        }
        (Value::Array(ours), Value::Array(mut theirs)) => {
            let id = |v: &Value| v.get("id").and_then(Value::as_str).map(str::to_owned);
            let plain = |v: &Value| !v.is_object() && !v.is_array();
            if ours.iter().chain(&theirs).all(|v| id(v).is_some()) {
                let have: Vec<String> = theirs.iter().filter_map(id).collect();
                theirs.extend(ours.into_iter().filter(|v| !have.contains(&id(v).unwrap_or_default())));
            } else if ours.iter().chain(&theirs).all(plain) {
                for v in ours {
                    if !theirs.contains(&v) {
                        theirs.push(v);
                    }
                }
            }
            Value::Array(theirs)
        }
        (_, theirs) => theirs,
    }
}

/// When a session was last used, from its `lastActivityAt` (milliseconds).
fn last_activity(bytes: &[u8]) -> Option<i64> {
    parse(bytes)?.get("lastActivityAt")?.as_i64()
}

/// One copy run: the ID swap, where backups go, and the running totals.
struct Run<'a> {
    refile: Refile,
    backup_dir: &'a Path,
    /// Each lane's `scratch-workspaces`, where "No folder" sessions live.
    src_scratch: PathBuf,
    dst_scratch: PathBuf,
    summary: Summary,
}

impl Run<'_> {
    /// Point a copied "No folder" session at the target lane's own scratch
    /// folder. Claude only files a session under "This computer" when its
    /// `originCwd` is inside that lane's own `scratch-workspaces` (it compares
    /// paths, not files); otherwise each one shows as a separate folder.
    /// `cwd`, where the session runs and keeps its history, stays the shared
    /// original, and a link at the new path leads to that same folder.
    fn rehome_scratch(&self, bytes: Vec<u8>) -> io::Result<Vec<u8>> {
        let Some(mut session) = parse(&bytes) else {
            return Ok(bytes);
        };
        let Some(origin) = session.get("originCwd").and_then(Value::as_str) else {
            return Ok(bytes);
        };
        let Ok(rest) = Path::new(origin).strip_prefix(&self.src_scratch) else {
            return Ok(bytes);
        };
        let parts: Vec<_> = rest.components().collect();
        if parts.len() < 3 {
            return Ok(bytes);
        }
        // <account>/<org>/<scratch-…>, re-filed under the target's IDs.
        let folder: PathBuf = parts[..3].iter().collect();
        let link = self
            .dst_scratch
            .join(self.refile.text(&folder.to_string_lossy()));
        if fs::symlink_metadata(&link).is_err() {
            if let Some(parent) = link.parent() {
                fs::create_dir_all(parent)?;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(self.src_scratch.join(&folder), &link)?;
        }
        let new_origin = parts[3..].iter().fold(link, |path, part| path.join(part));
        session["originCwd"] = Value::String(new_origin.to_string_lossy().into_owned());
        serde_json::to_vec(&session).map_err(io::Error::other)
    }

    /// Write `bytes` to `dst` (at `rel` inside the lane). Leaves an identical
    /// file alone; otherwise saves what was there under the backup folder first.
    fn put(&mut self, dst: &Path, rel: &Path, bytes: &[u8]) -> io::Result<()> {
        if let Ok(existing) = fs::read(dst) {
            if existing == bytes {
                return Ok(());
            }
            let backup = self.backup_dir.join(rel);
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(backup, existing)?;
        }
        fs::write(dst, bytes)?;
        self.summary.files += 1;
        Ok(())
    }

    /// A source file's contents with IDs re-filed, if it's JSON that has them.
    fn refiled(&self, src: &Path) -> io::Result<Vec<u8>> {
        let raw = fs::read(src)?;
        let is_json = src.extension().is_some_and(|e| e == "json");
        if !is_json || !self.refile.applies_to(&String::from_utf8_lossy(&raw)) {
            return Ok(raw);
        }
        let Some(mut value) = parse(&raw) else {
            return Ok(raw);
        };
        if self.refile.json(&mut value) {
            serde_json::to_vec(&value).map_err(io::Error::other)
        } else {
            Ok(raw)
        }
    }

    /// Merge a settings file into the target's own copy. `git-worktrees.json`'s
    /// clean-up bookkeeping (`untrackedDirGc`) stays the target's: the source's
    /// is about its own folders.
    fn merge_settings(&mut self, src: &Path, dst: &Path, rel: &Path) -> io::Result<()> {
        let incoming = self.refiled(src)?;
        let Some(mut theirs) = parse(&incoming) else {
            return self.put(dst, rel, &incoming); // not valid JSON: copy as it is
        };
        if let Some(map) = theirs.as_object_mut() {
            map.remove("untrackedDirGc");
        }
        let ours = fs::read(dst).ok().and_then(|b| parse(&b));
        let merged = match ours {
            // A different schemaVersion means Claude changed the format: don't mix them.
            Some(ours) if ours.get("schemaVersion") == theirs.get("schemaVersion") => merge(ours, theirs),
            _ => theirs,
        };
        self.put(dst, rel, &serde_json::to_vec(&merged).map_err(io::Error::other)?)
    }

    /// Merge the folder `src` into `dst`, re-filing IDs in names. Symlinks are
    /// skipped. In the session list, a session the target used more recently
    /// is kept as it is.
    fn copy_folder(&mut self, src: &Path, dst: &Path, rel: &Path, sessions: bool) -> io::Result<()> {
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".DS_Store" {
                continue;
            }
            let refiled_name = self.refile.text(&name);
            let (target, rel) = (dst.join(&refiled_name), rel.join(&refiled_name));
            let kind = entry.file_type()?;
            if kind.is_dir() {
                fs::create_dir_all(&target)?;
                self.copy_folder(&entry.path(), &target, &rel, sessions)?;
            } else if kind.is_file() {
                let mut bytes = self.refiled(&entry.path())?;
                let is_session = sessions && name.starts_with("local_") && name.ends_with(".json");
                if is_session {
                    let existing = fs::read(&target).ok();
                    let ours = existing.as_deref().and_then(last_activity);
                    if ours.is_some_and(|ours| last_activity(&bytes).is_some_and(|theirs| ours > theirs)) {
                        // Keep the target's newer copy, but still file it correctly.
                        self.summary.kept_newer += 1;
                        let kept = self.rehome_scratch(existing.unwrap_or_default())?;
                        self.put(&target, &rel, &kept)?;
                        continue;
                    }
                    self.summary.sessions += 1;
                    bytes = self.rehome_scratch(bytes)?;
                }
                self.put(&target, &rel, &bytes)?;
            }
        }
        Ok(())
    }
}

/// Copy `from`'s Claude setup into `to`. `to`'s Claude must not be running.
/// Every file the copy changes in `to` is saved under `backup_dir` first.
pub fn copy_setup(
    from: &Path,
    from_id: &Identity,
    to: &Path,
    to_id: &Identity,
    backup_dir: &Path,
) -> io::Result<Summary> {
    let mut run = Run {
        refile: Refile::new(from_id, to_id),
        backup_dir,
        src_scratch: from.join("scratch-workspaces"),
        dst_scratch: to.join("scratch-workspaces"),
        // Before copying: copied sessions would otherwise hide the target's own list.
        summary: Summary {
            connect: to_connect(from, to),
            ..Summary::default()
        },
    };
    for name in FILES {
        let src = from.join(name);
        if src.is_file() {
            run.merge_settings(&src, &to.join(name), Path::new(name))?;
        }
    }
    for name in FOLDERS {
        let src = from.join(name);
        if src.is_dir() {
            let dst = to.join(name);
            fs::create_dir_all(&dst)?;
            run.copy_folder(&src, &dst, Path::new(name), *name == SESSIONS)?;
        }
    }
    Ok(run.summary)
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

    fn read_json(path: PathBuf) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn config(account: &str, org: &str) -> String {
        json!({
            "lastKnownAccountUuid": account,
            format!("dxt:allowlistEnabled:{org}"): true,
            "oauth:tokenCache": "SECRET",
        })
        .to_string()
    }

    fn session(activity: i64, title: &str) -> String {
        json!({ "cwd": format!("/Users/x/scratch-workspaces/{SA}/{SO}/s1"), "lastActivityAt": activity, "title": title })
            .to_string()
    }

    #[test]
    fn no_folder_sessions_are_filed_under_the_targets_scratch_folder() {
        let root = scratch("rehome");
        let (from, to) = (root.join("from"), root.join("to"));
        let folder = "scratch-2026-01-01-abcdef";
        let src_dir = from.join(format!("scratch-workspaces/{SA}/{SO}/{folder}"));
        fs::create_dir_all(&src_dir).unwrap();
        let src_path = src_dir.to_string_lossy().into_owned();
        let scratch_session = |activity: i64| {
            json!({ "cwd": src_path, "originCwd": src_path, "lastActivityAt": activity }).to_string()
        };
        let in_sessions = |acct: &str, org: &str, name: &str| format!("{SESSIONS}/{acct}/{org}/{name}");

        write(from.join("config.json"), &config(SA, SO));
        write(from.join(in_sessions(SA, SO, "local_new.json")), &scratch_session(100));
        write(from.join(in_sessions(SA, SO, "local_used.json")), &scratch_session(100));
        write(
            from.join(in_sessions(SA, SO, "local_repo.json")),
            &json!({ "cwd": "/Users/x/repo", "originCwd": "/Users/x/repo", "lastActivityAt": 1 }).to_string(),
        );
        // Copied earlier by an older Lanes, then used: newer, but filed wrongly.
        write(to.join("config.json"), &config(TA, TO));
        write(to.join(in_sessions(TA, TO, "local_used.json")), &scratch_session(200));

        let summary = copy_setup(&from, &ids(SA, SO), &to, &ids(TA, TO), &root.join("backup")).unwrap();
        assert_eq!(summary.kept_newer, 1);

        let link = to.join(format!("scratch-workspaces/{TA}/{TO}/{folder}"));
        assert_eq!(fs::read_link(&link).unwrap(), src_dir);
        for name in ["local_new.json", "local_used.json"] {
            let s = read_json(to.join(in_sessions(TA, TO, name)));
            assert_eq!(s["originCwd"], json!(link.to_string_lossy()), "{name}");
            assert_eq!(s["cwd"], json!(src_path), "{name}: runs in the shared folder");
        }
        assert_eq!(read_json(to.join(in_sessions(TA, TO, "local_used.json")))["lastActivityAt"], json!(200));
        assert_eq!(read_json(to.join(in_sessions(TA, TO, "local_repo.json")))["originCwd"], json!("/Users/x/repo"));

        // Copying again changes nothing.
        let again = copy_setup(&from, &ids(SA, SO), &to, &ids(TA, TO), &root.join("backup2")).unwrap();
        assert_eq!(again.files, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lists_connectors_the_target_still_needs() {
        let root = scratch("connectors");
        let (from, to) = (root.join("from"), root.join("to"));
        let at = |dir: &Path, acct: &str, org: &str, name: &str, activity: i64, urls: &[(&str, &str)]| {
            let list: Vec<Value> = urls.iter().map(|(n, u)| json!({ "uuid": "x", "name": n, "url": u, "tools": [] })).collect();
            write(
                dir.join(format!("{SESSIONS}/{acct}/{org}/{name}")),
                &json!({ "lastActivityAt": activity, "remoteMcpServersConfig": list }).to_string(),
            );
        };
        let gmail = ("Gmail", "https://gmailmcp.googleapis.com/mcp/v1");
        let drive = ("Google Drive", "https://drivemcp.googleapis.com/mcp/v1");
        let posthog = ("PostHog", "https://mcp.posthog.com/mcp");
        let docs = ("Claude Docs", "https://api.anthropic.com/v1/pages/mcp");
        // Source: an old session with Drive only, the newest with Gmail, PostHog and a built-in.
        at(&from, SA, SO, "local_old.json", 1, &[drive]);
        at(&from, SA, SO, "local_new.json", 9, &[gmail, posthog, docs]);
        // Target already has PostHog.
        at(&to, TA, TO, "local_t.json", 5, &[posthog, docs]);

        let names: Vec<String> = to_connect(&from, &to).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Gmail"]);
        // A lane with no sessions yet needs everything that isn't built in.
        let fresh: Vec<String> = to_connect(&from, &root.join("fresh")).into_iter().map(|c| c.name).collect();
        assert_eq!(fresh, ["Gmail", "PostHog"]);
        assert!(built_in("https://sandbox.claudemcpcontent.com/imagine_mcp"));
        assert!(!built_in("https://evilanthropic.com/mcp"));
        fs::remove_dir_all(root).unwrap();
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
    fn merge_keeps_target_only_entries_and_source_wins_conflicts() {
        let ours = json!({
            "mine": true,
            "shared": "old",
            "presets": [{ "id": "a", "name": "A old" }, { "id": "t", "name": "Target only" }],
            "origins": ["https://t.example", "https://both.example"],
            "nested": { "keep": 1, "swap": 1 },
        });
        let theirs = json!({
            "shared": "new",
            "presets": [{ "id": "a", "name": "A new" }, { "id": "s", "name": "Source only" }],
            "origins": ["https://both.example", "https://s.example"],
            "nested": { "swap": 2 },
        });
        assert_eq!(
            merge(ours, theirs),
            json!({
                "mine": true,
                "shared": "new",
                "presets": [{ "id": "a", "name": "A new" }, { "id": "s", "name": "Source only" }, { "id": "t", "name": "Target only" }],
                "origins": ["https://both.example", "https://s.example", "https://t.example"],
                "nested": { "keep": 1, "swap": 2 },
            })
        );
    }

    #[test]
    fn copies_setup_refiled_without_sign_in() {
        let root = scratch("copy");
        let (from, to, backup) = (root.join("from"), root.join("to"), root.join("backup"));

        // Source: signed in, with sessions, settings and sign-in material.
        write(from.join("config.json"), &config(SA, SO));
        write(from.join("Cookies"), "SECRET");
        write(from.join("Local Storage/leveldb/000003.log"), "SECRET");
        write(from.join(format!("{SESSIONS}/{SA}/{SO}/local_abc.json")), &session(100, "Fix"));
        write(from.join(format!("{SESSIONS}/{SA}/{SO}/deleted_def")), "");
        write(from.join(format!("local-agent-mode-sessions/skills-plugin/{SO}/{SA}/skills/a.md")), "skill");
        write(from.join("side-session-notes/local_abc/note.md"), "note");
        write(
            from.join("claude_desktop_config.json"),
            &json!({ "mcpServers": { "x": {} }, "preferences": { "byAccount": { SA: true } } }).to_string(),
        );
        write(
            from.join("git-worktrees.json"),
            &json!({ "worktrees": { "w": {} }, "untrackedDirGc": { "cwds": { "src": 1 } } }).to_string(),
        );

        // Target: signed in to another account, with its own settings file.
        write(to.join("config.json"), &config(TA, TO));
        write(to.join("claude_desktop_config.json"), r#"{"mine":true}"#);

        let summary = copy_setup(&from, &ids(SA, SO), &to, &ids(TA, TO), &backup).unwrap();
        assert_eq!((summary.sessions, summary.kept_newer), (1, 0));

        // Sessions re-filed under the target's account; their paths still shared.
        let copied = fs::read_to_string(to.join(format!("{SESSIONS}/{TA}/{TO}/local_abc.json"))).unwrap();
        assert_eq!(copied, session(100, "Fix"));
        assert!(to.join(format!("{SESSIONS}/{TA}/{TO}/deleted_def")).exists());
        assert!(to.join(format!("local-agent-mode-sessions/skills-plugin/{TO}/{TA}/skills/a.md")).exists());
        assert!(to.join("side-session-notes/local_abc/note.md").exists());

        // Settings merged and re-filed, the target's old file backed up; the
        // source's clean-up bookkeeping not copied.
        let settings = read_json(to.join("claude_desktop_config.json"));
        assert_eq!(settings["preferences"]["byAccount"][TA], json!(true));
        assert_eq!(settings["mine"], json!(true));
        assert_eq!(fs::read_to_string(backup.join("claude_desktop_config.json")).unwrap(), r#"{"mine":true}"#);
        let worktrees = read_json(to.join("git-worktrees.json"));
        assert!(worktrees.get("untrackedDirGc").is_none());
        assert!(worktrees.get("worktrees").is_some());

        // Sign-in never copied: the target keeps its own config.json.
        assert!(!to.join("Cookies").exists());
        assert!(!to.join("Local Storage").exists());
        assert_eq!(identity(&to), Some(ids(TA, TO)));
        assert!(!fs::read_to_string(to.join("config.json")).unwrap().contains(SA));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copying_again_keeps_newer_target_work_and_backs_up_changes() {
        let root = scratch("recopy");
        let (from, to) = (root.join("from"), root.join("to"));
        let sessions = |acct: &str, org: &str| format!("{SESSIONS}/{acct}/{org}");

        write(from.join("config.json"), &config(SA, SO));
        write(from.join(format!("{}/local_newer_in_target.json", sessions(SA, SO))), &session(100, "Source"));
        write(from.join(format!("{}/local_newer_in_source.json", sessions(SA, SO))), &session(300, "Source"));
        write(from.join(format!("{}/local_same.json", sessions(SA, SO))), &session(50, "Same"));
        write(
            from.join("git-worktrees.json"),
            &json!({ "schemaVersion": 2, "worktrees": { "from-source": {} }, "untrackedDirGc": { "cwds": { "src": 1 } } })
                .to_string(),
        );
        write(
            from.join("ccd-session-presets.json"),
            &json!({ "byAccount": { SA: { "presets": [{ "id": "p1", "name": "Source" }] } } }).to_string(),
        );

        // Target: already copied once, then used — it has newer and its own work.
        write(to.join("config.json"), &config(TA, TO));
        write(to.join(format!("{}/local_newer_in_target.json", sessions(TA, TO))), &session(200, "Target"));
        write(to.join(format!("{}/local_newer_in_source.json", sessions(TA, TO))), &session(150, "Target"));
        write(to.join(format!("{}/local_same.json", sessions(TA, TO))), &session(50, "Same"));
        write(to.join(format!("{}/local_target_only.json", sessions(TA, TO))), &session(10, "Mine"));
        write(
            to.join("git-worktrees.json"),
            &json!({ "schemaVersion": 2, "worktrees": { "from-target": {} }, "untrackedDirGc": { "cwds": { "dst": 1 } } })
                .to_string(),
        );
        write(
            to.join("ccd-session-presets.json"),
            &json!({ "byAccount": { TA: { "presets": [{ "id": "p2", "name": "Target" }] } } }).to_string(),
        );

        let backup = root.join("backup");
        let summary = copy_setup(&from, &ids(SA, SO), &to, &ids(TA, TO), &backup).unwrap();
        assert_eq!((summary.sessions, summary.kept_newer), (2, 1));

        let title = |name: &str| read_json(to.join(format!("{}/{name}", sessions(TA, TO))))["title"].clone();
        assert_eq!(title("local_newer_in_target.json"), json!("Target"));
        assert_eq!(title("local_newer_in_source.json"), json!("Source"));
        assert_eq!(title("local_target_only.json"), json!("Mine"));

        // Only files that changed are backed up: the older session, not the identical one.
        let backed_up = |name: &str| backup.join(format!("{}/{name}", sessions(TA, TO))).exists();
        assert!(backed_up("local_newer_in_source.json"));
        assert!(!backed_up("local_same.json"));
        assert!(!backed_up("local_newer_in_target.json"));

        // Worktrees and presets merged; the target keeps its own clean-up bookkeeping.
        let worktrees = read_json(to.join("git-worktrees.json"));
        assert!(worktrees["worktrees"].get("from-source").is_some());
        assert!(worktrees["worktrees"].get("from-target").is_some());
        assert_eq!(worktrees["untrackedDirGc"], json!({ "cwds": { "dst": 1 } }));
        let presets = read_json(to.join("ccd-session-presets.json"));
        let names: Vec<&str> = presets["byAccount"][TA]["presets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Source", "Target"]);

        fs::remove_dir_all(root).unwrap();
    }
}
