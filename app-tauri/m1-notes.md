# M1 Notes — Core Domain (owner checklist)

Companion to `m0-report.md`. M1 is the headless core of the new app
(`app-tauri/src-tauri/crates/core`): config schema + defaults + presets,
legacy importer, state machine with dirty tracking + event bus, and the
serial/BLE device transports. No UI yet — nothing here changes what runs on
your desktop until M2.

Everything below marked **[automated]** is already verified in CI or by
`cargo test`. Items marked **[owner]** need your hardware or your Windows
machine.

## Run the core tests yourself (Linux)

```bash
cd app-tauri/src-tauri
cargo test -p octopad-core --features serial,ble
```

Hardware tests are `#[ignore]`d, so the suite is safe to run with the pad
connected (it will not steal the port/BLE session from the running Python
app). To run the *hardware* smoke tests, close the Python app first:

```bash
# serial: opens the port, sends WHO, expects HELLO:OCTOPAD:...
cargo test -p octopad-core --features serial,ble --test serial_config -- --ignored --nocapture

# BLE: scans for the pad service UUID, connects, reads HELLO, disconnects
cargo test -p octopad-core --features serial,ble --test ble_spike -- --ignored --nocapture
```

**[owner] Device-busy check:** start the legacy Python app, leave it
connected over BLE, then run the BLE command above. Expected: a structured
"busy" error, no freeze, no crash, and the Python app keeps working. Record
what you saw.

**[owner] Also run the M0 items** still open in `m0-report.md`: install the
Linux build prerequisites
(`sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev`),
measure the legacy Python baselines, and fill in the release-binary memory
numbers.

## Windows

CI (`.github/workflows/tauri-m0.yml`) now runs the core tests **with
`--features serial,ble`**, so the serial/BLE code is compiled and exercised
on `windows-latest` on every push, and uploads the NSIS/MSI bundle. Download
the artifact from the latest workflow run, run it, and compare its memory
against `scripts\mem_baseline.ps1` on the Python app — that is the M0
go/no-go number.

## What M1 deliberately does not do (deferred, by plan)

- Executing actions for button events (keyboard/media/HA/scripts) — M3.
  In M1 a button event is only logged and forwarded to the UI as an event.
- Home Assistant client, app scanner, tray, audio-output switching — M3.
- Profiles (import/export) and the built-in preset gallery — M3.
- Any UI rendering — M2.
