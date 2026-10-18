# Tauri Rewrite — M2 UI Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Deliver the first usable cross-platform Tauri UI for configuring buttons, viewing device logs, editing basic settings, and managing the resident app window.

**Architecture:** Svelte 5 + TypeScript is a thin view over the existing `octopad-core` state and event bus. Tauri commands remain thin and delegate to `AppState`, `DeviceManager`, and `ConfigStore`; frontend state subscribes to `octopad://<event-name>` events and starts listening before loading the initial snapshot.

**Tech Stack:** Tauri 2, Rust (`octopad-core`), Svelte 5, TypeScript, Vite, Vitest, and Tauri plugins for single-instance and autostart behavior.

**Spec:** `docs/superpowers/specs/2026-10-18-tauri-rewrite-design.md` (M2); acceptance reference: `docs/superpowers/specs/2026-10-18-parity-checklist.md`.

## Global Constraints

- The same custom UI and behavior must work on Linux and Windows; macOS is out of scope.
- Use a dark-only custom theme in v1, implemented with design tokens so another theme can be added later without replacing component styles.
- The UI is a thin view over the Rust core; `AppState` is the source of truth for config, dirty state, selection, and device status.
- M2 includes the main window, Buttons, Console, Profiles, and Settings navigation, a button/action editor, basic settings, tray, and the shared CSS design system.
- Preserve the existing config schema, legacy single-action and list-action forms, and all unknown action fields when editing.
- Keep Tauri command bodies thin (approximately 10 lines or fewer); put reusable validation and state transitions in `octopad-core`.
- Buttons have 8 physical entries and 2 modes (`desktop`, `ha`); trigger names remain `single`, `double`, and `hold`.
- Timing controls use debounce 0–1000 ms, click 50–2000 ms, hold 50–3000 ms, all with 5 ms steps; firmware defaults remain 25/250/450.
- Importing a config loads it into the editor and requires explicit Save; first-run automatic legacy import remains the M1 exception.
- Closing the window hides it to the tray; Quit exits the app. A second launch focuses the existing instance on both OSes.
- No onboarding wizard. No action execution, Home Assistant client/dialogs, app scanner, audio picker, built-in presets, user preset management, or profile file import/export in M2; those remain M3.
- App command failures must be surfaced in the UI and must not be reported as successful edits or sends.
- Run core tests with `--features serial,ble`; run frontend checks/tests/build and verify the Windows bundle workflow.

## Review Focus

- **Unknown action fields and legacy action shapes:** Editing a known field must preserve other fields and round-trip both legacy object and list trigger slots.
- **Rapid mode/button selection:** An edit must never land on a different button after selection changes; command calls must be ordered and invalid indices rejected.
- **Import with unsaved edits:** A successful legacy import replaces the working config and marks it dirty but never saves without an explicit Save action; a failed import leaves the editor unchanged.
- **Disconnected or failed command write:** Rename and console send report failure when no transport is available or the actual write fails; state must not claim the pad was renamed.
- **UI startup/event timing:** Register event listeners before requesting the initial snapshot; unmounting pages must not leave duplicate listeners.
- **Window close and second launch:** Closing hides the window without stopping device connections; a second process activates the existing window, while Quit shuts down the runtime.

---

### Task 1: Expose the M2 state-edit and command API

**Files:**
- Modify: `app-tauri/src-tauri/src/commands/device.rs`
- Modify: `app-tauri/src-tauri/src/commands/mod.rs`
- Modify: `app-tauri/src-tauri/src/lib.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/bridge.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/state.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/bridge.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/state_machine.rs`

**Interfaces:**
- Consumes: `AppState`, `DeviceManager`, and the current `bridge::state_snapshot`.
- Produces these thin Tauri commands:
  - `set_mode_cmd(index: i64) -> bool`, `select_button_cmd(index: i64) -> bool`, and `set_label_cmd(text: String)`.
  - Mode and selection commands reject indices outside the current mode/button bounds; action commands accept only `single`, `double`, or `hold`.
  - `set_action_cmd(trigger: String, index: i64, action: Action)`, `add_action_cmd(trigger: String)`, `remove_action_cmd(trigger: String, index: i64)`, and `move_action_cmd(trigger: String, index: i64, delta: i64)`.
  - `set_timing_cmd(field: String, value: i64) -> bool`.
  - `send_device_command_cmd(command: String) -> bool`, async and true only after the selected transport reports a successful write.
  - `save_config_cmd() -> bool`, async; delegate to the core save helper, persist first, then best-effort `sync_timings` after a successful save without turning a successful save into a failure if the device is disconnected.
  - Extend `state_snapshot` with `configPath`, sourced from `ConfigStore::config_path()`.
- Tauri command names use the existing snake_case names and frontend invoke arguments use Tauri's camelCase argument mapping.
- Core helper: `pub async fn bridge::save_config(state: &AppState, manager: &DeviceManager) -> bool`.
- Change `AppState::set_mode(index: i64)` and `select_button(index: i64)` to return `bool`, rejecting out-of-range mode/button indices without mutating config or emitting events; valid unchanged indices remain successful no-ops.

- [ ] **Step 1: Write failing core tests** for invalid mode/button indices (no state or event change) and `bridge::save_config`: it saves and clears dirty state on success, sends timings only after a successful save, and still returns save success if no transport is connected. Test that a failed storage write does not send timings.
- [ ] **Step 2: Run the focused tests** with `cargo test -p octopad-core --features serial,ble --test bridge`; confirm the new helper/tests fail before implementation.
- [ ] **Step 3: Implement the core helper and thin Tauri commands.** Keep mutation logic in `AppState`; do not duplicate dirty tracking in the command layer.
- [ ] **Step 4: Run bridge tests and `cargo fmt --all -- --check`.** Expected: all focused tests pass.
- [ ] **Step 5: Commit** as `feat(tauri-m2): expose UI state and device commands`.

### Task 2: Build a typed frontend API and event-backed state store

**Files:**
- Create: `app-tauri/src/lib/types.ts`
- Create: `app-tauri/src/lib/api.ts`
- Create: `app-tauri/src/lib/state.ts`
- Create: `app-tauri/src/lib/state.test.ts`
- Modify: `app-tauri/package.json`
- Modify: `app-tauri/package-lock.json`
- Modify: `app-tauri/tsconfig.json`

**Interfaces:**
- Consumes: `get_state_snapshot`, the Task 1 commands, and the existing flat `AppEvent` payloads.
- Produces:
  - TypeScript `Config`, `Mode`, `Button`, `Action`, `AppSnapshot`, and event payload types matching the Rust wire shape.
  - A typed API module that wraps `invoke` and `listen`; event names are exactly `octopad://dirty-changed`, `octopad://button-changed`, `octopad://timing-changed`, `octopad://mode-changed`, `octopad://selection-changed`, `octopad://serial-status-changed`, `octopad://ble-status-changed`, `octopad://device-name-changed`, `octopad://device-info-changed`, and `octopad://serial-log`.
  - `initializeAppState()` which registers event listeners before fetching and applying `get_state_snapshot`, and returns a cleanup function that unregisters every listener.
  - A single Svelte store for the snapshot and a bounded in-memory log list; unknown event payload fields are ignored rather than corrupting the config.

- [ ] **Step 1: Add Vitest and write failing store tests** for listener-before-snapshot ordering, state updates for mode/selection/dirty/status events, command rejection propagation, and cleanup calling each `UnlistenFn`.
- [ ] **Step 2: Run `npm test -- src/lib/state.test.ts`**; confirm the tests fail because the API/store is absent.
- [ ] **Step 3: Implement the types, invoke wrappers, and store.** Use `svelte/store`; do not mirror mutable config in separate page-local stores.
- [ ] **Step 4: Run `npm test -- src/lib/state.test.ts` and `npm run check`.** Expected: tests and type-check pass.
- [ ] **Step 5: Commit** as `feat(tauri-m2): add typed frontend state bridge`.

### Task 3: Create the app shell, page navigation, and shared design system

**Files:**
- Modify: `app-tauri/src/App.svelte`
- Modify: `app-tauri/src/app.css`
- Create: `app-tauri/src/lib/components/AppShell.svelte`
- Create: `app-tauri/src/lib/components/StatusHeader.svelte`
- Create: `app-tauri/src/lib/components/Sidebar.svelte`
- Create: `app-tauri/src/lib/pages/ProfilesPage.svelte`
- Test: `app-tauri/src/lib/components/AppShell.test.ts`

**Interfaces:**
- Consumes: Task 2 snapshot/store and the existing shared event names.
- Produces: Four top-level pages in order: Buttons, Serial Console, Profiles, Settings. The Profiles page in M2 shows current config path, dirty/saved state, Save, and legacy import; preset gallery and profile file import/export remain M3.
- Builder visual call: retain the Linux UI's dark, compact chassis/keycap language and trigger colors, translate it into CSS variables and responsive web layout; no platform-specific styling branches.
- Header displays serial/BLE connection, device name, and firmware version, and provides Save feedback. Sidebar navigation remains usable at narrow window widths.

- [ ] **Step 1: Write failing component tests** for the page order, active-page navigation, dirty/connected status labels, and clearing UI listeners on unmount.
- [ ] **Step 2: Run the component test** and confirm the current M0 scaffold does not satisfy the shell contract.
- [ ] **Step 3: Implement the shell and CSS token set** (surface, border, text, accent, success/warning/error, spacing, radius, focus ring) with responsive grid breakpoints.
- [ ] **Step 4: Run component tests, `npm run check`, and `npm run build`.** Expected: four-page shell builds without Rust dependencies.
- [ ] **Step 5: Commit** as `feat(tauri-m2): add app shell and shared dark design system`.

### Task 4: Implement the Buttons page and 8-key grid

**Files:**
- Create: `app-tauri/src/lib/pages/ButtonsPage.svelte`
- Create: `app-tauri/src/lib/components/KeyGrid.svelte`
- Create: `app-tauri/src/lib/components/KeyCard.svelte`
- Create: `app-tauri/src/lib/components/ModeSwitch.svelte`
- Test: `app-tauri/src/lib/pages/ButtonsPage.test.ts`

**Interfaces:**
- Consumes: snapshot config, mode/selection commands, and `button-changed`, `mode-changed`, and `selection-changed` events.
- Produces: an 8-card 4×2 grid showing physical key number, label, active selection, and single/double/hold action indicators; mode switch selects Desktop or Home Assistant and selection highlights the matching card.
- Selection and mode commands are awaited before subsequent edit commands so a fast click cannot route an edit to stale selection.

- [ ] **Step 1: Write failing tests** asserting 8 cards in each mode, the correct active key and labels, mode changes invoking `set_mode_cmd`, and key clicks invoking `select_button_cmd` before editor changes.
- [ ] **Step 2: Run the page tests** and confirm they fail against the shell placeholder.
- [ ] **Step 3: Implement the grid and event-driven selection state.** Keep the inspector scrollable and grid cards responsive without dropping below a usable 2-column layout.
- [ ] **Step 4: Run page tests, `npm run check`, and `npm run build`.**
- [ ] **Step 5: Commit** as `feat(tauri-m2): add button grid and mode selection`.

### Task 5: Implement action-list editing for all current action types

**Files:**
- Create: `app-tauri/src/lib/components/ActionInspector.svelte`
- Create: `app-tauri/src/lib/components/ActionListEditor.svelte`
- Create: `app-tauri/src/lib/components/ActionCard.svelte`
- Create: `app-tauri/src/lib/components/ActionFields.svelte`
- Test: `app-tauri/src/lib/components/ActionListEditor.test.ts`

**Interfaces:**
- Consumes: the selected button in the shared snapshot and Task 1 action mutation commands.
- Produces: label editor and one action list per trigger; add/remove/reorder actions with at least one `none` action retained per trigger, matching `AppState` semantics.
- Action types and editable fields:
  - `none`, `mode_toggle`: no fields.
  - `shortcut`: modifier flags and key.
  - `text`: multi-line text.
  - `media`: media command.
  - `audio_output`: ordered output identifiers in a basic editable list; native picker is M3.
  - `launch_app`: app name and path; app picker is M3.
  - `system_action`: system-action identifier.
  - `bash_script`: script text, with the label “Script”.
  - `home_assistant`: entity, domain, service, and data fields; HA picker is M3.
  - `ha_webhook`: URL.
  - `delay`: seconds in 0–600 with 0.1 step.
- Update only the edited action field so unrecognized action metadata survives JSON round-trips. Testing/executing actions is not enabled until M3.

- [ ] **Step 1: Write failing component tests** for editing each action type, preserving unknown fields, preserving list order, add/remove/reorder, one-item minimum, and both legacy-object and list-shaped trigger slots.
- [ ] **Step 2: Run the action-editor tests** and confirm they fail before the editor exists.
- [ ] **Step 3: Implement the editor components.** Avoid destructive normalization of action fields not currently displayed.
- [ ] **Step 4: Run tests, `npm run check`, and `npm run build`.**
- [ ] **Step 5: Commit** as `feat(tauri-m2): add button action list editor`.

### Task 6: Implement basic Settings and Profiles workflows

**Files:**
- Create: `app-tauri/src/lib/pages/SettingsPage.svelte`
- Create: `app-tauri/src/lib/components/TimingControls.svelte`
- Modify: `app-tauri/src/lib/pages/ProfilesPage.svelte`
- Create: `app-tauri/src/lib/pages/SettingsPage.test.ts`

**Interfaces:**
- Consumes: snapshot, `set_timing_cmd`, `rename_device_cmd`, `save_config_cmd`, and `import_legacy_config`.
- Produces: Settings fields for the three timings and device rename; status information includes device name, firmware version, current transport, and connection message. Profiles displays config path, dirty/saved state, Save, and explicit legacy import.
- Timing field IDs are `debounceMs`, `clickTimeoutMs`, and `holdTimeoutMs`; validation matches the ranges in Global Constraints.
- Import errors and rename write failures produce visible feedback; import never silently saves.

- [ ] **Step 1: Write failing page tests** for min/max/step values, invalid timing rejection, rename trim/24-character validation, import replacing the working snapshot without saving, and save updating dirty state.
- [ ] **Step 2: Run the page tests** and confirm they fail before these pages are wired.
- [ ] **Step 3: Implement Settings/Profiles using the Task 1 save and import APIs.**
- [ ] **Step 4: Run page tests, core bridge/state tests, `npm run check`, and `npm run build`.**
- [ ] **Step 5: Commit** as `feat(tauri-m2): add settings and profile status workflows`.

### Task 7: Implement the live Serial Console

**Files:**
- Create: `app-tauri/src/lib/pages/ConsolePage.svelte`
- Create: `app-tauri/src/lib/components/ConsoleLog.svelte`
- Create: `app-tauri/src/lib/pages/ConsolePage.test.ts`

**Interfaces:**
- Consumes: `serial-log` events and `send_device_command_cmd(command) -> bool`.
- Produces: timestamped, color-classified log lines; Clear; quick commands `WHO`, `INFO`, `SAVE`, `HELP`; a command input with Enter-to-send; maximum 1500 visible lines, trimming the oldest 200 when exceeded.
- Failed writes show an error and preserve the command text; successful writes clear the input. The manager's `[TX]`/`[TX-BLE]` event remains the authoritative hardware-write log.

- [ ] **Step 1: Write failing tests** for event append/classification, quick-command values, Enter-to-send, preserving text on failure, clearing on success, Clear, and trimming after 1500 lines.
- [ ] **Step 2: Run Console tests** and confirm they fail before the page exists.
- [ ] **Step 3: Implement the console and subscribe through the shared state layer.**
- [ ] **Step 4: Run Console tests, `npm run check`, and `npm run build`.**
- [ ] **Step 5: Commit** as `feat(tauri-m2): add live device console`.

### Task 8: Add tray residency, single-instance behavior, and keyboard shortcuts

**Files:**
- Modify: `app-tauri/src-tauri/Cargo.toml`
- Modify: `app-tauri/src-tauri/src/lib.rs`
- Modify: `app-tauri/src-tauri/tauri.conf.json`
- Modify: `app-tauri/src/App.svelte`
- Modify: `app-tauri/src/lib/pages/SettingsPage.svelte`
- Create: `app-tauri/m2-notes.md`
- Test: manual cross-platform smoke checklist in `app-tauri/m2-notes.md`

**Interfaces:**
- Consumes: existing `RuntimeControl`, Tauri window lifecycle, the header's device status, and shared Save command.
- Produces: tray menu with Open, connection status, and Quit; window close hides instead of exiting; Quit signals runtime shutdown. A second launch activates the existing window on Linux and Windows. `--hidden` startup keeps the window hidden for autostart.
- Builder call: use Tauri 2's tray API plus `tauri-plugin-single-instance` and `tauri-plugin-autostart`; the Settings page exposes the autostart toggle. No extra tray framework.
- Register Ctrl+S to save and Ctrl+Q to quit while the app window is focused; Save-as-preset remains M3.

- [ ] **Step 1: Add a testable frontend shortcut handler** and write failing tests for Ctrl+S/Ctrl+Q while ensuring text inputs retain normal editing behavior.
- [ ] **Step 2: Run shortcut tests** and confirm they fail before implementation.
- [ ] **Step 3: Write failing tests for autostart state read/write and the Settings toggle; run them to confirm they fail.**
- [ ] **Step 4: Add the tray and Tauri plugins**, then wire activate/hide/quit/autostart behavior and the tested UI toggle.
- [ ] **Step 5: Run `cargo fmt --all -- --check`, core tests, `npm test`, `npm run check`, and `npm run build`.** Manually smoke-check tray, second launch, close/hide, and autostart on Linux and Windows; record OS-specific results in `m2-notes.md`.
- [ ] **Step 6: Commit** as `feat(tauri-m2): add tray residency and app shortcuts`.

### Task 9: Verify and publish the M2 Windows artifact

**Files:**
- Modify: `.github/workflows/tauri-m0.yml`
- Modify: `app-tauri/m2-notes.md`

**Interfaces:**
- Consumes: completed Tasks 1–8.
- Produces: CI checks for Rust core tests with `serial,ble`, workspace `cargo check`, `npm ci`, frontend tests, `npm run check`, `npm run build`, and the existing Windows NSIS/MSI artifact upload. Owner checklist identifies tray/single-instance, device status, editor, and install verification on Windows.

- [ ] **Step 1: Add workflow commands** for frontend test, check, and build before the Windows Tauri bundle step.
- [ ] **Step 2: Validate YAML and run all locally available checks.**
- [ ] **Step 3: Run the Windows workflow** and confirm it uploads the NSIS/MSI artifacts.
- [ ] **Step 4: Update `app-tauri/m2-notes.md`** with workflow and artifact results, then commit as `ci(tauri-m2): verify UI and publish Windows artifact`.

## M2 Completion Gate

- [ ] `cargo test -p octopad-core --features serial,ble` passes.
- [ ] `npm test`, `npm run check`, and `npm run build` pass.
- [ ] Windows CI workspace check and NSIS/MSI artifact upload pass.
- [ ] Owner confirms the Windows artifact UI, tray, device status, serial/BLE command path, and single-instance behavior.
- [ ] Linux smoke test confirms the UI, tray, autostart, and hide-on-close behavior.
- [ ] Hardware-specific serial/BLE checks are performed only with the legacy Python app closed.

**Handoff:** summarize M2 UI and CI results; proceed to M3 integrations after the owner has exercised the Windows artifact.
