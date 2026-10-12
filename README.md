# 🎛️ OctoPad v2.6

[![ESP32](https://img.shields.io/badge/Microcontroller-ESP32--C3-red.svg?logo=espressif)](https://www.espressif.com/)
[![Connectivity](https://img.shields.io/badge/Connectivity-BLE%20HID%20%7C%20USB--Serial-blue.svg)](https://github.com/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Windows-0078D6.svg?logo=windows&logoColor=white)](https://github.com/SamVivan1/octopad)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**OctoPad v2.6** is an 8-key wireless mechanical macropad based on the **ESP32-C3 SuperMini** with **Bluetooth Low Energy (BLE HID)** and **USB Serial** connectivity. It is complemented by **native desktop applications** — **GTK4 + Libadwaita on Linux** and **PySide6/Qt6 on Windows** — for configuring keys, monitoring hardware events, and **Home Assistant** integration — no web server, no Docker.

---

## ✨ Key Features

- **8 Mechanical Keys with Multi-Trigger**:
  - **Single Click (1x)**: Executes a standard shortcut with instant response.
  - **Double Click (2x)**: Detects a double click with LED feedback blinking twice.
  - **Hold / Long Press (HOLD)**: Press and hold a key for > 450ms with LED feedback blinking three times.
- **Dual Layer / Mode Switcher**:
  - **Desktop Mode**: Daily shortcuts, navigation, and OS productivity (Status LED ON).
  - **Home Assistant Mode**: IoT shortcuts and smart home automation (Status LED OFF).
- **Dedicated System Key (B1 / GPIO 20)**:
  - Single Click: Toggle layer (*Desktop Mode* ↔ *Home Assistant Mode*).
  - Hold / Long Press: Automatically types a passcode/string (`031004`).
- **Native Desktop Application (GTK4 + Libadwaita)**:
  - Visual interactive 8-key mechanical chassis with a per-trigger action inspector (1x / 2x / hold).
  - **Live Hardware Event Monitor**: on-screen key press animations in real time when the physical switch is pressed.
  - **Serial Console**: hardware log + send commands to the macropad from within the app.
  - **Profiles** page: save, import & export key mapping profiles (`.json`) + built-in presets.
  - **Serial** and **Home Assistant** status pills directly in the header.
- **Native Linux Ubuntu Support**:
  - Ready to connect via BLE Keyboard or a USB Serial cable.
  - **Native desktop application** (GTK4 + Libadwaita) — just like Vial / OpenRGB, not a background daemon.
  - Ubuntu `.desktop` application scanner + button launcher without editing scripts.
  - **Native Home Assistant**: call HA services (`light.toggle`, `automation.trigger`, ...)
    directly from the application via the REST API — no bash script, no `curl`.
  - Fallback to webhook & bash script modes if still needed.

---

## 📌 Hardware Specifications & Pinout

Uses an **ESP32-C3 SuperMini** microcontroller with an active-LOW switch configuration (*Active-LOW / Internal Pull-Up*):

| Component | Physical Pin (GPIO) | Default Function | Description |
| :--- | :---: | :--- | :--- |
| **Status LED** | `GPIO 4` | Mode Indicator & Blink Feedback | ON = Desktop Mode, OFF = HA Mode |
| **Button 1** | `GPIO 20` | Mode Switch / Passcode String | Single: Toggle Mode, Hold: String `"031004"` |
| **Button 2** | `GPIO 9` | Macro 1 | Desktop: Ctrl+Alt+Shift + 2 / Q / A |
| **Button 3** | `GPIO 2` | Macro 2 | Desktop: Ctrl+Alt+Shift + 3 / W / S |
| **Button 4** | `GPIO 1` | Macro 3 | Desktop: Ctrl+Alt+Shift + 4 / E / D |
| **Button 5** | `GPIO 21` | Macro 4 | Desktop: Ctrl+Alt+Shift + 5 / I / F |
| **Button 6** | `GPIO 10` | Macro 5 | Desktop: Ctrl+Alt+Shift + 6 / T / G |
| **Button 7** | `GPIO 3` | Macro 6 | Desktop: Ctrl+Alt+Shift + 7 / Y / H |
| **Button 8** | `GPIO 0` | Macro 7 | Desktop: Ctrl+Alt+Shift + 8 / U / J |

### ⏱️ Timing Parameters (Timing Engine)
* **Debounce Filter**: `25 ms` (filters mechanical switch bounce)
* **Click Timeout**: `250 ms` (maximum interval between taps for double-click detection)
* **Hold Timeout**: `450 ms` (press duration required to trigger the Long Press action)

---

## 🗺️ Default Mapping (Keymap Layers)

All default shortcuts are sent with the modifier combination: `Ctrl + Alt + Shift + <Key>`

### 1. Desktop Mode (LED GPIO 4: ON)
| Key | Single Click (1x) | Double Click (2x) | Hold / Long Press |
| :---: | :---: | :---: | :---: |
| **B1** | *Toggle to HA Mode* | *None* | Text: `"031004"` |
| **B2** | `Key 2` | `Key Q` | `Key A` |
| **B3** | `Key 3` | `Key W` | `Key S` |
| **B4** | `Key 4` | `Key E` | `Key D` |
| **B5** | `Key 5` | `Key I` | `Key F` |
| **B6** | `Key 6` | `Key T` | `Key G` |
| **B7** | `Key 7` | `Key Y` | `Key H` |
| **B8** | `Key 8` | `Key U` | `Key J` |

### 2. Home Assistant Mode (LED GPIO 4: OFF)
| Key | Single Click (1x) | Double Click (2x) | Hold / Long Press |
| :---: | :---: | :---: | :---: |
| **B1** | *Toggle to Desktop Mode* | *None* | Text: `"031004"` |
| **B2** | `Key F2` | `Key Z` | `Key 9` |
| **B3** | `Key F3` | `Key X` | `Key 0` |
| **B4** | `Key F4` | `Key C` | `Key I` |
| **B5** | `Key F5` | `Key V` | `Key O` |
| **B6** | `Key F6` | `Key B` | `Key P` |
| **B7** | `Key F7` | `Key N` | `Key K` |
| **B8** | `Key F8` | `Key P` | `Key L` |

---

## 📁 Repository Structure

```
octopad/
├── octopad.ino          # Arduino / ESP32-C3 firmware
├── README.md                 # Main repository documentation
├── VERSION                   # Application version (used by the packaging CI)
├── .github/workflows/        # CI: lint + tests + build .deb/.exe/firmware
├── tests/                    # pytest suite (config, parsers, TIMING sync)
├── scripts/                  # Developer helpers
│   ├── build_firmware.sh     # Build/flash firmware with arduino-cli
│   └── check.sh              # Run pyflakes + compileall + pytest (+ firmware)
├── packaging/deb/            # Debian package builder (build_deb.sh)
├── windows-app/              # Native Windows application (PySide6 / Qt6)
│   ├── octopad.spec          # PyInstaller build definition (.exe)
│   ├── main.py               # QApplication entry point
│   ├── config.py             # Profiles, presets, configuration validation
│   ├── state.py              # Runtime state bus (Qt signal bridge)
│   ├── ui/                   # Qt pages & dialogs
│   ├── app_scanner.py        # Installed application scanner (Start Menu)
│   ├── platform_actions.py   # Local action execution (shortcuts, audio, ...)
│   ├── home_assistant.py     # Home Assistant REST client
│   ├── serial_listener.py    # Serial listener + device identification
│   ├── ble_bridge.py         # BLE GATT client (bleak)
│   ├── tray.py               # System tray icon
│   ├── install.ps1           # Installer (PowerShell)
│   └── uninstall.ps1         # Uninstaller (PowerShell)
└── linux-app/                # Native desktop application (GTK4 + Libadwaita)
    ├── main.py               # Adw.Application entry point
    ├── config.py             # Profiles, presets, configuration validation
    ├── state.py              # Runtime state bus (no GTK)
    ├── ui/                   # GTK4 pages & dialogs
    │   ├── window.py         # Main window (sidebar + page stack)
    │   ├── keys_page.py      # 8-keycap grid + action inspector
    │   ├── action_editor.py  # Per-trigger action editor (1x / 2x / hold)
    │   ├── console_page.py   # Serial console (log + send commands)
    │   ├── settings_page.py  # Profiles & Settings page
    │   ├── ha_dialogs.py     # HA connection dialog & Entity Picker
    │   ├── app_picker.py     # Application picker (.desktop)
    │   └── css.py            # Custom styles following the GNOME HIG
    ├── app_scanner.py        # Installed application scanner (.desktop)
    ├── system_actions.py     # Local action execution (shortcuts, audio, text, ...)
    ├── home_assistant.py     # Home Assistant REST client (called directly)
    ├── serial_listener.py    # Serial listener + device identification + dispatcher
    ├── ble_bridge.py         # BLE GATT client (bluez D-Bus)
    ├── octopad.svg           # Official application vector icon
    ├── run.sh                # Application launcher script
    ├── install.sh            # Install into the Ubuntu Application Menu (Super Key)
    ├── uninstall.sh          # Cleanly remove the application & shortcut
    └── README.md             # Native application documentation
```

### 💻 Running as a Native Linux Application (Recommended)
This application runs like modern peripheral software (**Vial, Piper, Razer Synapse, OpenRGB**), not a daemon that burdens the background:
1. **Install into the Ubuntu Application Menu**:
   ```bash
   cd linux-app
   ./install.sh
   ```
2. Press the **Super / Windows** key on the keyboard, type **`OctoPad`**, and click the application! A native GTK4 window will open.
3. Or run it directly from the terminal:
   ```bash
   cd linux-app
   ./run.sh
   ```
4. When the window is closed, the application stops cleanly without leaving a background process.

Application pages:
- **Buttons** — 8-keycap grid, Desktop / Home Assistant mode switch, and per-trigger action inspector.
- **Serial Console** — real-time hardware log + command input to send commands to the macropad.
- **Profiles** — save / import / export JSON profiles + built-in presets.
- **Settings** — timing parameters (debounce / click / hold), Home Assistant connection, and application rescan.

### 🏠 Native Home Assistant Connection (Scriptless)
All actions of type **Home Assistant (Native)** are executed directly by the application
to `POST /api/services/<domain>/<service>` — replacing the old `ha-*.sh` scripts.
No IP, token, or entity is hardcoded in the source:
1. Open the application → click the **`HA: ...`** pill in the header (or **Connection** in the Key Inspector).
   The dialog opens immediately — status is read from cache, and the live check runs in the background.
2. Enter the HA URL + **Long-Lived Access Token** (HA → Profile → Security → Long-Lived Access Tokens).
3. **Test Connection** → **Save Connection** → the system automatically scans all interactive entities
   and opens the **Entity Picker** (choose HA, then choose an entity → attach it to a key).
4. Choose a service (`toggle`, `turn_on`, `trigger`, `press`, ...), then **Test** to execute.
5. The token is stored in `~/.config/octopad/ha_config.json`
   (fallback read: `~/.config/home-assistant/env`); entity scan results are cached in
   `~/.config/octopad/ha_entities_cache.json` so they remain visible while HA is offline.
6. All calls are made directly from the Python module `home_assistant.py` —
   there is no local HTTP endpoint or additional service running in the background.

### 🪟 Running as a Native Windows Application (Recommended)
The Windows port mirrors the Linux app feature-for-feature with **PySide6 / Qt6**
and needs **Python 3.10+** (required by `bleak` 1.x and later):
1. **Install** — open PowerShell in `windows-app` and run:
   ```powershell
   powershell -ExecutionPolicy Bypass -File .\install.ps1
   ```
   This creates a virtual environment, installs the dependencies, and adds a
   Start Menu shortcut (plus a background/tray autostart entry).
2. Launch **OctoPad** from the Start Menu, or run `run.bat`.
3. Closing the window keeps the app running in the **system tray**; use the tray
   menu to reopen it or quit.

The device talks over USB Serial or **Bluetooth LE** (via `bleak`), identifies
the pad through the same `HELLO:OCTOPAD:<name>:v<version>` handshake and BLE
service UUID as the firmware, and stores its configuration in
`%APPDATA%\OctoPad\`. See [`windows-app/README.md`](windows-app/README.md) for
details.

### 📦 Prebuilt Packages & CI/CD
Every update triggers the [`Build`](.github/workflows/build.yml) workflow, which
produces a **`.deb`** for Linux (statically built with `packaging/deb/build_deb.sh`),
a **standalone `.exe`** for Windows (built with PyInstaller) and the **ESP32
firmware `.bin`** (compiled with `arduino-cli` for the ESP32-C3). Push a tag like
`v2.6.0` and all three files are attached to the GitHub **Release**
automatically. Pull requests that touch `octopad.ino` are compile-checked by the
[`Firmware`](.github/workflows/firmware.yml) workflow.

- **Linux (.deb)** — install with:
  ```bash
  sudo apt install ./octopad_*_all.deb
  ```
  Pulls the GTK4 / Libadwaita dependencies, adds the *OctoPad* entry to the
  application menu and a background/tray autostart entry. Remove with
  `sudo apt remove octopad`.
- **Windows (.exe)** — download `octopad_*_windows-x64.exe` and run it; no Python
  installation required.

Workflow artifacts (from any `main` build) are available on the run's page under
**Actions → Build → Artifacts**.

### 🏷️ Device Naming & Identification (BYO board)
Your DIY build does not have to be called *OctoPad*. The firmware and app agree
on a small identity protocol so any name works:

- The firmware advertises a **project-unique Bluetooth service UUID**
  (`6f63746f-7061-4430-8001-000000000001`; the prefix spells “octo"), and prints
  an identity line on boot / on demand:
  `HELLO:OCTOPAD:<device-name>:v<version>` (over USB serial and as the TX value
  over Bluetooth). The app recognises the pad by this signature — not by name.
- **Rename your pad** from the app: **Settings → Device → Rename…** (over USB
  serial). The name is stored on the ESP32 (NVS) and survives reflashing the app.
  From the console you can also send `NAME:My Pad`.
- Device commands (accepted over USB serial **and** the Bluetooth RX
  characteristic):

  | Command | Reply | Effect |
  | --- | --- | --- |
  | `WHO` / `VERSION` | `HELLO:…` / `VERSION:…` | Print the identity line |
  | `HELP` | `HELP:…` | List the supported commands |
  | `NAME:<text>` | `NAME:OK:<text>` | Rename + persist the pad |
  | `TIMING:<d>,<c>,<h>` | `TIMING:OK:…` | Set debounce / click / hold (ms) |
  | `MODE:DESKTOP\|HA` | `MODE: …` | Switch layer + persist |
  | `SAVE` | `SAVED` | Persist name, mode and timings |
  | `INFO` | `INFO:…` | Dump name, mode and timings |
  | `RESET` | `RESET:OK` | Restore default timings + Desktop mode |
  | `RESTART` | `REBOOTING` | Reboot the ESP32 |

  The firmware also runs a task watchdog: if the main loop ever hangs, the pad
  reboots itself automatically.

  The app sends `TIMING:` automatically whenever it connects or saves, over
  **whichever link is active** (USB serial or Bluetooth), so the pad always
  matches the values shown in **Settings → Timing Parameters**.

To remove the application shortcut and icon from the Ubuntu system:
```bash
cd linux-app
./uninstall.sh
```

---

## 🐧 Configuration on Linux Ubuntu

### 1. USB Serial Access Permission
So that a browser on Ubuntu has permission to read the microcontroller serial port:
```bash
# Add your user to the dialout group
sudo usermod -a -G dialout $USER

# Grant permission to the ttyUSB or ttyACM port
sudo chmod a+rw /dev/ttyUSB0    # or /dev/ttyACM0
```
*(After running the commands above, log out and log back in for the group to take effect).*

### 2. Bluetooth BLE Pairing on Ubuntu
1. Turn on Bluetooth on your Ubuntu PC.
2. Power on the ESP32 OctoPad.
3. Open **Settings -> Bluetooth** on Ubuntu.
4. Find the device named **`OctoPad`**, then click **Connect**.

---

## 🛠️ Flashing Firmware to the ESP32

1. Open the Arduino IDE.
2. Add the ESP32 Board URL if not already present:
   `https://raw.githubusercontent.com/espressif/arduino-esp32/gh-pages/package_esp32_index.json`
3. Select the board: **ESP32C3 Dev Module** (or whichever ESP32-C3 board you use).
4. Install the required library:
   - `NimBLE-Arduino` (by h2zero) — `Preferences` is built into the ESP32 core.
5. Open the file [`octopad.ino`](octopad.ino).
6. Plug the USB-C cable into the ESP32 and click **Upload**.

### ⚡ Building from the command line (arduino-cli)

Prefer the terminal (or CI-like reproducibility)? The helper script installs the
exact core/library versions and compiles the sketch:

```bash
# Compile only — produces build/octopad.ino.bin
scripts/build_firmware.sh

# Compile and flash to an auto-detected board
scripts/build_firmware.sh --upload

# Flash to a specific port
scripts/build_firmware.sh --upload --port /dev/ttyACM0
```

Override versions if needed with `FQBN`, `CORE_VER` and `NIMBLE_VER` environment
variables. Before opening a pull request, run the full local check (Python lint +
tests + firmware compile):

```bash
scripts/check.sh
```

---

## 👤 Author & License

Created by **SamVivan** and the OctoPad contributors.

This project is open-source under the [MIT License](LICENSE). Feel free to fork,
modify, and adapt it to your homelab setup — contributions are welcome (see
[CONTRIBUTING.md](CONTRIBUTING.md)).