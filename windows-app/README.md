# OctoPad for Windows

Native Windows application (PySide6 / Qt6) for the OctoPad 8-key macropad.
It talks to the pad over USB Serial or Bluetooth Low Energy, runs the mapped
actions locally, and integrates with Home Assistant over its REST API — no web
server, no external scripts.

## Requirements

- Windows 10 or 11
- Python 3.9+ (from [python.org](https://www.python.org/) or the Microsoft Store)

## Install

Open PowerShell in this folder and run:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

This creates a `.venv`, installs the dependencies, and adds an OctoPad shortcut
to the Start Menu (plus a background/tray autostart entry).

Run it afterwards with the Start Menu shortcut, or:

```bat
run.bat
```

### Manual install

```bat
python -m venv .venv
.venv\Scripts\python -m pip install -r requirements.txt
.venv\Scripts\python main.py
```

## Features

- **Buttons** — map Single / Double / Hold on each of the 8 keys to one or more
  actions (shortcuts, text, media, audio-output switching, app launch, system
  presets, scripts, Home Assistant services/webhooks, delays, mode toggle).
- **Serial console** — live hardware log and a command box to send commands
  (`WHO`, `NAME:<text>`, `CMD:...`) to the pad.
- **Profiles** — import/export JSON profiles, built-in presets, and your own
  saved presets.
- **Settings** — debounce/click/hold timings, Home Assistant connection,
  application rescan, and device rename.

## Device discovery

The app identifies the pad by its OctoPad service UUID over BLE and by the
`HELLO:OCTOPAD:<name>:v<version>` handshake over serial. The user-visible device
name can be anything; rename it in **Settings → Device** (USB connection
required).

## Configuration location

- Config: `%APPDATA%\OctoPad\config.json`
- Home Assistant credentials: `%APPDATA%\OctoPad\ha_config.json`
- Saved presets: `%APPDATA%\OctoPad\presets\`

## Uninstall

```powershell
powershell -ExecutionPolicy Bypass -File .\uninstall.ps1
```

Your configuration in `%APPDATA%\OctoPad` is left untouched.

## License

MIT — see the [LICENSE](../LICENSE) in the repository root.
