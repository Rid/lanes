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

/// The set of `--user-data-dir` paths that currently have a live process, so the
/// UI can show which lanes are already open. Best-effort: if `pgrep` is missing or
/// errors, returns empty (the UI just shows nothing running).
pub fn running_data_dirs() -> Vec<String> {
    let Ok(output) = Command::new("pgrep")
        .arg("-fl")
        .arg("--user-data-dir=")
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut dirs: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.split("--user-data-dir=").nth(1) {
            // The path runs until the next argument (we never put spaces in profile ids).
            let path = rest.split_whitespace().next().unwrap_or("").to_string();
            if !path.is_empty() && !dirs.contains(&path) {
                dirs.push(path);
            }
        }
    }
    dirs
}
