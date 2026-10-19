# M2 smoke checklist — tray residency, single instance, shortcuts

Task 8 of `docs/superpowers/plans/2026-10-18-tauri-rewrite-m2.md`. Everything in the
"Automated" column was run on this machine on 2026-10-19; everything in the "Human"
column still needs eyes and hands on a real desktop session, once per OS.

## What was built

| Area | Where | Behaviour |
| --- | --- | --- |
| Tray icon + menu | `src-tauri/src/lib.rs` (`build_tray`) | Tauri 2 tray API, no extra framework. Items: **Open**, **Status: …** (disabled), **Quit**. Left click opens the window on Linux. |
| Connection status in the tray | `update_tray_status` | Driven by the same `serial-status-changed` / `ble-status-changed` events the frontend listens to, so the resident app reports its state with no window open. |
| Quit | `shutdown_and_exit` | Marks quitting, signals the existing `RuntimeControl` shutdown channel, then `app.exit(0)`. Used by the tray item **and** by `quit_app_cmd` (`Ctrl+Q`), so the two cannot diverge. |
| Close hides to tray | `handle_window_event` | `CloseRequested` on the `main` window is prevented and the window hidden. Device connections keep running. Stands down once a real quit is under way. |
| Second launch focuses the existing instance | `tauri-plugin-single-instance`, registered **first** in the builder chain | Un-minimises, shows and focuses the resident window; no second runtime, no second device connection. |
| Autostart | `tauri-plugin-autostart` (`--hidden` arg) + Settings toggle | The OS login entry launches with `--hidden`; `lib.rs` hides the window for that launch. A manual launch has no argument and shows the window. |
| `Ctrl+S` / `Ctrl+Q` | `src/lib/shortcuts.ts`, installed by `AppShell` | Save runs the *same* `saveWorkingConfig()` path as the Profiles button; quit runs `quit_app_cmd`. |

### The shortcut rule (decided and tested, see `src/lib/shortcuts.ts`)

`Ctrl+S` saves and `Ctrl+Q` quits **while the OctoPad window is focused and focus is not
inside an editing surface** — `<input>`, `<textarea>`, `<select>`, or anything inside a
`contenteditable` region. Inside a field the keystroke is left to normal text editing, so
Ctrl+S typed into the device-name field, a timing input, the console line or a script field
is never hijacked. Also ignored: chords with extra modifiers (`Ctrl+Shift+S` stays free for
the M3 save-as binding) and IME compositions (`event.isComposing`). An ignored keystroke is
a complete no-op — no binding runs and `preventDefault` is not called.

### Autostart rule

The login entry lives in the OS, so the toggle never keeps its own copy: `readAutostart()`
publishes what the platform reports (`null` = unknown), and every write is confirmed by a
re-reading. A rejected write, or one the platform silently ignored, is reported as a failure
and the control snaps back to the real platform state.

## Task 9: CI and published artifacts

The workflow is `.github/workflows/tauri.yml` ("Tauri Build & Test"). It runs
on every push to `main` that touches `app-tauri/**`, and on pull requests for
the same paths.

| Job | Runner | What it proves | Artifact |
| --- | --- | --- | --- |
| `linux` | `ubuntu-latest` | frontend tests + type-check + build, core tests with `serial,ble`, workspace `cargo check`, release build | `octopad-tauri-linux-bundles` (`.deb` + AppImage) |
| `debian` | `debian:bookworm` container | the `.deb`'s declared dependencies exist on Debian 12, the package installs cleanly, core tests pass there | — |
| `windows` | `windows-latest` | the same checks plus the NSIS + MSI bundle | `octopad-tauri-windows-bundle` |

Linux is the primary platform, so it is a first-class job rather than an
afterthought; the Debian container job exists because most installations are on
a Debian-derived distribution and `ubuntu-latest` cannot prove that the
dependency set resolves there.

Download a bundle from the run page (the green checkmark next to the job), or
with `gh run download <run-id> -n octopad-tauri-windows-bundle`.

### CI history

| Run | Commit | Outcome | Note |
| --- | --- | --- | --- |
| 39220766140 | `a4d29cb` | failed | core tests failed on Windows: `m0_serial_spike_paths_still_resolve` asserted a `/dev/ttyUSB0` sample, but `candidate_ports` filters per platform. Fixed in `3c6edac`. |
| 39221969175 | `3c6edac` | see below | Windows-only run of the platform-aware test fix |
| 39222107467 | `39dd3b6` | see below | first run with all three jobs |

## Automated (run here, 2026-10-19)

| Check | Command | Result |
| --- | --- | --- |
| Rust workspace compiles | `cargo check --workspace` (in `app-tauri/src-tauri`) | clean |
| Rust formatting | `cargo fmt --all -- --check` | clean |
| Core tests | `cargo test -p octopad-core --features serial,ble` | **201 passed, 0 failed** (one known flaky test, see below) |
| Frontend tests | `npm test` | **138 passed / 8 files** (29 new in this task: 14 shortcut, 9 autostart, 6 Settings toggle) |
| Frontend types | `npm run check` | 0 errors, 0 warnings |
| Frontend build | `npm run build` | ok |
| Release bundle | `npx tauri build --bundles deb` | `src-tauri/target/release/bundle/deb/OctoPad_0.1.0_amd64.deb` (5.81 MiB), `Depends: libayatana-appindicator3-1, libwebkit2gtk-4.1-0, libgtk-3-0` |
| `--hidden` start | launch `octopad-tauri --hidden` in a D-Bus session | process stays alive, no panic, **no window mapped** |
| Single instance | second launch while one is resident | second process exits 0 immediately, resident process count unchanged (1) |
| Tray code path | stderr during launch | libayatana-appindicator is loaded and the indicator is constructed without error |

### Two things the owner should know about this machine

1. **Pre-existing startup crash found and fixed.** Before Task 8, `octopad-tauri` panicked
   at startup on *every* platform:
   `thread 'main' panicked at crates/core/src/device/serial_transport.rs:213: there is no
   reactor running`. The setup hook calls `DeviceManager::spawn` (a `spawn_blocking`) and
   `bridge::connect_device_events` (a `tokio::spawn`) from the main thread with no runtime
   entered. Verified pre-existing by rebuilding `HEAD` (pre-Task-8) and reproducing the same
   panic. `lib.rs` now starts those two calls inside `tauri::async_runtime::block_on`.
   Without it nothing in this task — and nothing in M1 — could run at all.
2. **Known flaky core test.** `queued_serial_commands_are_discarded_when_their_connection_drops`
   fails roughly 1 run in 4, on the untouched baseline as well as with these changes. It is
   not caused by Task 8; it should be looked at separately.

### Limits of this session (why the Human column is not optional here)

* The `.deb` bundle needs the appindicator **dev** package at packaging time. This machine has
  the runtime library but not `libayatana-appindicator3-dev`, and installing it needs root,
  so the packaging step was run with a locally supplied `ayatana-appindicator3-0.1.pc`
  (`PKG_CONFIG_PATH=/tmp/opencode/pkgconfig`). Tauri detected and declared the real runtime
  dependency (`libayatana-appindicator3-1`, already installed); no app code was changed for it.
* No window manager is reachable from this shell, and **neither** the installed M1 binary
  nor the new one maps a window here (`xwininfo` finds zero OctoPad windows for both), so
  window show/hide, focus stealing, tray menu contents and the icon itself were **not**
  observed. Everything about window behaviour below is unverified by machine.

## Manual smoke checklist

Legend — **L** = run by us on Linux, **W** = run by the owner on Windows. Every row is
unchecked until someone runs it. Close the legacy Python app first: serial/BLE must not be
claimed by two programs.

### A. Tray

| # | Step | Linux (us) | Windows (owner) | Expected |
| --- | --- | --- | --- | --- |
| A1 | Launch OctoPad normally | ☐ | ☐ | Tray icon appears (an OctoPad keycap) |
| A2 | Open the tray menu | ☐ | ☐ | **Open**, a greyed-out **Status: Not connected** line, **Quit** |
| A3 | Connect the pad over serial, then over BLE | ☐ | ☐ | Status line becomes `Status: Serial` / `Status: Bluetooth` / `Status: Serial + Bluetooth` without opening the window |
| A4 | Click **Open** with the window hidden | ☐ | ☐ | Window appears and takes focus |
| A5 | Left-click the tray icon | ☐ | ☐ | Window opens/focuses (Linux); on Windows the left click shows the menu |
| A6 | Click **Quit** | ☐ | ☐ | App exits immediately, tray icon disappears, no serial/BLE port left held |

### B. Close hides to tray

| # | Step | Linux | Windows | Expected |
| --- | --- | --- | --- | --- |
| B1 | With the pad connected, close the window (X / ✕) | ☐ | ☐ | Window disappears, **process stays alive**, tray icon still there |
| B2 | While hidden, send a pad event (press a key on the pad) | ☐ | ☐ | The event is received — check the status line / reconnect the window and look at the console history |
| B3 | Re-open via **Open** | ☐ | ☐ | Window returns; the app did not restart (same PID) |
| B4 | Close the window while the config has unsaved changes | ☐ | ☐ | Still hides; changes are still pending afterwards (no implicit save) |

### C. Second launch

| # | Step | Linux | Windows | Expected |
| --- | --- | --- | --- | --- |
| C1 | With OctoPad running and visible, launch it again from the launcher | ☐ | ☐ | The existing window comes to the front; **no second tray icon**, no second connection |
| C2 | Hide the window, then launch again | ☐ | ☐ | The resident window is un-hidden and focused |
| C3 | Repeat C1 with the pad connected | ☐ | ☐ | Still one instance, one connection, one tray icon |
| C4 | Launch with an explicit argument: `octopad-tauri --hidden` | ☐ | ☐ | Starts with **no** window, tray only (this is the autostart path) |

### D. Autostart toggle

| # | Step | Linux | Windows | Expected |
| --- | --- | --- | --- | --- |
| D1 | Settings → Application → "Start OctoPad at login" on | ☐ | ☐ | Checkbox reflects the real OS state; status reads "OctoPad starts at login." |
| D2 | Log out / reboot (or run the registered autostart entry by hand) | ☐ | ☐ | OctoPad starts **with no window**, tray icon present, device connects |
| D3 | Turn the toggle off, log in again | ☐ | ☐ | OctoPad does not start automatically |
| D4 | Toggle on, then delete the OS entry by hand and reopen Settings | ☐ | ☐ | The toggle shows **off** (state is re-read, never remembered) |
| D5 | Toggle it where the OS refuses (locked-down account) | ☐ | ☐ | A red message appears under the toggle and it snaps back to the real state |

### E. Keyboard shortcuts

| # | Step | Linux | Windows | Expected |
| --- | --- | --- | --- | --- |
| E1 | Focus the window (not a text field), press `Ctrl+S` | ☐ | ☐ | Config is saved; the header pill flips to "Saved" |
| E2 | Click into the Device name field, type `Ctrl+S` | ☐ | ☐ | **Nothing is saved and nothing is typed** — the field keeps editing normally |
| E3 | Same check with the console command input and a script-text action field | ☐ | ☐ | Keystroke stays in the field |
| E4 | Press `Ctrl+Shift+S` in the window | ☐ | ☐ | No shortcut fires (reserved for M3 save-as) |
| E5 | Press `Ctrl+Q` in the window | ☐ | ☐ | App exits like the tray Quit item (runtime stops, process gone) |
| E6 | With an IME composing text, press `Ctrl+Q` | ☐ | ☐ | App does not quit mid-composition |

### F. Header device status (regression check for the tray status path)

| # | Step | Linux | Windows | Expected |
| --- | --- | --- | --- | --- |
| F1 | Start with no pad attached | ☐ | ☐ | Header: "Serial: Disconnected", "Bluetooth: Disconnected"; tray: "Status: Not connected" |
| F2 | Attach the pad | ☐ | ☐ | Header shows the port and device name; tray status line follows |
| F3 | Unplug the pad | ☐ | ☐ | Both go back to disconnected without a restart |

## Out of scope / later

* "Save as preset" stays M3 (`Ctrl+Shift+S` is deliberately unbound).
* The tray menu has no "pause scanning" item; the runtime always keeps running while the app
  is resident.