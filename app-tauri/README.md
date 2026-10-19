# OctoPad Tauri

Tauri 2 + Svelte + TypeScript rewrite of OctoPad.

## M0 Scaffold

This is the M0 scaffold - see `docs/superpowers/plans/2026-10-18-tauri-rewrite-m0.md`
for full details and `m0-report.md` for the go/no-go report.

## Development

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run tauri build
```

## Core API

- `octopad-core::version()` - Get core crate version
- `octopad-core::config_paths::legacy_config_dir()` - Get legacy config directory path
- `octopad-core::proto::*` - Protocol UUID constants

## M0 memory measurement procedure

Fresh boot, idle, same method on both sides: **total RSS (Linux) / total
WorkingSet64 (Windows) of the app process and all its children, one sample
after a 4-second idle wait.** Scripts print a single machine-readable line:
`octopad_mem_mb=<MB> procs=<N> pattern=<P> idle_s=4 ts=<ISO8601>`.

Legacy Python baseline (measure with the app running and its window shown):

```bash
# Linux
scripts/mem_baseline.sh "linux-app/main.py" 4
# Windows (PowerShell)
scripts\mem_baseline.ps1 -ProcessName python -IdleSeconds 4
```

Tauri app (release build preferred; start it fresh, wait for the window,
let it idle):

```bash
# Linux
scripts/mem_measure.sh OctoPad 4        # release; use "octopad-tauri" for dev
# Windows (PowerShell)
scripts\mem_measure.ps1 -ProcessName OctoPad -IdleSeconds 4
```

Verdict rules (from the M0 plan): Linux fresh boot <= ~100 MB; Windows must
not exceed the legacy Python Windows baseline. Record the one-line output in
`m0-report.md`.

## Spikes (Tauri commands exposed by the backend)

- `spike_ble_discover(timeout_ms)` - BLE discovery by service UUID (primary
  and legacy Nordic-UART), name fallback, structured busy errors
- `spike_list_ports()` - serial port enumeration (empty list when none)
- `spike_legacy_config_dir()` - legacy config dir path

Hardware smoke tests are `#[ignore]`d; run deliberately with:

```bash
cargo test -p octopad-core --features ble -- --ignored
cargo test -p octopad-core --features serial --test serial_config -- --ignored
```
