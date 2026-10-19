# M0 Report — OctoPad Tauri + Rust Rewrite

Status: **tentative GO** — pending owner measurements (memory baselines +
release-binary memory) and the hardware spike. See "Go/No-Go" below.

## Scope

M0 of the Tauri + Rust rewrite (plan:
`docs/superpowers/plans/2026-10-18-tauri-rewrite-m0.md`): scaffold the
`app-tauri/` workspace (Tauri 2 + Svelte + TypeScript + `octopad-core`),
implement the protocol UUIDs / config-path helpers / serial + BLE spikes,
verify them with hardware-free logic tests, set up the memory-measurement
harness and a first Windows CI artifact. The Python apps
(`linux-app/`, `windows-app/`) and the Arduino firmware are untouched.

## Environment (measurement machine — Linux)

| Item | Value |
| --- | --- |
| OS | Ubuntu 26.10 (questing), x86_64 |
| Kernel | 7.3.0-9-generic |
| WebKitGTK | `libwebkit2gtk-4.1-0` 2.52.6-1ubuntu1 (runtime only — see caveat) |
| Rust | rustc 1.99.1 (58cf8f9c2 2026-10-13), cargo 1.99.1 |
| Node | v25.9.0 |
| Package manager | npm (`package-lock.json` committed; pnpm not installed) |

**Caveat (environment issue):** the webkit2gtk-4.1 **dev headers**
(`libwebkit2gtk-4.1-dev`, provides `webkit2gtk-4.1.pc`) are not installed on
this machine and cannot be installed (no sudo). Therefore the *full-workspace*
`cargo check`/`cargo build` of the Tauri app crate cannot complete **on this
Linux machine**; everything below that is marked as verified was verified for
the `octopad-core` crate (which has no GUI dependency). The Tauri app crate
compiles on Windows CI (WebView2 ships with `windows-latest`) and will compile
on Linux once the dev package is installed
(`sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev`).

Environment for the **Windows** baseline/measurement run: TBD (owner machine).

## Baselines (legacy Python apps — owner measurement required)

Method (identical on both OSes, per plan): total RSS (Linux) / total
`WorkingSet64` (Windows) of the app process **and all children**, one sample
after a 4 s idle wait, window shown, app freshly started. Scripts:
`scripts/mem_baseline.sh` (Linux), `scripts/mem_baseline.ps1` (Windows).

| Baseline | Command | RSS (MB) | Timestamp | Status |
| --- | --- | --- | --- | --- |
| Linux PySide6 app | `scripts/mem_baseline.sh "linux-app/main.py" 4` | **TBD (owner)** | TBD | pending owner run |
| Windows PySide6 app | `scripts\mem_baseline.ps1 -ProcessName python -IdleSeconds 4` | **TBD (owner) — BLOCKER for the final verdict** | TBD | pending owner run |

The plan makes the Windows legacy baseline a **BLOCKER** for the final
GO decision: the rule is "Windows Tauri must not exceed the legacy Python
Windows baseline", which cannot be evaluated until the baseline number
exists.

## Tauri scaffold measurements

| Build | Mode | Command | RSS (MB) | Timestamp | Status |
| --- | --- | --- | --- | --- | --- |
| Linux release | release (`npm run tauri build`) | `scripts/mem_measure.sh OctoPad 4` | **TBD (owner)** | TBD | blocked on this machine by missing webkit2gtk-4.1 dev headers; run on owner machine |
| Linux dev | dev (`npm run tauri dev`) | `scripts/mem_measure.sh octopad-tauri 4` | **TBD (owner)** | TBD | fallback if release build unavailable |
| Windows release | release (`npm run tauri build`, NSIS) | `scripts\mem_measure.ps1 -ProcessName OctoPad -IdleSeconds 4` | **TBD (owner)** | TBD | use CI artifact or local build |

Method note: the scripts print exactly one machine-readable line —
`octopad_mem_mb=<MB> procs=<N> pattern=<P> idle_s=4 ts=<ISO8601>` — with
`<MB>` = sum of RSS/WorkingSet64 over the process tree in mebibytes
(1024-based), one decimal. Record the raw line here when measured.

## Spike results (Tasks 2–4, measured 2026-10-18 on the machine above)

All logic tests pass; hardware-dependent tests are `#[ignore]`d and were not
run (no OctoPad / no BLE adapter on the CI-like machine). Test summary
(`cargo test -p octopad-core --features serial,ble`):

| Test target | Passed | Failed | Ignored (hardware) |
| --- | --- | --- | --- |
| lib (proto/config/serial/ble unit tests) | 10 | 0 | 0 |
| `ble_spike` | 6 | 0 | 1 |
| `proto_paths` | 5 | 0 | 0 |
| `serial_config` | 10 | 0 | 1 |
| **Total** | **31** | **0** | **2** |

### Serial enumeration (Review Focus Input 2 — USB serial not present)

- `list_ports()` returned a structured `Ok(...)` listing the 32 kernel
  `ttyS*` ports of this machine (no USB serial attached) — **no panic**, no
  error. Exact live output:
  `["/dev/ttyS0", "/dev/ttyS1", … "/dev/ttyS31"]`.
- Deterministic empty case: `port_names(vec![])` → empty `Vec` (test
  `empty_port_enumeration_yields_empty_list_without_panicking`).
- `probe_hello()` on a nonexistent port → `Ok(None)` (graceful skip), tested.

### BLE discovery logic (Review Focus Input 5 — name mismatch)

- Discovery matches by **service UUID first** (primary
  `6f63746f-…-0001` and legacy Nordic-UART `6e400001-…`), case-insensitively;
  a device renamed by the user ("My Custom Pad") is still discovered with
  `matched_by_uuid = true`.
- Name fallback (`OctoPad`, case-insensitive, whitespace-stripped) only when
  no matching service UUID is present — mirrors the Python bridges.
- Unknown/anonymous devices and characteristic-UUID-only advertisements are
  rejected.
- Hardware discovery/connect and the **device-busy** behavior
  (Review Focus Input 1: Python BLE bridge holding the pad) are
  **TBD (owner)** — run
  `cargo test -p octopad-core --features ble -- --ignored` and the
  `spike_ble_discover` command with the pad near the adapter, once with the
  Python bridge running (expect a structured busy/GATT-in-use error, no
  freeze) and once with it closed.

### Serial HELLO probe (hardware part TBD)

- HELLO parsing mirrors `tests/test_parsers.py` exactly (brand check,
  optional version, case-insensitive, noise-tolerant, unnamed-pad default) —
  6 parsing tests pass.
- Structured `Busy` / `PermissionDenied` error kinds serialize machine-readably
  (serde) and render human-readable messages.
- Live probe against a real pad: **TBD (owner)** — run
  `cargo test -p octopad-core --features serial --test serial_config -- --ignored`
  with the pad attached; also test **device busy** on serial by running the
  probe while the legacy Python app owns the port (expect `Busy`, no freeze).

### Config paths (Review Focus Input 3 — missing config dir)

- `legacy_config_dir()` returns `~/.config/octopad` on Unix (ignoring
  `XDG_CONFIG_HOME`, exactly like `linux-app/config.py`) and
  `%APPDATA%\OctoPad` on Windows (`APPDATA` with `~` fallback, exactly like
  `windows-app/serial_listener.py`). Tested on Linux; the Windows branch is
  asserted by the same test when it runs on the Windows CI runner.
- It only computes a path: no filesystem access, **missing directory never
  crashes**.

## UUID match verification

Asserted byte-for-byte in `crates/core/tests/proto_paths.rs` against the
Python apps (`linux-app/ble_bridge.py`, `windows-app/ble_bridge.py`) and the
firmware (`octopad.ino`); all tests green:

| Constant | Value |
| --- | --- |
| `SERVICE_UUID` | `6f63746f-7061-4430-8001-000000000001` |
| `TX_UUID` | `6f63746f-7061-4430-8001-000000000002` |
| `RX_UUID` | `6f63746f-7061-4430-8001-000000000003` |
| `LEGACY_SERVICE_UUID` | `6e400001-b5a3-f393-e0a9-e50e24dcca9e` |
| `LEGACY_TX_UUID` | `6e400003-b5a3-f393-e0a9-e50e24dcca9e` |
| `LEGACY_RX_UUID` | `6e400002-b5a3-f393-e0a9-e50e24dcca9e` |

## CI (first Windows artifact)

`.github/workflows/tauri-m0.yml` builds `app-tauri` on `windows-latest`:
`cargo test -p octopad-core` → `cargo check` (workspace) →
`npm run tauri build` (NSIS `.exe` + MSI `.msi`) → uploads the
`octopad-tauri-windows-bundle` artifact. Placeholder icons were generated
(`tauri icon` from a 1024×1024 source; dark slate "OP" placeholder) so the
bundle config resolves — **replace with real branding before release**.
Windows CI is unaffected by the Linux webkit2gtk caveat (WebView2 is
preinstalled on the runner).

## Memory verdict

| Rule | Threshold | Measured | Verdict |
| --- | --- | --- | --- |
| Linux fresh boot | ≤ ~100 MB total RSS (idle, release) | TBD (owner) | **pending** |
| Windows fresh boot | ≤ legacy Python Windows baseline | Tauri: TBD (owner); baseline: TBD (owner) | **pending — BLOCKER** |

## UUIDs verified against Python

Yes — exact-match tests green (table above); BLE classification mirrors the
Python match order (service UUID → default-name fallback) and case handling.

## Go/No-Go

**Tentative GO** to proceed to M1, conditional on the following being
completed by the owner before M1 work starts in earnest:

1. **Linux memory measurement** (release build, fresh boot, idle) ≤ ~100 MB.
2. **Windows memory measurement** of both the legacy PySide6 baseline and the
   Tauri release build; Tauri must not exceed the baseline. Until the
   baseline exists this is a **BLOCKER** for the *final* verdict.
3. **Hardware spike**: BLE discovery + minimal connect and serial HELLO probe
   with the real pad, including the **device-busy** cases (Python bridge
   holding BLE; Python app holding the serial port) — expect structured
   errors, no freeze, no state corruption.
4. (Environment) Install `libwebkit2gtk-4.1-dev` on the Linux dev machine, or
   verify the Linux build on a machine that has it. This does not block the
   Windows CI artifact.

Rationale: every hardware-free M0 gate that can be evaluated on this machine
is green — 31/31 logic tests pass, serial enumeration is panic-free with no
device, config paths match the legacy apps exactly, and UUIDs are verified
byte-for-byte. The remaining risks (memory budget, real-hardware behavior)
are exactly the ones the plan reserves for owner measurement.

### Decision tree

- If Linux release RSS ≤ ~100 MB **and** Windows Tauri RSS ≤ Windows Python
  baseline **and** hardware spike shows structured busy handling → **GO** as
  planned.
- If Windows Tauri RSS > Windows Python baseline (or the baseline cannot be
  captured at all) → **NO-GO pending rework**, options (pick at least one,
  re-measure):
  1. **Defer heavy UI panels** to M2+ and ship M1 with the minimal tray +
     single window (smallest footprint is what gets measured).
  2. **Trim the Windows surface**: disable the WebView2 devtools, disable
     background throttling off, and ship `windows-subsystem` console=false —
     each shaves working set; re-measure before deciding.
  3. **Accept a documented Windows exception** with an explicit rationale
     (e.g. ≤1.25× baseline) if the absolute numbers are small in practice —
     requires owner sign-off, recorded here.
- If Linux release RSS > ~100 MB but Windows passes → GO with a Linux
  follow-up: measure a stripped release (`strip = true`, `opt-level = "z"`,
  single-window, no tray in M1) before the Linux packaging milestone.
- If the hardware spike shows a freeze/corruption on device-busy → NO-GO for
  M1's connection layer until the busy handling is fixed and re-verified.

## Owner checklist (outstanding)

- [ ] Linux legacy baseline (`scripts/mem_baseline.sh "linux-app/main.py" 4`)
- [ ] Windows legacy baseline (`scripts\mem_baseline.ps1`)
- [ ] Linux Tauri release RSS (`scripts/mem_measure.sh OctoPad 4`)
- [ ] Windows Tauri release RSS (`scripts\mem_measure.ps1`)
- [ ] BLE discovery + connect spike on real hardware; device-busy with Python
      BLE bridge running
- [ ] Serial HELLO probe on real hardware; device-busy with Python app
      holding the port
- [ ] Install `libwebkit2gtk-4.1-dev` on the Linux dev machine (or verify
      Linux build elsewhere)
- [ ] Replace placeholder icons with real branding before any public release
