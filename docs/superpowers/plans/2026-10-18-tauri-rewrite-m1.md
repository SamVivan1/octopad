# Tauri + Rust Rewrite — M1 (Core Domain) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the full headless core domain of the new app in `octopad-core`: configuration (schema, load/save, presets, legacy importer), the runtime state machine with dirty-tracking and an event bus, and a device connection manager (serial + BLE, reconnect, transport preference) — every behavior unit-tested, matching the legacy Python semantics exactly. No UI yet.

**Architecture:** Pure-Rust library code inside `app-tauri/src-tauri/crates/core/`. The `config` module mirrors `linux-app/config.py` (single source of truth for the on-disk schema); `state` mirrors `linux-app/state.py` (working config + saved-baseline dirty diff + subscribe/emit bus, minus GTK/GLib — emissions go through a plain callback registry the Tauri layer will bridge in M2); `device` unifies the M0 serial/BLE spikes into a reconnecting manager with the Python transport preference (serial first, BLE fallback). Async code uses tokio; the state machine itself is sync and thread-safe.

**Tech Stack:** Rust, tokio, serde/serde_json, serialport (feature `serial`), btleplug (feature `ble`), tempfile (dev-dep for tests).

**Spec:** docs/superpowers/specs/2026-10-18-tauri-rewrite-design.md (M1 section); legacy behavior source of truth: `linux-app/config.py`, `linux-app/state.py`, `linux-app/serial_listener.py`, `linux-app/ble_bridge.py`, `windows-app/config.py` (profiles/preset semantics).

## Global Constraints

- Only modify files under `app-tauri/` (plus this plan file). Python apps and firmware untouched.
- Config semantics must match `linux-app/config.py` exactly: `CURRENT_SCHEMA_VERSION=1`, `PINOUT=[20,9,2,1,21,10,3,0]`, `DEFAULT_TIMINGS={debounceMs:25, clickTimeoutMs:250, holdTimeoutMs:450}`, 2 modes (`desktop`, `ha`), 8 buttons per mode, triggers `single|double|hold`, action types (`none, shortcut, text, media, audio_output, launch_app, system_action, bash_script, home_assistant, ha_webhook, delay, mode_toggle`), `TIMING:{d},{c},{h}` command format, preset slugs per `preset_slug()`.
- Dirty-tracking semantics match `state.py`: non-edit keys `activeModeIndex`/`activePreset` never mark dirty; dirty emitted only on flip; `_mark_saved` deep-copies baseline.
- Transport preference: `send_command` tries serial first, then BLE (like `state.send_device_command`).
- Reconnect: serial re-scans ports in a loop; BLE reconnect backoff 3s → capped 15s, reset on successful connect (mirrors `_RECONNECT_S`/`_RECONNECT_MAX_S`).
- Legacy import must be **re-runnable**: importing again after the user edits the legacy app's config must pick up new changes (explicit choice at import time wins — import is a full replace from the legacy file). Auto-import runs **once, at first run only**, when the new app's config file is missing AND a legacy config file exists; every later import is on demand.
- Windows profiles are import/export files (no on-disk legacy store) — profiles import/UI is deferred to M3 with the profiles feature. Built-in preset gallery (`PRESET_ITEMS`) is also M3. Neither is part of M1.
- `send_command` carries a `log: bool` flag (Python parity: `[TX] …`/`[TX-BLE] …` log lines when true; `sync_timings` sends with `log=false`).
- Shutdown mechanism: `tokio::sync::watch` channel (no extra crate). Core needs tokio features `sync`, `rt`, `time`, `macros`; feature flags become `serial = ["serialport", "tokio"]`, `ble = ["btleplug", "tokio"]`.
- New app config lives in the new app's own directory: `dirs::config_dir()/OctoPad/config.json` (NOT `~/.config/octopad`, which stays legacy-read-only). Presets under `<new dir>/presets/`.
- All commands/tests run with `cargo test -p octopad-core --features serial,ble` (workspace GUI check still blocked by missing webkit2gtk on this machine — do not attempt workspace-level checks).
- TDD: write the failing test first, watch it fail, implement, watch it pass, commit.

## Review Focus

- **Input 1: Corrupt/partial legacy config JSON.** Importer must fail with a clear error and never write a half-migrated config; the new app keeps its previous config.
- **Input 2: Legacy config with legacy dict-shaped actions (single action, not list).** `actions_list` normalization must accept both shapes everywhere (load, normalize, state mutators).
- **Input 3: Serial port disappears mid-connection (USB unplug).** Manager must emit disconnected, return to scanning, and not panic; reconnect when the port reappears.
- **Input 4: BLE drop mid-session.** Manager emits disconnected with message, backs off 3s→15s, reconnects, re-reads TX for HELLO; no busy-spin.
- **Input 5: Concurrent state mutation from device thread + UI thread.** State must be `Send + Sync` (Mutex/RwLock), emissions delivered without deadlock (subscriber invoked outside the lock).
- **Input 6: Re-import when new config has unsaved edits.** Importer replaces working config and triggers the dirty flow exactly like `set_config(source="import")` in Python.

---

### Task 1: Config data model + defaults + timing command

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/config/mod.rs`
- Create: `app-tauri/src-tauri/crates/core/src/config/model.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/config_model.rs`

**Interfaces:**
- Consumes: none
- Produces (later tasks rely on these exact names):
  - `config::CURRENT_SCHEMA_VERSION: i64 = 1`
  - `config::model::{Config, Mode, Button, Action}` — serde structs mirroring the JSON shape (`Config{version: f64, schema_version: i64, device: String, debounce_ms, click_timeout_ms, hold_timeout_ms, active_mode_index: i64, active_preset: Option<String>, modes: Vec<Mode>}` with serde rename to camelCase keys `schemaVersion`, `debounceMs`, …; `Action` is `{"type": String}` + flattened extras `HashMap<String, serde_json::Value>` to tolerate every action shape)
  - `config::default_config() -> Config`
  - `config::default_action(action_type: &str) -> Action`
  - `config::actions_list(raw: &serde_json::Value) -> Vec<Action>`
  - `config::timing_command(cfg: &Config) -> Option<String>` → `Some("TIMING:25,250,450")` for valid values, `None` when any timing value is non-numeric/out of range (timings stored as `serde_json::Value` internally or validated at the boundary — the `None` path must be unit-testable)
  - `config::normalize_config(value: &serde_json::Value) -> Result<(), String>` (port of Python `normalize_config`)
  - `config::migrate_config(cfg: &mut Config)` (port of `migrate_config`; schema 0→1 no-op fill)

- [ ] **Step 1: Write failing tests** in `tests/config_model.rs`: `default_config()` has 2 modes (`desktop`,`ha`), 8 buttons each, button 0 desktop single=`mode_toggle`, `timing_command` → `"TIMING:25,250,450"`, `actions_list` handles dict/list/other per Python, `default_action("shortcut")` fields match Python, `normalize_config` rejects non-object/1-mode/bad action per Python cases, serde round-trip `Config` → JSON contains `"schemaVersion":1`, `"debounceMs":25`.
- [ ] **Step 2: Run to verify FAIL.** `cargo test -p octopad-core --test config_model` → compile error (module missing).
- [ ] **Step 3: Implement** `config/model.rs` + `config/mod.rs` per Interfaces; re-export in core `lib.rs` (keep existing `pub mod proto; pub mod platform; pub mod config_paths;`).
- [ ] **Step 4: Run to verify PASS.** Same command → all pass.
- [ ] **Step 5: Commit.** `feat(tauri-m1): add config data model, defaults, timing command, normalize` — files: `crates/core/src/config/`, `crates/core/tests/config_model.rs`, `crates/core/src/lib.rs`, `Cargo.lock`.

---

### Task 2: Config load/save + user presets (file IO)

**Files:**
- Modify: `app-tauri/src-tauri/crates/core/src/config/mod.rs` (add `store` submodule or functions)
- Test: `app-tauri/src-tauri/crates/core/tests/config_store.rs`

**Interfaces:**
- Consumes: Task 1 (`Config`, `default_config`, `migrate_config`).
- Produces:
  - `config::ConfigStore { root: PathBuf }` with `ConfigStore::new_app() -> ConfigStore` (platform config dir `OctoPad/`; if `dirs::config_dir()` returns `None`, fall back to `~/.config/OctoPad` — never panics) and `ConfigStore::at(root: PathBuf)` (tests)
  - `store.load_config() -> Config` — **lenient per-key merge, Python parity**: parse the file as `serde_json::Value` first; only a top-level non-object or malformed JSON counts as corrupt (→ defaults + log line). Then merge per-key with type checks mirroring Python: top-level keys copied when present and of the expected type (wrong type → keep default + warning, NOT full-defaults); `modes` merged button-by-button with `_merge_button` semantics; timings stay loosely typed so `timing_command` can return `None` for non-numeric values (port of Python test: `debounceMs: "not-a-number"` → `sync_timings` false). Typed `Config` is produced by the merge, not by one strict serde pass.
  - `store.save_config(&Config) -> Result<(), String>` (creates dirs, JSON indent=2, ensure_ascii=false equivalent = plain UTF-8)
  - `store.list_presets() -> Vec<String>`, `store.save_preset(name, &Config) -> Result<(), String>`, `store.load_preset(name) -> Option<Config>`, `store.delete_preset(name) -> bool`
  - `config::preset_slug(name: &str) -> String` (exact Python port incl. fallback `"preset"`)

- [ ] **Step 1: Failing tests** (`tests/config_store.rs` with `tempfile::tempdir`): load missing → defaults; save→load round-trip preserves edits; corrupt file → defaults, no panic; `_merge_button` behavior via load: stored button list shorter than 8 → remaining buttons from default template; legacy dict action accepted; `preset_slug("My Cool Preset!!") == "my-cool-preset"`, empty→`"preset"`; save/list/load/delete preset lifecycle on tempdir.
- [ ] **Step 2: Verify FAIL.** `cargo test -p octopad-core --test config_store`.
- [ ] **Step 3: Implement** per Interfaces (add `tempfile` dev-dep to core Cargo.toml).
- [ ] **Step 4: Verify PASS.**
- [ ] **Step 5: Commit.** `feat(tauri-m1): add config store with load/save merge and user presets`.

---

### Task 2b (fixup): Trigger slots hold action **lists** (parity correction)

Discovered during Task 2 review: Python trigger slots (`single`/`double`/`hold`) hold a **list of actions** (`config.actions_list`, `state.add_action/remove_action/move_action` operate on lists; legacy dict still accepted). Task 1's `Button` model stores one `Action` per trigger and Task 2's merge keeps only the first action of a list — both drop multi-action sequences (feature loss, anti-goal). Fix before Task 4 builds on the model.

**Files:**
- Modify: `app-tauri/src-tauri/crates/core/src/config/model.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/config/mod.rs` (merge + normalize paths)
- Modify/extend: `crates/core/tests/config_model.rs`, `crates/core/tests/config_store.rs`

**Interfaces:**
- Produces: `config::model::TriggerActions` — `#[serde(untagged)] enum { One(Action), Many(Vec<Action>) }` with helper `as_slice(&self) -> &[Action]` and `from_list(Vec<Action>) -> Self`; `Button.single/double/hold: TriggerActions`. Deserialization accepts legacy dict or list; serialization preserves the original shape (dict stays dict — exact round-trip parity with Python, which writes back whatever shape it loaded).
- `actions_list` semantics move onto `TriggerActions::as_slice` (list filters non-dicts at deserialize; dict-without-type handled by existing merge rules).

- [ ] **Step 1: Failing tests.** Round-trip: button with `single` as legacy dict → load → save → JSON still has dict; `single` as 2-action list → both actions preserved through load; store merge keeps **all** actions of a stored list (not just the first); `default_config()` round-trip unchanged (dict-shaped defaults stay dicts).
- [ ] **Step 2: Verify FAIL.** `cargo test -p octopad-core --test config_model --test config_store`.
- [ ] **Step 3: Implement** per Interfaces; fix Task 2 merge to use `as_slice`/shape-preserving logic.
- [ ] **Step 4: Verify PASS.** Full suite `cargo test -p octopad-core --features serial,ble` green.
- [ ] **Step 5: Commit.** `fix(tauri-m1): store action lists per trigger slot (parity with python actions_list)`.

---

### Task 3: Legacy config importer (re-runnable)

**Files:**
- Modify: `app-tauri/src-tauri/crates/core/src/config/mod.rs` (add `import` submodule)
- Test: `app-tauri/src-tauri/crates/core/tests/config_import.rs`

**Interfaces:**
- Consumes: Task 1 (`normalize_config`, `migrate_config`), Task 2 (`ConfigStore`), `config_paths::legacy_config_dir()`.
- Produces:
  - `config::import_legacy(store: &ConfigStore, legacy_dir: &Path) -> Result<ImportReport, ImportError>` — reads `<legacy_dir>/config.json`, validates via `normalize_config`, migrates, **replaces** the new store config (write to temp + atomic rename), also copies `<legacy_dir>/presets/*.json` into the new store's presets dir (overwrite existing files of the same slug); returns `ImportReport { imported_config: bool, presets: Vec<String>, warnings: Vec<String> }`
  - `config::ImportError` enum: `NotFound`, `Parse(String)`, `Invalid(String)`, `Io(String)` (serde-serializable)
  - Import is re-runnable by design: calling it again re-reads whatever the legacy app has now and overwrites (test proves second import picks up an edit made to the legacy file between calls).

- [ ] **Step 1: Failing tests** (`tests/config_import.rs`, tempdirs for both legacy + new): legacy config with 2 modes + custom label + legacy dict action → import OK, new store config contains the label, `schemaVersion==1`; presets copied; corrupt legacy JSON → `Err(Parse)` and new store untouched (still defaults); missing legacy file → `Err(NotFound)`; **re-run test**: edit legacy file, import again, new store reflects the second state; invalid structure (1 mode) → `Err(Invalid)`; **first-run auto-import rule**: `import_legacy(store)` (no args) when new config missing + legacy present → auto-imports and returns `ImportReport{first_run_auto: true}`; when new config already exists → does nothing (`ImportReport` with `first_run_auto: false`, `imported_config: false`); add `first_run_auto: bool` to `ImportReport`.
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Verify PASS.**
- [ ] **Step 5: Commit.** `feat(tauri-m1): add re-runnable legacy config importer with atomic replace`.

---

### Task 4: State machine + event bus (headless)

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/state.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/state_machine.rs`

**Interfaces:**
- Consumes: Task 1–2 (`Config`, `ConfigStore`, `preset_slug`, `default_action`, `actions_list`).
- Produces:
  - `state::AppState` — `Send + Sync` (internally `RwLock`/`Mutex`), holds: working `Config`, saved baseline snapshot, `mode_index`, `selected_index`, `dirty: bool`, `active_preset: Option<String>`, plus device/status fields (`serial_connected, serial_port, ble_connected, ble_message, device_name, firmware_version, ha_connected, ha_message, ha_entities_count, installed_apps_count`). **Init: `mode_index = 0` always** (Python hardcodes startup mode 0 regardless of stored `activeModeIndex`).
  - `state::AppEvent` enum (serde-rename to the Python event names): `DirtyChanged{dirty}`, `ConfigReplaced{source}`, `ButtonChanged{index}`, `TimingChanged{field,value}`, `ModeChanged{mode_index}`, `SelectionChanged{index}`, `ConfigSaved{ok, preset: Option<String>}`, `PresetsChanged{name}`, `SerialStatusChanged{connected, port}`, `BleStatusChanged{connected, message}`, `DeviceNameChanged{name}`, `DeviceInfoChanged{version}`, `SerialLog{line}`, `HaStatusChanged{connected, message}`, `HaEntitiesChanged{count}`, `AppsChanged{count}` — exact strings: `"dirty-changed"`, `"config-replaced"`, `"button-changed"`, `"timing-changed"`, `"mode-changed"`, `"selection-changed"`, `"config-saved"`, `"presets-changed"`, `"serial-status-changed"`, `"ble-status-changed"`, `"device-name-changed"`, `"device-info-changed"`, `"serial-log"`, `"ha-status-changed"`, `"ha-entities-changed"`, `"apps-changed"`.
  - **Event payload convention** (pins the Python wire shape the M2 UI consumes): payloads serialize as flat snake_case objects — `dirty-changed: {dirty}`, `config-replaced: {source}`, `button-changed: {index}`, `timing-changed: {field, value}`, `mode-changed: {mode_index}`, `selection-changed: {index}`, `config-saved: {ok, preset}`, `presets-changed: {name}`, `serial-status-changed: {connected, port}`, `ble-status-changed: {connected, message}`, `device-name-changed: {name}`, `device-info-changed: {version}`, `serial-log: {line}`, `ha-status-changed: {connected, message}`, `ha-entities-changed: {count}`, `apps-changed: {count}`. Implement `AppEvent::payload(&self) -> (&'static str, serde_json::Value)` returning (name, flat object) so the Tauri bridge emits exactly this shape (no external-tag nesting).
  - `AppState::new(store: ConfigStore) -> AppState` (loads config, sets baseline, drops stale `activePreset` if preset file missing — mirrors `state.py` init)
  - Methods (mirroring Python): `current_mode()`, `current_button()`, `mark_dirty()`, `has_changes()`, `set_config(cfg, source)`, `set_label(text)`, `set_action(trigger, index, action)`, `add_action(trigger, Option<Action>)`, `remove_action(trigger, index)`, `move_action(trigger, index, delta)`, `get_actions(trigger) -> Vec<Action>`, `set_timing(field, i64)`, `set_mode(index)`, `select_button(index)`, `save() -> bool`, `save_preset(name, overwrite) -> bool`, `apply_user_preset(name) -> bool`, `delete_user_preset(name) -> bool`, `set_serial_status(connected, port)`, `set_ble_status(connected, message)`, `set_device_name(Option<String>)`, `set_firmware_version(Option<String>)`, `set_ha_status(connected, message)`, `set_ha_entities(count)`, `set_apps(count)`.
  - **Per-method emission rules (Python parity — pin in tests):** `set_label`/`set_action`/`set_timing`/`add_action`/`remove_action`/`move_action` **always emit** their event (even same-value edits) — only *dirty* is flip-gated. `set_mode`/`select_button` early-return with **no event** when unchanged. Status setters (`set_serial_status`, `set_ble_status`, `set_ha_status`, `set_device_name`, `set_firmware_version`, `set_ha_entities`, `set_apps`) **dedupe**: no event when value unchanged.
  - **`set_timing` fields:** accepts only `debounceMs|clickTimeoutMs|holdTimeoutMs` (camelCase, mapping to the typed members); any other field is a no-op returning false.
  - **Preset semantics (Python parity):** `save_preset` snapshot drops `activePreset` before writing; `overwrite=true` with empty name falls back to `active_preset`, and if still empty → `ConfigSaved{ok:false}`. `delete_user_preset` does **not** persist the working config when dirty (returns false, mirroring `state.py:356-359`). `apply_user_preset` sets the active flag **before** `set_config` and leaves `dirty=true`.
  - Event bus: `AppState::subscribe(Fn(AppEvent) + Send + Sync) -> Subscription` / `Subscription::unsubscribe()` (or `AppState::unsubscribe(id)`); **subscriber callbacks run outside internal locks** (collect then dispatch).

- [ ] **Step 1: Failing tests** (`tests/state_machine.rs`, tempdir store): dirty only flips on real edit (set_label same value → no flip; different value → DirtyChanged(true), revert → false); `activeModeIndex`/`activePreset` changes don't mark dirty; `set_action` legacy append semantics; `remove_action` last-slot leaves one `none`; `move_action` out-of-range no-op; `save()` clears dirty and emits ConfigSaved{ok:true}; `save_preset` snapshot drops `activePreset`, overwrite+empty-name falls back to `active_preset`, still-empty → ConfigSaved{ok:false}; `apply_user_preset` sets active flag before set_config and leaves dirty=true; `delete_user_preset` refuses to persist when dirty; `set_config(cfg, "import")` replaces config, emits ConfigReplaced + DirtyChanged(true) vs old baseline; emission gating: set_label/set_timing emit even for same-value edits, set_mode/select_button emit nothing when unchanged, status setters dedupe (set_ha_status/set_ha_entities/set_apps included); events observed by subscriber have flat snake_case payloads per the payload table; subscriber panic/unsubscribe safety (dispatch loop continues); `AppState::new` starts at mode_index 0 even when stored `activeModeIndex` is 1.
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement** `state.rs` per Interfaces; re-export in `lib.rs`.
- [ ] **Step 4: Verify PASS.**
- [ ] **Step 5: Commit.** `feat(tauri-m1): add headless state machine with dirty tracking and event bus`.

---

### Task 5: Device manager — serial transport with reconnect

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/device/mod.rs`
- Create: `app-tauri/src-tauri/crates/core/src/device/serial_transport.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/device_serial.rs`

**Interfaces:**
- Consumes: M0 `serial_spike` logic (port listing, hello parsing, error classification) — move/reuse it inside `device::serial_transport` (keep `serial_spike` module as thin re-export or delete after moving; tests must keep passing).
- Produces:
  - `device::DeviceEvent` enum: `Log{line}`, `StatusChanged{transport: TransportKind, connected: bool, port: Option<String>, message: Option<String>}`, `HelloReceived{name: Option<String>, version: String}`, `ButtonEvent{mode: String, index: i64, trigger: String}` (parsed per `EVENT_PATTERN`, index is 0-based as Python), `Error{message}` — `TransportKind` is `enum { Serial, Ble }`; BLE disconnects carry a human message (e.g. `"Pad disconnected — retrying…"`, Python parity).
  - `device::serial_transport::SerialTransport` — struct with **injectable port-lister, opener, and sleeper** (a `Sleeper` trait or config struct with zero-delay test mode) so tests can drive it without hardware: `fn spawn(tx: tokio::sync::mpsc::Sender<DeviceEvent>, shutdown: watch::Receiver<bool>) -> JoinHandle` behavior: loop — list candidate ports (`/dev/ttyUSB*`,`/dev/ttyACM*` on Linux; `COM*` enumeration on Windows), probe each with WHO/HELLO (from M0), on connect: emit StatusChanged{Serial, true}, push `TIMING:{d},{c},{h}` (Python pushes timings right after connect), spawn reader (lines → parse hello/event per Python `_feed_line` order: hello first, then event), on disconnect/port-vanish: emit StatusChanged{Serial, false}, rescan sleeps: **2s when no port/open failed, 1s after mid-session read error** (Python `serial_listener.py:304,322,364`).
  - Event parsing helpers already in core (`parse_hello`, `parse_event_line` from M0 serial spike) — expose as `device::protocol::{parse_hello, parse_event_line}` (move from `serial_spike`).

- [ ] **Step 1: Failing tests** (`tests/device_serial.rs`): `parse_event_line("[DESKTOP] Button 3 -> SINGLE")` → `mode="desktop", index=2, trigger="single"`; HA variant; case-insensitive; noise lines → None; hello parse cases from Python `tests/test_parsers.py` (brand gate, default name, version). Transport-level: with injected fake opener that yields one connect→hello→button event→disconnect cycle, manager emits the expected `DeviceEvent` sequence (async test with timeout).
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement** per Interfaces (move M0 helpers into `device::protocol`, re-export old paths).
- [ ] **Step 4: Verify PASS.** (`cargo test -p octopad-core --features serial,ble`)
- [ ] **Step 5: Commit.** `feat(tauri-m1): add serial device transport with reconnect and event parsing`.

---

### Task 6: Device manager — BLE transport with reconnect/backoff

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/device/ble_transport.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/device/mod.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/device_ble.rs`

**Interfaces:**
- Consumes: `device::protocol::*` (Task 5), M0 `ble_spike` classification logic (move into `device::ble_transport` or keep and reuse).
- Produces:
  - `device::ble_transport::BleTransport::spawn(tx, shutdown: watch::Receiver<bool>) -> JoinHandle` behavior: scan for pad by service UUID (fallback name), connect, **read TX once** (HELLO), start notifications on TX, reassemble stream using the Windows-Python `_split_events` line-pattern semantics (fragmented notifications with partial lines across packets), write path `send_command(cmd)` on RX characteristic; reconnect loop with backoff `3s → ×2 capped 15s`, reset on connect; device busy → structured `DeviceEvent::Error` with busy message, keep backing off (no tight loop). On connect, also push `TIMING:` (Python parity).
  - Backoff helper `device::backoff::Backoff { initial: 3s, max: 15s }` with `next_delay()`/`reset()` (unit-testable).
  - Line reassembly helper `device::protocol::split_events(buffer: &mut String) -> Vec<String>` — exact port of Windows `_split_events` + `_LINE_PATTERN` (event, `MODE:` line, `HELLO:` line), unit-tested with fragmented inputs.

- [ ] **Step 1: Failing tests** (`tests/device_ble.rs`): `split_events` fragmented HELLO across 3 notifications; event + trailing noise; `MODE:` line captured; partial line retained in buffer; `Backoff` sequence 3,6,12,15,15 and reset→3; classification tests still pass after the move (UUID-first, name fallback).
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Verify PASS.**
- [ ] **Step 5: Commit.** `feat(tauri-m1): add BLE transport with stream reassembly and reconnect backoff`.

---

### Task 7: Unified DeviceManager (serial preferred, BLE fallback) + timing/name sync

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/device/manager.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/device/mod.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/device_manager.rs`

**Interfaces:**
- Consumes: Tasks 4–6 (`AppState` event sinks optional — manager emits `DeviceEvent`s; a later bridge wires them to `AppState`), both transports.
- Produces:
  - `device::manager::DeviceManager::spawn(tx: mpsc::Sender<DeviceEvent>, shutdown: watch::Receiver<bool>) -> JoinHandle` — spawns both transports, merges their event streams; `send_command(&self, cmd: &str, log: bool) -> bool` prefers serial (track per-transport connected state from events), falls back to BLE, returns false when neither connected; when `log=true`, emits `Log{"[TX] …"}` for serial / `Log{"[TX-BLE] …"}` for BLE (Python parity); `sync_timings(&Config) -> bool` builds `TIMING:{d},{c},{h}` via `config::timing_command` and `send_command(cmd, log=false)`.
  - `device::manager::rename_device(name: &str) -> bool` — **strip whitespace first**, then validate (non-empty, ≤24 chars after strip), send `NAME:{name}` with `log=true` (mirrors `state.rename_device`); on success the **caller** (Task 8 bridge/command) updates state via `set_device_name(Some(name))` — manager itself stays state-free.

- [ ] **Step 1: Failing tests** (`tests/device_manager.rs`): with fake transports (serial connected + BLE connected), `send_command("X", true)` hits serial only and emits a `[TX] ` log line; `send_command("X", false)` emits nothing; serial disconnected → falls back to BLE (`[TX-BLE] ` line); neither → false; `sync_timings` sends correct `TIMING:` string via `send_command(..., log=false)` (no log line); rename validation: empty→false, 25 chars→false, name-with-spaces→strips then validates, valid→true (fake transport asserts `NAME:...` sent). Manager event merge: hello from either transport surfaces once. **TIMING-on-connect:** fake serial connect cycle asserts a `TIMING:25,250,450` write happens right after StatusChanged{connected:true} (Python `serial_listener.py:317-320` parity).
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Verify PASS.** Full suite: `cargo test -p octopad-core --features serial,ble` (all prior tests still green; old `-- --ignored` hardware tests still compile).
- [ ] **Step 5: Commit.** `feat(tauri-m1): add unified device manager with serial-preferred command routing`.

---

### Task 8: AppState ↔ DeviceManager bridge + Tauri command surface (headless-verified)

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/bridge.rs`
- Modify: `app-tauri/src-tauri/src/lib.rs` (register commands), `app-tauri/src-tauri/src/commands/` (extend)
- Test: `app-tauri/src-tauri/crates/core/tests/bridge.rs`

**Interfaces:**
- Consumes: Task 4 (`AppState`), Task 7 (`DeviceManager`, `DeviceEvent`).
- Produces:
  - `bridge::connect_device_events(state: Arc<AppState>, rx: mpsc::Receiver<DeviceEvent>) -> JoinHandle` — maps every `DeviceEvent` to the right `AppState` mutator + `SerialLog` emission (`HelloReceived` → `set_device_name` + `set_firmware_version`; `ButtonEvent` → `SerialLog` line only in M1 — action execution is M3; `StatusChanged{Serial,…}` → `set_serial_status`, `StatusChanged{Ble,…}` → `set_ble_status`; `Error` → `SerialLog`).
  - Tauri commands (in app crate): `get_state_snapshot() -> serde_json::Value` (current config + statuses for the future UI), `import_legacy_config() -> Result<ImportReport, ImportError>` — **after a successful store-level import, reload via `store.load_config()` and feed through `AppState::set_config(cfg, source="import")`** so the dirty flow triggers exactly like Python (Review-Focus Input 6); `save_config_cmd() -> bool`, `list_presets_cmd() -> Vec<String>`; `rename_device_cmd(name) -> bool` (calls manager, then `set_device_name` on success); register an app-wide event forwarder stub (emit `AppEvent` as Tauri events `octopad://<event-name>` with the flat snake_case payload via `AppEvent::payload` — the full UI wiring lands in M2, here it must compile and forward at least `dirty-changed`).
  - **Thin-wrapper constraint:** app-crate command bodies must be ≤ ~10 lines delegating to core; all real logic must be reachable from `octopad-core` tests (especially the import → set_config flow, tested at the core level via a `bridge::run_import(state, store) -> Result<ImportReport, ImportError>` core helper that the command wraps).
  - Note: app-crate compile cannot be verified locally (webkit2gtk missing); verify with `cargo check -p octopad-core` + rustfmt parse of app-crate files, same mitigation as M0 Task 4. CI (windows-latest) runs the real check.

- [ ] **Step 1: Failing tests** (`tests/bridge.rs`, headless): feed a synthetic `DeviceEvent` sequence through `connect_device_events` and assert `AppState` fields/events afterwards (serial connected status, ble status via `StatusChanged{Ble,…}`, device name+version from hello, SerialLog for ButtonEvent, error → log, no crash on unknown event); `run_import` with corrupt legacy file → Err + state untouched; `run_import` success → `ConfigReplaced{source:"import"}` + `DirtyChanged(true)` + working config updated.
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement** bridge + commands per Interfaces.
- [ ] **Step 4: Verify PASS** core tests; `cargo fmt --check` on app crate sources; run `cargo test -p octopad-core --features serial,ble` full suite.
- [ ] **Step 5: Commit.** `feat(tauri-m1): add device-event bridge and initial Tauri command surface`.

---

### Final M1 verification

- [ ] `cargo test -p octopad-core --features serial,ble` — all green (hardware `--ignored` tests compile only).
- [ ] `cargo fmt --check -p octopad-core` clean.
- [ ] **CI feature-flag gap fixed:** update `.github/workflows/tauri-m0.yml` (or its M1 successor) so the test step runs `cargo test -p octopad-core --features serial,ble` — without the features, all device/serial/BLE tests silently skip in CI. This is the one allowed edit outside `app-tauri/`.
- [ ] No files touched outside `app-tauri/`, this plan, and the CI workflow fix above.
- [ ] Push/CI: Windows CI workflow runs core tests + workspace check (validates app crate that local machine cannot).
- [ ] Owner checklist appended to `app-tauri/m0-report.md` (or new `m1-notes.md`): run the ignored hardware tests with the real pad (serial hello + BLE connect + drop/reconnect observed) before M2 starts in earnest.

**Handoff:** present the M1 summary + test counts to the owner; if hardware smoke passes, proceed to write the M2 plan (UI foundation).
