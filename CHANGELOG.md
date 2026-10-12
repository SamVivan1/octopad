# Changelog

All notable changes to OctoPad are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.6.0]

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

### Changed
- The applications can now **send commands over Bluetooth** as well: the Linux
  (BlueZ D-Bus `WriteValue`) and Windows (bleak `write_gatt_char`) bridges write
  to the RX characteristic, so timing sync works on a fully wireless setup.
- Firmware LED feedback is now **non-blocking**: double/hold feedback no longer
  stalls button handling for up to ~480 ms.
- Firmware watchdog (task WDT) reboots the pad automatically if the main loop
  ever hangs; added `INFO` (dump name/mode/timings) and `RESTART` commands.
- Renaming the pad now works over **USB or Bluetooth** (Settings → Device).
- Version bumped to 2.6 (firmware, apps, `VERSION`).
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

## [2.5.0] - 2026

- Native Windows application (PySide6/Qt6) alongside the Linux GTK4 app.
- CI/CD: build `.deb` and standalone `.exe` on every update; versioned GitHub
  Release on tags.
- First public release.
