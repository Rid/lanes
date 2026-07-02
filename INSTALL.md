# Installing Lanes

Lanes is a free, independent app that isn't notarized by Apple yet, so macOS asks you to confirm it
**once** on first launch. It's a quick, one-time step — here's how.

## Install

1. Download **`Lanes-0.1.0.dmg`**.
2. Open it and drag **Lanes** into your **Applications** folder.
3. Double-click **Lanes** in Applications.

## First launch — "Open Anyway" (no Terminal)

The first time you open Lanes, macOS blocks it (*"Lanes can't be opened because Apple cannot check
it for malicious software"*, or *"Lanes is damaged"*). This is expected for any app without a paid
Apple certificate — it isn't actually damaged.

To allow it:

1. Open  **System Settings → Privacy & Security**.
2. Scroll to the bottom. You'll see a message like *"Lanes was blocked to protect your Mac."*
3. Click **Open Anyway**, then confirm with **Open**.

That's it — Lanes launches from your **menu bar** (no Dock icon), and you'll never see the prompt
again.

## If it still says "damaged"

On some Macs the GUI step above isn't offered. Open **Terminal** (Applications → Utilities →
Terminal), paste this line, press Return, then open Lanes again:

```bash
xattr -dr com.apple.quarantine /Applications/Lanes.app
```

This just removes the "downloaded from the internet" quarantine flag macOS put on the file.

## Why the prompt at all?

The warning disappears once an app is signed and notarized with an Apple Developer ID, which is a
paid certificate. Lanes is free and open source — you can [read every line](https://github.com/synthetixis/lanes)
or build it yourself:

```bash
npm install
npm run tauri build
```

## Uninstall

Drag **Lanes** from Applications to the Trash. To also remove your saved profiles and lane data,
delete `~/Library/Application Support/Lanes`. (Your normal Claude login lives in
`~/Library/Application Support/Claude` and is untouched by this.)
