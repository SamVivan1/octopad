# Tauri + Rust Rewrite — M0 (Scaffold, Spikes & Go/No-Go) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the initial `app-tauri/` skeleton for the new app, run the required spikes to collect measurements, and produce a concrete go/no-go report that will drive M1–M5. The Python apps must remain untouched.

**Architecture:** Initialize a Tauri 2 + Svelte + TypeScript project in `app-tauri/` (separates Rust core from web UI). The skeleton includes a minimal `octopad-core` crate inside the workspace (mirroring the `state.py`/domain split), buildable on both target OSes. Spikes validate: serial port enumeration, BLE discovery using the same UUIDs as the existing Python apps, configuration paths, and fresh-boot memory (measured against the legacy baselines).

**Tech Stack:** Rust (workspace + `octopad-core` + Tauri backend), Tauri 2.x, Svelte + TypeScript (Vite), Node.js toolchain for frontend, `serialport` (feature-gated), `btleplug` (BLE), `tauri-plugin-shell/tray` as needed, `serde/serde_json`.

**Spec:** docs/superpowers/specs/2026-10-18-tauri-rewrite-design.md (plan implements M0 only).

## Global Constraints

- Only exception to file scope: add `.github/workflows/tauri-m0.yml` in Task 5 to produce the first Windows CI artifact. Do not modify other files outside `app-tauri/` and `docs/superpowers/plans/` unless explicitly necessary to support this workflow.
- Never change the Arduino firmware.
- Target OS floors: Windows 10+ x64, Linux with webkit2gtk-4.1 (Ubuntu 22.04+, recent Fedora).
- Memory rule: Linux fresh boot ≤ ~100 MB; for Windows, do not exceed the legacy Python Windows baseline (measured in this task). Measurement method: total RSS of all app process(es) (sum children), idle immediately after fresh boot. Prioritize release binary (`pnpm tauri build`) measurement; record dev build numbers if release not available, with uncertainty noted.
- Project structure: `app-tauri/` lives in this repo, next to `linux-app/` and `windows-app/`.
- Use the existing device protocol: `SERVICE_UUID=6f63746f-7061-4430-8001-000000000001`, `TX_UUID=6f63746f-7061-4430-8001-000000000002`, `RX_UUID=6f63746f-7061-4430-8001-000000000003`, with legacy Nordic-UART UUIDs accepted for discovery.
- Be explicit about pass/fail for M0; if the memory go is a no-go, the report must state the alternatives and a concrete recommendation.

## Review Focus

- **Input 1: Device present but app Python still running.** If the OctoPad is already connected via the Python BLE bridge, will the spike correctly report "device busy/in use" without freezing or corrupting state? The report must document observed behavior on Linux (owner device available).
- **Input 2: USB serial not present.** Enumerating serial ports with none attached must return an empty list and no panic.
- **Input 3: Config directory missing.** Legacy config paths must be computed from platform defaults (`~/.config/octopad` on Linux, `%APPDATA%/OctoPad` on Windows) and a missing directory must not crash.
- **Input 4: Fresh-boot timing on resource-constrained environment.** The measurement must be reproducible (same command, captured RSS, idle state); define the exact one-line command(s) used and the units.
- **Input 5: BLE device with name mismatch.** Discovery must accept the pad by service UUID even if the advertised name differs from `DEFAULT_DEVICE_NAME=OctoPad`.

---

### Task 1: Scaffold Tauri 2 + Svelte + TypeScript in `app-tauri/`

**Files:**
- Create: `app-tauri/package.json`
- Create: `app-tauri/tsconfig.json`, `app-tauri/tsconfig.node.json`, `app-tauri/svelte.config.js`, `app-tauri/vite.config.ts`
- Create: `app-tauri/src/` (frontend: `src/App.svelte`, `src/app.css`, `src/main.ts`, `src/lib/`)
- Create: `app-tauri/src-tauri/tauri.conf.json`, `app-tauri/src-tauri/Cargo.toml`, `app-tauri/src-tauri/src/main.rs`, `app-tauri/src-tauri/src/lib.rs`, `app-tauri/src-tauri/build.rs`, `app-tauri/src-tauri/capabilities/default.json`, `app-tauri/src-tauri/icons/README.md` (stub)
- Create: `app-tauri/src-tauri/.gitignore`, `app-tauri/.gitignore`
- Create: `app-tauri/README.md`

**Interfaces:**
- Consumes: none
- Produces: a buildable Tauri workspace where `cd app-tauri && pnpm install && cargo test --manifest-path src-tauri/Cargo.toml --no-run 2>&1 | tail -5` exits 0, and `pnpm tauri build -- --no-bundle` (check-only) or `cargo check` succeeds.

- [ ] **Step 1: Write skeleton Cargo.toml (workspace + core + app).** Create `src-tauri/Cargo.toml` as a workspace. Define workspace members: `.` and `crates/core`. App depends on `octopad-core = { path = "crates/core" }`, `tauri = { version = "2", features = [] }`, `tauri-plugin-shell = "2"`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `tokio = { version = "1", features = ["rt-multi-thread", "macros"] }`.

- [ ] **Step 2: Create `src-tauri/crates/core/Cargo.toml`.** Add `octopad-core` crate: `edition = "2021"`, `serde` derive, `serde_json`, `tracing`, and optional deps `serialport = { version = "4", optional = true }`, `btleplug = { version = "0.11", optional = true }`. Export a minimal API: `pub fn version() -> &'static str`.

- [ ] **Step 3: Create `src-tauri/crates/core/src/lib.rs`.** Implement `pub mod platform; pub mod proto; pub mod config_paths;` with stubs returning correct types, plus `version()`. `config_paths` must provide `pub fn legacy_config_dir() -> std::path::PathBuf` returning `~/.config/octopad` on Unix and `%APPDATA%/OctoPad` on Windows (platform-specific).

- [ ] **Step 4: Scaffold Tauri backend (`src-tauri/src/lib.rs`, `main.rs`, `build.rs`).** `lib.rs` exposes `#[tauri::command] fn get_version() -> String`, registers plugins. `build.rs` runs `tauri-build`. `main.rs` is minimal. `tauri.conf.json` sets product name `OctoPad`, identifier `io.github.deusdata.octopad` (or matching existing repo style — just consistent), and enables shell/tray if needed later; keep minimal.

- [ ] **Step 5: Scaffold frontend Svelte app.** Create `package.json` with `@tauri-apps/api`, `@tauri-apps/cli`, `svelte`, `vite`, `typescript`. `vite.config.ts` uses `@sveltejs/vite-plugin-svelte`. `App.svelte` shows "OctoPad Tauri M0 scaffold". `main.ts` mounts it. `tsconfig.json` minimal.

- [ ] **Step 6: Verify buildability.** Run `pnpm install` (or `npm install`) in `app-tauri/`; run `cargo check --manifest-path src-tauri/Cargo.toml`. Expected: both succeed without errors. Run `cargo test --manifest-path src-tauri/Cargo.toml` (should pass with stub tests).

- [ ] **Step 7: Commit.** `git add app-tauri` and commit: `feat(tauri-m0): scaffold Tauri 2 + Svelte + TS workspace with octopad-core stub`.

---

### Task 2: Implement protocol constants, config-path helpers, and serial enumeration stub

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/proto.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/config_paths.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/lib.rs` (re-export)
- Create: `app-tauri/src-tauri/crates/core/src/platform/mod.rs` (conditional)
- Test: `app-tauri/src-tauri/crates/core/tests/proto_paths.rs` (new)

**Interfaces:**
- Consumes: none
- Produces: `proto::{SERVICE_UUID, TX_UUID, RX_UUID, LEGACY_SERVICE_UUID, LEGACY_TX_UUID, LEGACY_TX_UUID legacy name check}`, and `config_paths::legacy_config_dir()` returning correct platform path (no panic if missing). Serial helper `list_ports() -> Vec<String>` behind `serial` feature flag.

- [ ] **Step 1: Write failing tests.** Create `crates/core/tests/proto_paths.rs` asserting the exact UUID strings from the Python apps; assert `legacy_config_dir()` returns a non-empty `PathBuf`; on Linux-like test env it ends with `.config/octopad` (or equivalent); on Windows, ends with `OctoPad`.

- [ ] **Step 2: Run tests to verify they fail.** `cargo test --manifest-path src-tauri/Cargo.toml --test proto_paths` → expected FAIL.

- [ ] **Step 3: Implement `proto.rs`.** Define the six UUID constants exactly as in Python: `SERVICE_UUID="6f63746f-7061-4430-8001-000000000001"`, `TX_UUID="6f63746f-7061-4430-8001-000000000002"`, `RX_UUID="6f63746f-7061-4430-8001-000000000003"`, `LEGACY_SERVICE_UUID="6e400001-b5a3-f393-e0a9-e50e24dcca9e"`, `LEGACY_TX_UUID="6e400003-b5a3-f393-e0a9-e50e24dcca9e"`, `LEGACY_RX_UUID="6e400002-b5a3-f393-e0a9-e50e24dcca9e"`. Add helper `fn is_our_service(uuid: &str) -> bool`.

- [ ] **Step 4: Implement `config_paths.rs`.** Use `dirs` crate or `home` to expand `~`; on Windows use `%APPDATA%` (`std::env::var("APPDATA")` or `dirs::config_dir()` equivalent). Return `PathBuf`. No filesystem creation. Add to Cargo.toml `dirs = "6"` (or platform-specific). Implement platform module if needed.

- [ ] **Step 5: Add serial enumeration (feature-gated).** Behind `#[cfg(feature="serial")]`, implement `pub fn list_ports() -> Result<Vec<String>, String>` using `serialport::available_ports()` and mapping to port names/paths. Empty list if none. Add feature to core Cargo.toml.

- [ ] **Step 6: Run tests and verify they pass.** `cargo test --manifest-path src-tauri/Cargo.toml --test proto_paths`. Expected PASS.

- [ ] **Step 7: Commit.** `git add app-tauri/src-tauri/crates/core/src/proto.rs app-tauri/src-tauri/crates/core/src/config_paths.rs app-tauri/src-tauri/crates/core/tests/proto_paths.rs app-tauri/src-tauri/crates/core/Cargo.toml app-tauri/src-tauri/Cargo.toml` (if modified); commit `feat(tauri-m0): add protocol UUIDs and legacy config path helpers`.

---

### Task 3: BLE discovery spike (btleplug) with UUID filtering

**Files:**
- Modify: `app-tauri/src-tauri/crates/core/Cargo.toml` (enable `btleplug` feature)
- Create: `app-tauri/src-tauri/crates/core/src/ble_spike.rs`
- Create: `app-tauri/src-tauri/crates/core/tests/ble_spike.rs` (documented, may be ignored in CI if no adapter; tests assert logic only)
- Modify: `app-tauri/src-tauri/crates/core/src/lib.rs` (re-export module behind feature)

**Interfaces:**
- Consumes: `proto::{SERVICE_UUID, LEGACY_SERVICE_UUID, ...}`
- Produces: `ble_spike::discover_octopad(timeout_ms: u64) -> Result<Vec<Discovered>, BleError>` that matches by advertised service UUID first (name fallback only if needed). `Discovered` contains `name`, `address` (string form), `matched_by_uuid: bool`.

- [ ] **Step 1: Write failing logic test.** Add unit test that checks UUID match logic (no hardware) — e.g. given a list of UUIDs containing our service, returns matched_by_uuid=true. Also test name fallback only when no matching UUID (matches existing Python behavior).

- [ ] **Step 2: Run logic test to verify fail.** `cargo test --manifest-path src-tauri/Cargo.toml --test ble_spike` → FAIL.

- [ ] **Step 3: Implement BLE spike using `btleplug`.** Add `btleplug = { version = "0.11", optional = true }` to core. Implement discovery: get adapters, start scan with timeout, inspect advertisement data for service UUIDs (lowercased), accept our service or legacy service. Do not attempt connect in spike — discovery only.

- [ ] **Step 4: Add Tauri command for spike (optional but useful).** Add `#[tauri::command] async fn spike_ble_discover(timeout_ms: u64) -> Result<Vec<DiscoveredDto>, String>` in backend that calls core ble_spike. This allows owner to run spike from UI if desired.

- [ ] **Step 5: Add minimal BLE connect (gated by hardware).** Implement minimal connect attempt behind `btleplug` feature: find device by service UUID (or legacy), connect, read TX characteristic once (to receive HELLO if present), then disconnect cleanly. Do not implement full event stream. On "device busy" (adapter/GATT in use) return a structured error with that reason. Skip gracefully if no adapter/devices.

- [ ] **Step 6: Verify build and logic tests.** `cargo test --manifest-path src-tauri/Cargo.toml --lib --tests` (hardware test may be skipped; logic tests must pass).

- [ ] **Step 7: Commit.** `git add app-tauri/src-tauri/crates/core/src/ble_spike.rs app-tauri/src-tauri/crates/core/tests/ble_spike.rs app-tauri/src-tauri/crates/core/Cargo.toml app-tauri/src-tauri/src/lib.rs` (if modified); commit `feat(tauri-m0): add btleplug discovery+minimal connect spike matching Python UUID logic`.

---

### Task 4: Serial spike + config path verification

**Files:**
- Modify: `app-tauri/src-tauri/crates/core/Cargo.toml` (enable `serialport` feature)
- Create: `app-tauri/src-tauri/crates/core/src/serial_spike.rs`
- Create: `app-tauri/src-tauri/src/commands/spikes.rs` (or add to lib.rs)
- Create: `app-tauri/src-tauri/crates/core/tests/serial_config.rs`

**Interfaces:**
- Consumes: `config_paths::legacy_config_dir()`
- Produces: `serial_spike::list_ports() -> Vec<String>` (feature-gated), and `serial_spike::probe_hello(port_path: &str, timeout_ms: u64) -> Result<Option<Hello>, String>` returning minimal HELLO read (gated by hardware).

- [ ] **Step 1: Write failing tests.** Test `legacy_config_dir()` returns expected suffix per platform; serial list returns non-panicking result (empty Vec when nothing attached). Add unit test for HELLO parsing helper.

- [ ] **Step 2: Run tests to fail.** `cargo test --manifest-path app-tauri/src-tauri/Cargo.toml --test serial_config` → FAIL (until implemented).

- [ ] **Step 3: Implement serial spike.** `serialport = { version = "4", optional = true }`. Return port names. Add command `#[tauri::command] fn spike_list_ports() -> Result<Vec<String>, String>`.

- [ ] **Step 4: Add minimal serial I/O probe (gated by hardware).** Implement `probe_hello` that opens port at 115200, reads until timeout looking for `HELLO:OCTOPAD:...` (reuse `parse_hello` logic or mirror), closes cleanly. Return structured busy/permission errors; skip if port not found.

- [ ] **Step 5: Expose config path via command.** `#[tauri::command] fn spike_legacy_config_dir() -> String` returning `legacy_config_dir().to_string_lossy().to_string()`.

- [ ] **Step 6: Verify.** `cargo test --manifest-path app-tauri/src-tauri/Cargo.toml --test serial_config` passes.

- [ ] **Step 7: Commit.** `git add app-tauri/src-tauri/crates/core/src/serial_spike.rs app-tauri/src-tauri/src/commands app-tauri/src-tauri/crates/core/tests/serial_config.rs` (as needed); commit `feat(tauri-m0): add serial enumeration + minimal hello probe and config path command`.

---

### Task 5: Memory measurement harness + baselines capture + M0 report

**Files:**
- Create: `app-tauri/scripts/mem_baseline.sh` (Linux)
- Create: `app-tauri/scripts/mem_baseline.ps1` (Windows)
- Create: `app-tauri/scripts/mem_measure.sh` (Linux, measures Tauri dev build)
- Create: `app-tauri/scripts/mem_measure.ps1` (Windows)
- Create: `app-tauri/m0-report.md` (draft)
- Create: `app-tauri/m0-checklist.json` (machine-readable)
- Modify: `app-tauri/README.md` (document M0 procedures)

**Interfaces:**
- Consumes: outputs from Tasks 1–4 (buildable app)
- Produces: a concrete M0 report with baselines, spike results (serial/ble/config), memory numbers, and explicit GO/NO-GO with decision tree.

- [ ] **Step 1: Write the report template.** Create `m0-report.md` with sections: Scope, Environment (OS/kernel, webkit2gtk/WebView2), Baselines (Linux PySide6 app, Windows PySide6 app), Tauri scaffold measurements (dev build + release if possible), Spike results, UUID match verification, Memory verdict (Linux ≤100MB? Windows vs baseline?), UUIDs verified against Python, and Go/No-Go + Alternatives. Fill in "TBD" only where measurement must be taken.

- [ ] **Step 2: Write measurement scripts.** Linux: use `ps`, `pmap` or `/proc/<pid>/status` RSS/VSZ; wait for idle (e.g. 3–5 seconds after window shown); sum child processes if any. Windows: use PowerShell `Get-Process | Measure-Object -Property WorkingSet64 -Sum` or `tasklist` + WMI; document method. Both scripts must write a one-line summary.

- [ ] **Step 3: Capture legacy baselines (owner).** Run baseline scripts against the running Python apps on Linux and Windows. Record exact commands, RSS in MB (integer or one decimal), timestamp, and method (sum of app processes). If Windows baseline cannot be captured at this moment, mark as **BLOCKER** in the report.

- [ ] **Step 4: Build and measure Tauri (release first).** Prioritize `cd app-tauri && pnpm tauri build`. Measure total RSS of all app process(es) including children, idle immediately after fresh boot. If release unavailable, measure `pnpm tauri dev` (backgrounded, idle) and mark as dev-mode with uncertainty.

- [ ] **Step 5: Run spikes (owner device).** Use Tauri commands/UI to run BLE discovery+minimal connect and serial enumeration+hello probe (Python app closed if needed). Explicitly test "device busy": with Python BLE bridge running, attempt BLE ops and record structured error/behavior. Log results, verify UUIDs match exactly.

- [ ] **Step 6: Add minimal Windows CI workflow.** Create `.github/workflows/tauri-m0.yml` (only exception to scope): builds `app-tauri` on `windows-latest`, runs tests/check, and uploads NSIS/Windows bundle artifact. Keep minimal; do not touch other workflow files.

- [ ] **Step 7: Fill report and decide.** Populate `m0-report.md` completely with numbers. State: **GO** or **NO-GO** for proceeding to M1–M5 as specified. If NO-GO (Windows > baseline or Linux > 100MB), list at least two concrete alternatives (e.g. defer heavy UI panels, use smaller WebView hints, or accept Windows baseline exception with explicit rationale). If GO, state conditions.

- [ ] **Step 8: Commit.** `git add app-tauri/scripts app-tauri/m0-report.md app-tauri/m0-checklist.json app-tauri/README.md .github/workflows/tauri-m0.yml`; commit `docs(tauri-m0): add M0 report, measurement scripts, Windows CI artifact, and go/no-go`.

---

### Final M0 verification

- [ ] **All tasks committed.** History shows 5 commits as above (with M0 CI workflow added).
- [ ] **Repo state clean.** `git diff --name-only --cached` and working tree clean except planned artifacts.
- [ ] **Build check.** `cargo check --manifest-path app-tauri/src-tauri/Cargo.toml` succeeds.
- [ ] **Tests pass.** `cargo test --manifest-path app-tauri/src-tauri/Cargo.toml` passes (hardware-dependent tests may be skipped but logic tests pass).
- [ ] **Report is actionable.** `m0-report.md` contains measured numbers (Windows baseline must not be BLOCKER without owner input recorded) and an unambiguous GO/NO-GO. `m0-checklist.json` captures timestamps, OS, mode (release/dev), RSS sum method, and observed device-busy results.

**Handoff:** After committing, present `docs/superpowers/plans/2026-10-18-tauri-rewrite-m0.md` and `app-tauri/m0-report.md` (draft) to the owner for review. If GO, the next step is to write M1 plan using the same pattern; if NO-GO, discuss alternatives before proceeding.
