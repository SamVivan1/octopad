# OctoPad Desktop — Tauri + Rust Rewrite (Design)

Date: 2026-10-18
Status: Approved by project owner

## Intent & Why

OctoPad's desktop companion currently exists as two diverged Python/PySide6
applications:

- `linux-app/` — ~7,000 LOC, the mature one (custom CSS in `ui/css.py`,
  `Adw.Application` window, full `system_actions.py` with audio-output
  switching, StatusNotifier tray)
- `windows-app/` — ~4,900 LOC, several steps behind (no custom stylesheet,
  plain widgets, fewer system actions, no audio switching, plain
  `QSystemTrayIcon`), but unique `profiles_page.py` not present on Linux

Every bug must be fixed twice and every feature built twice; Windows has
fallen badly behind in both UI quality and feature coverage, while demand
for a full-featured Windows app has grown well beyond the owner's original
personal use case.

Replace both applications with a single Tauri 2 application (Rust backend +
web frontend) in a new `app-tauri/` folder in this repository. The Arduino
firmware is untouched.

## Goals

- One codebase with feature parity on Linux and Windows, and one custom
  design identical on both OSes.
- v1 = full parity with the current apps:
  - USB serial connection and BLE connection to the device
  - Home Assistant integration (status, entity picker, service calls)
  - System actions: keyboard injection, shortcuts, text typing, media keys
  - Audio output switching on both OSes
  - Tray icon, presets, app scanner
  - The Windows-only profiles feature, ported to Linux as well
- Memory: Linux fresh boot ≤ ~100 MB (current PySide6 baseline). Windows is
  measured against its own current Python baseline (WebView2 alone commonly
  uses 80–150 MB, so an absolute 100 MB cap is not realistic there); the
  rule is "not heavier than the app it replaces".
- Distribution without Python: NSIS `.exe` for Windows; deb + AppImage for
  Linux.
- Legacy config migration: the new app auto-imports the old Python app's
  config and presets; the importer is re-runnable for users who keep using
  the old app occasionally.

## Non-goals (v1)

- Auto-update (deferred to a later release)
- New UI features beyond parity
- macOS support
- Code signing (owner has none; document the SmartScreen workaround)
- Firmware changes

## Anti-goals (what failure looks like)

- Any current feature lost in the rewrite
- Old users unable to migrate
- The project stalling half-done
- Memory exceeding the current app on either OS
- Windows shipping as a second-class UI again

## Constraints

- The owner is a Rust beginner; the implementer does the heavy lifting. Code
  favors small, clearly separated modules so the owner can read along.
- No release deadline. The owner personally tests on Windows; development
  happens on the Linux machine.
- The owner's OctoPad is paired over BLE to the Linux development machine
  for daily use. BLE tests in M0/M1 are done with the Python app temporarily
  closed, or via USB serial.
- Strategy: incremental per module; the Python apps keep working until
  parity is reached, then are retired.

## Technical decisions

Chosen by the owner through Q&A:

- Frontend: Svelte + TypeScript (smallest runtime, compiled, official
  Tauri template).
- UI design: one custom design, identical on both OSes.
- Config compatibility: new app imports legacy config; format may change as
  long as migration is automatic.
- Installer: NSIS `.exe` for Windows; deb + AppImage for Linux.
- Home Assistant: REST only in v1 (same as the current apps); WebSocket
  deferred.
- Platform floor: Windows 10+ x64; Linux distributions shipping
  webkit2gtk-4.1 (Ubuntu 22.04+, recent Fedora).
- Verification: a Windows CI artifact is produced after every milestone and
  dogfooded by the owner; no fixed release deadline.
- Incremental per-module strategy; `app-tauri/` lives in this repo.

Builder's calls (adjustable if a better option appears during implementation):

- Tauri 2.x, Rust backend, Svelte + TypeScript frontend.
- Indicative crates: `tokio` (async core), `serialport` (USB serial),
  `btleplug` (BLE; BLE is already proven to work on Windows via the current
  Python `bleak` implementation), `reqwest` (Home Assistant REST),
  `serde`/`serde_json` (config), Tauri tray icon, `enigo`
  (keyboard/mouse injection), the `windows` crate for default-audio-device
  switching (IPolicyConfig COM) and SendInput, `wpctl`/`pactl` subprocesses
  on Linux for audio switching, and freedesktop `.desktop` parsing / Start
  Menu scanning for the app scanner.
- Architecture: a Rust core crate (`octopad-core` inside `app-tauri/`)
  owns config, state, and device connections (serial + BLE with automatic
  reconnect) and emits events; the web UI is a thin view over Tauri
  commands/events, mirroring the existing `state.py` emit/subscribe model.
- Config: schema-versioned JSON in the platform config directory, plus a
  re-runnable one-time importer for the legacy Python config/presets
  (including profiles).
- Coexistence/upgrade story:
  - The Linux deb reuses the `octopad` package name so `apt upgrade`
    replaces the old app.
  - The NSIS installer detects a legacy Python install, removes its
    autostart entry and artifacts.
  - The Python apps keep working until the M5 retirement step.
  - The importer may be re-run at any time (merge behavior: user's explicit
    choice at import time wins).
- CI: GitHub Actions matrix (`windows-latest` + `ubuntu-latest`) building
  bundles and running tests; artifacts published per milestone for the
  owner to test on Windows.
- Testing: Rust unit tests for parsers/config/state, smoke integration with
  a fake serial source, thin UI. Parity acceptance is a feature checklist
  derived from both existing Python apps (320 unique functions/methods in
  `linux-app`, 269 in `windows-app`, from the codebase-memory graph) —
  see `docs/superpowers/specs/2026-10-18-parity-checklist.md`, whose §7b
  holds the resolved cross-app decisions (labels, timing ranges, validation,
  single instance, theme, import flow).
- Memory measurement method: total RSS of all processes belonging to the
  app, idle, immediately after fresh boot; same method used for the legacy
  baselines in M0.

## Milestones

- **M0 — Scaffold + spikes.** Tauri skeleton in `app-tauri/`; measure
  fresh-boot memory on Linux (target ≤ 100 MB) and the legacy Python
  baselines on both OSes; connect BLE and serial to a real OctoPad on Linux;
  first Windows CI artifact. Go/no-go on the memory budget.
- **M1 — Core domain.** Config schema + legacy importer, state machine,
  device connection manager (serial + BLE with reconnect), event bus; fully
  headless and unit-tested.
- **M2 — UI foundation.** Main window, button grid/keys page, action
  editor, settings, console page, tray; the shared CSS design system.
  Windows CI artifact for owner testing.
- **M3 — Integrations.** Home Assistant client + dialogs, app scanner,
  system actions on both OSes, audio output switching on both OSes,
  presets, profiles (both OSes). Windows CI artifact.
- **M4 — Parity audit.** Walk the checklist derived from the old apps; the
  owner dogfoods the Windows build; close every gap.
- **M5 — Packaging & release.** NSIS, deb, AppImage; CI workflows updated;
  README/install docs and migration guide; then retire the Python apps.

## What's left to the builder

Exact crate selection among tied alternatives, internal module layout of
the core crate, UI component structure, and the visual design specifics.
