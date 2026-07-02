//! The entire mechanism, in one place.
//!
//! Spawn the app's executable **directly** with `--user-data-dir=<profile dir>`.
//! Directly — NOT via macOS `open`, which routes through LaunchServices and would
//! just focus the already-running instance instead of starting a fresh isolated one.

use std::path::Path;
use std::process::{Command, Stdio};

/// Launch `binary` pointed at `data_dir`. Detached: we never wait on the child, so
/// quitting Lanes never takes the launched app down with it.
pub fn launch(binary: &Path, data_dir: &Path) -> std::io::Result<u32> {
    std::fs::create_dir_all(data_dir)?;
    let child = Command::new(binary)
        .arg(format!("--user-data-dir={}", data_dir.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(child.id())
}

/// Every running process's full command line, one per entry. Best-effort: empty on
/// error. The caller matches against *known* data-dir strings rather than parsing
/// paths out of these, because data-dir paths contain spaces
/// (`~/Library/Application Support/…`) and can't be split on whitespace.
pub fn command_lines() -> Vec<String> {
    let Ok(output) = Command::new("ps").args(["-axo", "command="]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}
