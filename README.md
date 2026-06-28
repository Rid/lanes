<div align="center">

# Lanes

**Two accounts. No logout.**

A tiny, free macOS menu-bar launcher that opens your AI desktop apps — each signed into a
different account, side by side. Work and personal, running at once.

<img src="docs/panel.svg" alt="The Lanes menu-bar panel showing three Claude lanes — Work, Personal, and Client — with two Claude windows open side by side on different accounts" width="620">

<sub>Built by <a href="https://synthetixis.com">Synthetixis</a> · free &amp; open source · macOS</sub>

</div>

---

## All it does, in one line

```
/Applications/Claude.app/Contents/MacOS/Claude --user-data-dir="~/Library/Application Support/Lanes/profiles/claude-work"
```

Lanes opens the **official** app pointed at a per-profile data directory. Each profile keeps its
own session in its own folder, so two accounts run as two independent windows. That's the whole
trick — and the whole product.

## It never touches your tokens

This is the entire safety story, and it's not a slogan — it's the mechanism:

- **No token handling.** Lanes only *chooses a directory* and *spawns a process*. It never reads,
  copies, moves, or writes your login/session material. Other account switchers shuffle
  credentials around; Lanes doesn't have to.
- **No network, ever.** Zero analytics, zero phone-home, zero update checks. Lanes makes no network
  requests at all. Everything happens locally on your Mac.
- **Menu-bar only.** No Dock icon, no main window, no background services. The dropdown is the
  entire app. It's about 3 MB.
- **Open source.** Every line is here. Read it, build it yourself, or trust that others can.

## Supported apps

| App | Status | Why |
|-----|--------|-----|
| **Claude Desktop** | ✅ Supported | Electron; honors `--user-data-dir`, so each profile gets a fully isolated, persistent session. |
| ChatGPT | ❌ Not supported (v0) | Native (non-Electron) app — there's no per-profile data-dir switch to point it at. |
| Codex | ❌ Not supported (v0) | Electron, but it ignores `--user-data-dir` and writes to a fixed location, so two instances share one login. |

The app list is a single data table in [`src-tauri/src/apps.rs`](src-tauri/src/apps.rs). If a future
app version exposes a working isolation path, adding it is one line — no other code changes.

## Install

1. Download `Lanes_x.y.z_aarch64.dmg` from [Releases](https://github.com/synthetixis/lanes/releases).
2. Open it and drag **Lanes** into your Applications folder.
3. Launch it. Lanes appears in your **menu bar** (no Dock icon).

> **Gatekeeper note:** v0 builds are ad-hoc signed, not yet notarized. On first launch macOS may
> warn that the developer can't be verified — right-click the app → **Open**, or allow it under
> **System Settings → Privacy & Security**. (See [Distribution](#distribution) for the notarization
> hook.)

## How to use

1. Click the **Lanes** icon in the menu bar.
2. **+ Add profile** → pick Claude, name the lane (`Work`, `Personal`, a client name…), pick a color.
3. Click the lane. The real Claude app opens on that account. Click another and they run side by
   side. Sign in once per lane — the login stays put.
4. **Manage** lets you rename or remove a lane. Removing offers to delete that lane's data folder,
   naming the exact path first — nothing is erased without an explicit confirmation.

Profiles live in `~/Library/Application Support/Lanes/` (a small `lanes.json` plus one folder per
profile). Lanes never looks inside those folders — it just hands the path to the app.

## Build from source

Requires [Rust](https://rustup.rs) and Node.js.

```bash
npm install
npm run tauri dev      # run locally
npm run tauri build    # produce Lanes.app + a .dmg in src-tauri/target/release/bundle/
```

Stack: [Tauri v2](https://tauri.app) (Rust core + system tray + a small webview for the dropdown).
The privileged surface is a handful of vetted Rust commands; the UI is plain HTML/CSS/JS reusing the
Synthetixis brand tokens. No UI framework, no network plugins, no auto-updater.

```
src-tauri/src/
  apps.rs        # known-apps table (data-driven) + install detection
  profiles.rs    # JSON load/save, data-dir create/delete, atomic writes
  launch.rs      # spawn the app binary directly with --user-data-dir (never `open`)
  lib.rs         # tray, panel positioning, the command surface
ui/              # index.html · panel.css · panel.js  (the dropdown)
```

## Distribution

To ship a build users can open without Gatekeeper friction, code-sign and notarize the `.dmg`. With
an Apple Developer ID, set the signing identity in `src-tauri/tauri.conf.json` under
`bundle.macOS.signingIdentity` and provide notarization credentials via the standard
`APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` environment variables, then run `npm run tauri build`.
See the [Tauri macOS signing guide](https://tauri.app/distribute/sign/macos/).

## Honest about what this is

Lanes is an **unofficial** personal-productivity utility. It is **not affiliated with, endorsed by,
or supported by Anthropic or OpenAI**. It launches official apps you already have installed; it does
not modify or repackage them.

Running multiple accounts may be subject to your plan's terms — **check them before doing so,
especially on Team or Enterprise plans.**

## License

MIT © Synthetixis. See [LICENSE](LICENSE).
