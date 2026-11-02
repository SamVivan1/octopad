# Changelog

All notable changes to OctoPad are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.7.0] - 2026-11-02

### Added
- **Standalone HID mode.** The pad exposes a BLE HID keyboard + consumer
  control next to the OctoPad service. The app pushes a compact action table to
  the pad (Settings → Connection → **Sync**, plus automatically on every save
  and USB connection). When the app is not running, the firmware replays the
  actions itself: text, shortcuts, volume/media keys, screenshot
  (Shift+PrtSc), lock (Super+L) and terminal (Ctrl+Alt+T).
- Firmware commands `ACT:`, `ACTCLR`, `ACTSAVE` and `PING`. The table is staged
  and only committed on `ACTSAVE`, so an interrupted sync keeps the previous
  keymap.
- The firmware treats the app as present when the TX notifications are
  subscribed, the USB CDC port is open, or a `PING` arrived in the last 8 s.
  Without the app it switches to standalone mode on its own.
- **Release USB for flashing** from the tray menu or Settings → Connection.
  The app closes the serial port (Bluetooth stays connected) so Arduino IDE /
  `arduino-cli` can upload; **Reconnect USB** reopens it without restarting
  the app.

### Changed
- Actions that need the desktop app (Home Assistant, launch-app, multi-step
  sequences, text longer than 64 characters or non-ASCII) are refused with a
  5-blink LED when the app is absent.
- The action table is synced over USB; Bluetooth stays in use for the short
  commands (link, timing, rename, ping).
- NimBLE-Arduino pinned to 2.5.2 for the HID device support.

### Fixed
- Action sync no longer loses bytes on the ESP32-C3 USB CDC: the app pauses
  5 ms between lines, the firmware uses a 4 KB RX buffer and a non-blocking
  line reader, and the sync starts after the pad's `HELLO` instead of while it
  is still booting. This was why `ACT:ERR` appeared and the pad stayed empty.
- The firmware build (CI and `scripts/build_firmware.sh`) now sets
  `CDCOnBoot=cdc`, matching the native-USB SuperMini.

## [2.6.1] - 2026-10-30

### Fixed
- **No more double input.** With the cable plugged in while Bluetooth is also
  connected, every press used to run its action twice: the firmware sends each
  event over both transports and both bridges executed it. Input now goes
  through a link manager: only the active link executes, with a 300 ms
  duplicate guard as a safety net.
- The default configuration no longer embeds a passcode string in the button-1
  hold slot.

### Added
- **Input link preference** (`auto` / `wired` / `bluetooth`) in Settings:
  `auto` prefers the cable, so unplugging switches to Bluetooth without any
  extra hardware. The choice is stored in the profile and pushed to the pad.
- Firmware `LINK:AUTO|USB|BLE` command: the pad stops sending events over the
  inactive transport. Older firmware ignores it and stays safe thanks to the
  app-side guard.

### Changed
- **One version everywhere.** `VERSION` is now the single source: the apps read
  it at runtime, the firmware constant must match, and CI fails when the
  changelog or firmware drifts. Hardcoded "2.6" strings are gone.
- Configuration schema 2: the unused legacy `version` field is dropped (the
  product version lives in `VERSION`, the schema in `schemaVersion`), and the
  serial listener now reuses `config.default_config()` instead of its own copy.

## [2.6.0] - 2026-10-13

### Added
- **Configurable timing from the app.** The firmware now accepts a
  `TIMING:<debounce>,<click>,<hold>` command and stores it in NVS. The app sends
  it whenever it connects or saves, so the pad matches **Settings → Timing
  Parameters** instead of being stuck on the compile-time defaults.
- **Device command channel over Bluetooth.** The RX characteristic now has a
  write callback, so the same command protocol works over USB serial and BLE.
  New `VERSION` and `HELP` commands.
- **Layer persistence**: the active mode (Desktop / Home Assistant) is stored in
  NVS and restored on boot, instead of resetting to Desktop on every power-up.
- **Configuration schema migration** (`migrate_config` + `schemaVersion`) in both
  applications, so old profiles keep working after format changes.
- Unit tests (`tests/`, pytest) for the config/preset logic, the firmware line
  parsers and the `TIMING` sync, wired into CI.
- Developer scripts: `scripts/build_firmware.sh` (compile/flash via `arduino-cli`
  with pinned core/library versions) and `scripts/check.sh` (pyflakes +
  compileall + pytest + firmware compile), documented in the README.
- **Headless smoke test for the Windows app** (`scripts/smoke_windows.py`): runs
  the real PySide6 UI with Qt's `offscreen` platform plugin and builds the main
  window, all pages and all dialogs. A new CI job runs it on every change, so
  UI construction is validated on Linux without a display or a Windows machine.
- CI dependencies bumped and verified one-by-one (GitHub Actions:
  `checkout` v4→v7, `setup-python` v5→v7, `upload-artifact` v4→v7,
  `download-artifact` v4→v8, `arduino/setup-arduino-cli` v1→v2); `bleak` pinned
  to `>=1.0`.

### Changed
- The applications can now **send commands over Bluetooth** as well: the Linux
  (BlueZ D-Bus `WriteValue`) and Windows (bleak `write_gatt_char`) bridges write
  to the RX characteristic, so timing sync works on a fully wireless setup.
- Firmware LED feedback is now **non-blocking**: double/hold feedback no longer
  stalls button handling for up to ~480 ms.
- Timing sync is built by a single `config.timing_command()` helper (with
  `DEFAULT_TIMINGS`), so USB and BLE always send an identical `TIMING` command;
  both bridge modules now reuse the serial `parse_hello()` instead of their own
  regex, and the unused `SerialDaemonListener.rename_device()` was removed.
- **Bluetooth on Windows works with modern `bleak`** (1.x/2.x/3.x): the pad is
  identified from its advertised service UUIDs through a version-tolerant
  `_device_uuids()` helper, because bleak 1.x+ moved that data from
  `device.metadata['uuids']` to `device.details` (BlueZ `props.UUIDs` /
  WinRT `service_uuids`). The minimum is therefore `bleak>=1.0` and Python 3.10+.
- Firmware watchdog (task WDT) reboots the pad automatically if the main loop
  ever hangs; added `INFO` (dump name/mode/timings) and `RESTART` commands.
- Renaming the pad now works over **USB or Bluetooth** (Settings → Device).
- Console: quick-command buttons (Who/Info/Save/Help) and command sending that
  uses whichever transport is connected (USB or Bluetooth), not just USB.
- Settings now shows the pad's **firmware version**, learned from the identity
  (`HELLO`) line over both USB and Bluetooth.
- Version bumped to 2.6.0 (firmware, apps, `VERSION`).
- CI now compiles the firmware (`arduino-cli`, ESP32-C3) and attaches
  `octopad_*_firmware.bin` to GitHub Releases; a dedicated `Firmware` workflow
  checks firmware changes on pull requests.
- Home Assistant credentials are written with owner-only permissions (`0600`)
  where the platform supports it.

### Removed
- Dead firmware code: the `SET` command with the `cfg_desktop*` / `cfg_ha*`
  keycode arrays (unused since HID moved to the application) and the unused HID
  keycode constants. `SAVE` / `RESET` were redefined to persist/restore the
  runtime settings.

## [2.5.0] - 2026 (historical, never tagged)

- Native Windows application (PySide6/Qt6) alongside the Linux GTK4 app.
- CI/CD: build `.deb` and standalone `.exe` on every update; versioned GitHub
  Release on tags.
- First public release.
