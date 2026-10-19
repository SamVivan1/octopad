# Tauri Rewrite M3.2 — Test Button, Home Assistant, Pickers

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the four owner-reported gaps: always-working clear button, action Test buttons, a Home Assistant settings section with entity picking, and app/audio-output pickers with manual fallback.

**Architecture:** The core keeps owning execution: two new Tauri commands reuse `exec::execute_action` for Test. A new `ha.rs` core module ports the Python REST client as a managed singleton shared by UI scans and real button executions, with HTTP/clock/filesystem injectable so tests run offline. The frontend stays a thin Svelte view: pickers render core-provided lists, everything edits through `runCommand`.

**Tech Stack:** Rust + Tauri 2 (M3.1 patterns), `ureq` for HTTP (new dep, chosen for memory profile), Svelte 5, existing `dirs`/`serde_json`.

**Spec:** `docs/superpowers/specs/2026-10-19-tauri-rewrite-m32-design.md`

## Global Constraints

- **Linux first**; Windows paths stay stubbed with their M3.1 messages until the next parity push. HA + test button are platform-neutral and get Windows CI runs as-is.
- **UI text English**; dark-only theme.
- **All reason strings Python-verbatim**, pinned by tests citing the Python source lines listed per task. The audio arm is `pactl` parity (`pactl get-default-sink` / `set-default-sink` / `move-sink-input`), never wpctl — verified at `system_actions.py:319-359`.
- **Token never returned to the webview** (`ha_get_settings_cmd` exposes `tokenSaved: bool` only). Empty token field always means "use saved token".
- **Desktop notifications for HA calls: failure only** (`notify-send` via the injected `CommandRunner`).
- **Everything testable offline**: HTTP behind an injected transport, time behind an injected clock, network/file operations never execute during tests.
- **Never hold the state lock while sleeping or doing network I/O**.
- Test baselines that must stay green after every task: core `cargo test -p octopad-core --features serial,ble` (356 passed / 3 ignored), app `cargo test -p octopad-tauri` (13), frontend `npm test` (143). Every task adds to those suites with its own test names. `cargo fmt --all -- --check` clean at commit time.
- Unimplemented action types keep failing loudly; the M3.2 stub arm may only shrink, never grow.

## Review Focus

1. **HA lost mid-action** (cable out, HA restarting): a service call must fail fast with `Connection timeout to {url} (check the LAN)` / `Failed to reach Home Assistant: {e}`, log a red `[FAIL]` line, and *never* hang the button chain (chain continuation-over-failures is already M3.1 behaviour). Test: injected transport returns a timeout error for service call; expect `<ok=false, that exact reason>` (`tests/ha_client.rs::a_lost_ha_fails_fast`).
2. **Scan retry guard is real**: a second `get_entities` within 30 s of a failed scan must perform zero network requests and serve the cache. Test: injectable clock, two scans 10 s apart, transport request counter stays 1 (`tests/ha_client.rs::the_retry_guard_blocks_a_second_live_scan`).
3. **URL change never offers another HA's entities**: Save with a different URL must discard the old cache and make the picker empty until a fresh scan. Test: seed cache for URL A, save URL B, `get_entities` returns empty (`tests/ha_client.rs::a_url_change_discards_the_entity_cache`).
4. **Old configs keep executing**: `ha_service` alias and snake_case `entity_id` still work (Python read both at `system_actions.py:449`). Test: dispatch an `ha_service` action with `entity_id` against a stubbed backend (`tests/exec_dispatch.rs::legacy_ha_keys_still_resolve`).
5. **Test button with no pad connected** must still run actions (exec is transport-free) and log `Executing action for Button 1 (test) -> Type: …`. Test in `tests/exec_chain.rs::a_test_trigger_runs_without_a_device`.

---

### Task 1: The clear button always works

**Files:**
- Modify: `app-tauri/src/lib/components/ActionCard.svelte` (`canRemove`, remove tooltip, `remove()` guard)
- Test: `app-tauri/src/lib/pages/ButtonsPage.test.ts` (or the existing ActionCard test file — follow where action-card assertions live today)

**Interfaces:**
- Consumes: `api.removeAction(trigger, index)` (exists), core `remove_action` semantics: removing the last action replaces it with `none` (`state.rs:504-525`).
- Produces: single-action slots show an enabled `[data-testid="action-remove"]`; clicking it turns the slot's only action into `type: "none"`.

- [ ] **Step 1: Write the failing test** reproducing the owner story: one action, type changes `none → `shortcut`; `[data-testid="action-remove"]` is enabled; clicking it leaves the slot with one action of `type: "none"`. Also `title` reads "Clear to None" on a single slot.
- [ ] **Step 2: Run** `npm test` — confirm the new test fails (button disabled today: `canRemove = total > 1`).
- [ ] **Step 3: Implement** — enable `✕` always; tooltip text switches between "Remove this action" (multi) and "Clear to None" (single); drop the early-return guard only where it defends single slots (keep the programmatic-click argument for a genuinely invalid index, if any).
- [ ] **Step 4: Run** `npm test` (143 + new) and `npm run check`, `npm run build`.
- [ ] **Step 5: Commit** `fix(tauri-m3): the clear button always answers, single slots clear to none`

### Task 2: Core test runner (test one action / one trigger)

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/exec/test_runner.rs` (or extend `chain.rs` — the implementer reads both and keeps one responsibility per file)
- Modify: `crates/core/src/exec/mod.rs` (declarations + re-exports), `app-tauri/src-tauri/src/commands/device.rs`, `src-tauri/src/lib.rs` (`generate_handler!`)
- Test: `app-tauri/src-tauri/crates/core/tests/exec_test.rs`

**Interfaces:**
- Consumes: `execute_action(action, &ExecDeps)`, `ChainDeps::production(state)` (M3.1), `state.log()` → `octopad://serial-log`.
- Produces (called by Task 3 and the commands layer):
  - `pub fn spawn_test_action(state: Arc<AppState>, action: Action) -> JoinHandle<ActionOutcome>` — outcome only, no console preamble.
  - `pub fn spawn_test_trigger(state: Arc<AppState>, button_index: usize, trigger: &str) -> JoinHandle<Vec<ActionOutcome>>` — resolves `mode/button/sections` exactly like `chain.rs::spawn_trigger_chain`, skips `none`, one outcome per action, failures never abort.
- Console lines (identical to the real chain, trigger label `test`): `Executing action for Button {n} (test) -> Type: {type}`, then `[ACTION] B{n} (TEST): {reason} [OK|FAIL]` — produced by the same formatting code path as `chain.rs`, not a second formatter.

- [ ] **Step 1: Failing tests** in `tests/exec_test.rs`: skipped `none` produces no test line; two-action chain in order returns both outcomes; a middle `[FAIL]` still yields the last `[OK]`; console lines appear through a `RecordingState` log double with `B2 (TEST)` shape; a test run needs no device double at all (no serial/ble stubs).
- [ ] **Step 2: Run** `cargo test -p octopad-core --features serial,ble --test exec_test` — fails (module missing).
- [ ] **Step 3: Implement** the two functions; register `test_action_cmd(action)` and `test_trigger_cmd(buttonIndex, trigger)` as thin `#[tauri::command]`s that block on the handles' results (they must run off the UI thread: the spawn_blocking does the work, the command awaits).
- [ ] **Step 4: Run** full core suite (356 + new), `cargo check --workspace`, `cargo fmt --all -- --check`.
- [ ] **Step 5: Commit** `feat(tauri-m3): run actions from the UI with a test trigger`

### Task 3: Test buttons in the editor + toast results

**Files:**
- Create: `app-tauri/src/lib/toast.ts` (minimal store: `{ message, kind }[]`, auto-expiry ~4 s) and `app-tauri/src/lib/components/ToastHost.svelte` (fixed, non-modal, `role="status"`, reuses `--success/--error` tokens)
- Modify: `app-tauri/src/lib/api.ts` (two wrappers), `ActionCard.svelte` (per-card Test button `data-testid="action-test"`), `ActionListEditor.svelte` (Test all `data-testid="action-test-all"`)
- Test: `app-tauri/src/lib/toast.test.ts` + extension of the ButtonsPage/ActionCard tests

**Interfaces:**
- Consumes: Task 2's `api.testAction(action)` / `api.testTrigger(buttonIndex, trigger)` returning `ActionOutcome { ok, reason }` / `{ ok, reason }[]`.
- Produces: `toast.show(message, kind: "success" | "error" | "info")` — later tasks reuse it.

- [ ] **Step 1: Failing tests**: toast store pushes/expiry (fake timers per the existing test style); a card click invokes `testAction` with that exact action and renders a green toast `Test: {reason}` on `ok`, red on `!ok`; Test all invokes `testTrigger` and joins with ` | ` prefixed `Test: `; rejections surface through `lastError`, not a fake success.
- [ ] **Step 2: Run** `npm test` — fails.
- [ ] **Step 3: Implement** buttons + wiring through the page's `onEdit` queue where an edit would race (Test is an execution, not an edit: it runs *outside* the serialized edit queue, in parallel with it); mount `ToastHost` in `AppShell`.
- [ ] **Step 4: Run** `npm test`, `npm run check`, `npm run build`.
- [ ] **Step 5: Commit** `feat(tauri-m3): test buttons run actions for real with toast results`

### Task 4: HA client core (`ha.rs`)

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/ha.rs`
- Modify: `crates/core/Cargo.toml` (`ureq = "2"`, default features off, `tls` feature on) and `crates/core/src/lib.rs` (module decl)
- Test: `app-tauri/src-tauri/crates/core/tests/ha_client.rs`

**Interfaces:**
- Produces (every later task consumes these):
  - `pub struct HaState { … }` — managed singleton, `Arc`-safe, one throttle clock + one in-memory entity cache + disk cache.
  - `pub fn load(state_dir: &Path) -> HaState` — credentials from `{dir}/ha_config.json` (exact Python file, keys `url`/`token`, written with mode 0o600), read-only fallback `~/.config/home-assistant/env` lines `HA_URL=`/`HA_TOKEN=`.
  - `pub fn check_connection(&self, url: Option<String>, token: Option<String>) -> CheckResult` (`CheckResult { ok, message }`) — empty `url`/`token` argument means "use saved value"; timeout `(2,3)` s here.
  - `pub fn save(&self, url: String, token: String) -> bool` — clears the entity cache *iff* the URL changed, writes the file, returns false on IO error.
  - `pub fn get_entities(&self, force_refresh: bool) -> Vec<EntitySummary>` — `EntitySummary { entity_id, domain, friendly_name, state: Option<String>, icon: Option<String>, services: Vec<String>, default_service: String }`; filter per `INTERACTIVE_SERVICES` (exact table, spec §2), sort `(domain, friendly_name.to_lowercase())`, disk cache `ha_entities_cache.json` (`url` + `entities`), timeouts: background `(1.5,5)` first-scan-while-empty / forced `(2,8)` (spec: manual scan 8 s), 30 s failed-scan retry guard.
  - `pub fn call_service(&self, domain: &str, service: &str, entity_id: Option<&str>, data: &serde_json::Value, runner: &dyn CommandRunner) -> ActionOutcome` — `POST {url}/api/services/{domain}/{service}` with `data-or-{} + entity_id`, 5 s; **200 → `Service {domain}.{service} executed successfully`**; failure strings per spec §2 table; **notify-send failure-only** via `runner.spawn_detached` (success path never touches the runner).
  - `pub fn send_webhook(&self, url: &str, data: &serde_json::Value) -> ActionOutcome` — `POST {url}` `application/json`, 4 s, `Webhook sent to Home Assistant (HTTP {status})`, empty URL → `Home Assistant Webhook URL is empty`.
- Injection seams (tests never touch the network or `~`):
  - `trait HaHttp { fn request(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> }` with a production `UreqTransport`.
  - `trait HaClock { fn secs(&self) -> f64; }` (monotonic) + a `HaHome` trait or paths-only constructor for dirs.
- 401 body → `Authentication failed: invalid Token (401 Unauthorized)`; other status → `Server responded with HTTP {n}` (and HTTP error body message appended when JSON `message` exists, exactly like the Python branch).

- [ ] **Step 1: Failing tests** (name the pinned Python line in a comment per test): the check-connection string table (200/401/500/timeout/unreachable/missing-credential), save+chmod+URL-change-invalidation, entity filter/sort/shape, disk-cache round-trip and cross-URL discard, retry guard via injected clock, service-call success+failure strings and notify-send-on-failure-only, webhook strings. Timeouts are constants verified by assertion (no real sleeps).
- [ ] **Step 2: Run** `cargo test -p octopad-core --features serial,ble --test ha_client` — fails.
- [ ] **Step 3: Implement** `ha.rs` against the interfaces exactly; no network call outside `HaHttp`.
- [ ] **Step 4: Run** full core suite, no-features compile, `cargo check --workspace`, fmt.
- [ ] **Step 5: Commit** `feat(tauri-m3): port the home assistant rest client`

### Task 5: HA settings section + commands

**Files:**
- Modify: `crates/core/src/state.rs` (hold `Arc<HaState>` on `AppState`; construct in `DeviceManager` start-up), `src-tauri/src/commands/device.rs`, `src-tauri/src/lib.rs`
- Create: `app-tauri/src-tauri/src/commands/ha.rs`
- Create: `app-tauri/src/lib/components/HaSection.svelte`; Modify: `SettingsPage.svelte` (mount below the pad section)
- Test: `app-tauri/src/lib/pages/SettingsPage.test.ts` (add HA cases)

**Interfaces:**
- Consumes: Task 4's `HaState` singleton.
- Produces (Task 6 consumes): commands `ha_get_settings_cmd()` → `{ url: string, tokenSaved: bool, configured: bool }`; `ha_save_settings_cmd(url, token)` → emitted `octopad://ha-status-changed` `{configured, url, connected, message, cached}` + cache invalidation; `ha_test_connection_cmd(url, token)` (empty string = saved value) → `{ok, message}`. `haStatusChanged` event + `api.haGetSettings/haSaveSettings/haTestConnection` wrappers.

- [ ] **Step 1: Failing tests**: SettingsPage renders the section only from the store (no local edit — the page's standing truth rule); Test with both fields filled sends them; Test with empty token field reports "saved" usage in the displayed request description; Save with empty token field must keep the saved token (sending "" never blanks a stored token — the command layer substitutes the saved value); the token field never renders a value from the past (redacts: the API returns no token).
- [ ] **Step 2: Run** — fails.
- [ ] **Step 3: Implement** commands (thin, no business logic) + section UI (URL, password token with "token saved" placeholder when `tokenSaved`, **Test Connection**, **Save**, status line driven by `haStatusChanged`).
- [ ] **Step 4: Run** core (full), app `cargo test -p octopad-tauri`, frontend `npm test`, `npm run check`, fmt.
- [ ] **Step 5: Commit** `feat(tauri-m3): home assistant settings in the settings page`

### Task 6: HA execution + entity picker on the action page

**Files:**
- Modify: `crates/core/src/exec/mod.rs` (dispatcher arms), `crates/core/src/exec/` (`ExecDeps` gains `ha: &dyn HaBackend` where `HaBackend` re-exposes `call_service`/`send_webhook` — production wiring from `AppState`, a `NullHa` in tests returning `Home Assistant not configured`)
- Modify: `app-tauri/src/lib/components/ActionFields.svelte` (HA fields become entity select + service select + rescan), `state.ts` (subscribe to `ha-entities-changed`), `api.ts` (`haRescanCmd`)
- Test: `crates/core/tests/exec_dispatch.rs` (extend), `app-tauri/src/lib/components/ActionFields.test.ts` or the page-level test where fields are exercised

**Interfaces:**
- Consumes: Task 4 client, Task 5 events; dispatcher currently returns `Not available until M3.2: home_assistant` — this task deletes that lie.
- Produces: `home_assistant` / `ha_service` / `ha_webhook` execute for real; frontend `haEntityOptions` store; emits `ha_rescan_cmd()` → forced scan + `octopad://ha-entities-changed`.

- [ ] **Step 1: Failing core tests**: `home_assistant` with `entityId` (and alias `ha_service` with `entity_id` — Review Focus 4) resolves domain from `entity_id`, uses the backend, returns the client's outcome verbatim; empty domain+service → `Home Assistant domain or service not set`; unconfigured → `Home Assistant not configured`; `ha_webhook` sends via the backend with the webhook strings; the M3.2 stub arm shrinks to exactly `audio_output`/`launch_app`.
- [ ] **Step 2: Failing frontend tests**: opening an HA action card requests one throttled background scan (api spy: two cards → one call; remount with a cache → zero live calls is already the core's business, the frontend just calls once per mount); entity select writes `entityId` and derives the select's service options from that entity's `services`; service select defaults to the entity's `default_service`; no match/services → manual text input; `ha-rescan` calls the command; unconfigured → disabled select + link caption.
- [ ] **Step 3: Implement** dispatcher wiring + UI; register `ha_rescan_cmd`.
- [ ] **Step 4: Verify** both suites fully, both fmts, workspace check.
- [ ] **Step 5: Commit** `feat(tauri-m3): home assistant actions execute with entity picking`

### Task 7: Application picker

**Files:**
- Create: `crates/core/src/apps.rs`
- Modify: `commands/` (`list_applications_cmd`, event emission), `ActionFields.svelte` (launch_app select + Manual…), `state.ts`, `api.ts`
- Test: `crates/core/tests/app_scanner.rs`, frontend field tests

**Interfaces:**
- Consumes: directories injection (the scanner takes `Vec<PathBuf>` roots — tests pass synthetic trees, never the real `$HOME`).
- Produces: `pub fn scan_applications(roots: &[PathBuf]) -> Vec<AppEntry> where AppEntry { id: String, name: String }` — XDG order first-wins, `(name.to_lowercase())` sort, `Type`=Application + `Name` + `Exec` required, `NoDisplay`/`Hidden` skipped (spec §4 list; Snap tree included); `apps_changed` event `{url: "octopad://apps-changed", apps: AppEntry[]}` and `api.listApplications()`.

- [ ] **Step 1: Failing tests**: dedup order by root precedence; sort; skip `NoDisplay`/`Hidden`/`Type!=Application`/`Name`-missing; fixture tree with a `.desktop` per case; name extraction only from `[Desktop Entry]`.
- [ ] **Step 2: Run** — fails.
- [ ] **Step 3: Implement** parser (one INI-ish reader, `[Desktop Entry]` section only) + command + event; frontend: select writes `desktopId`+`appName` in one change; "Manual…" reveals the two text inputs (placeholders only, never auto-selected).
- [ ] **Step 4: Verify** suites (core full / no-features / frontend), fmt.
- [ ] **Step 5: Commit** `feat(tauri-m3): application picker with manual fallback`

### Task 8: Audio outputs — scan, picker, pactl execution

**Files:**
- Create: `crates/core/src/audio.rs`
- Modify: `exec/mod.rs` (replaces the `audio_output` stub arm), `commands/`, `ActionFields.svelte` (sinks become a checklist + Manual…), `api.ts`
- Test: `crates/core/tests/audio.rs` (extends the exec suite style)

**Interfaces:**
- Consumes: `CommandRunner` only (all pactl calls through it — tests with `RecordingRunner`, never real audio).
- Produces: `pub fn list_sinks(runner: &dyn CommandRunner) -> Vec<SinkEntry>` (`SinkEntry { name, description }`) via `pactl -f json list sinks` and the plain-text fallback; `pub fn execute_audio_output(sinks: &[String], runner: &dyn CommandRunner) -> ActionOutcome`; commands `list_audio_outputs_cmd`; event-free (response only, per spec).
- Strings pinned (verified `system_actions.py:319-359`): `Default output set to {name}`; `pactl is required to switch the audio output`; `No audio output selected`; current default via `pactl get-default-sink`; playing streams follow via `pactl move-sink-input {stream} {sink}` for every returned stream; stream move is best-effort (`check`-style failures logged, not fatal, matching the Python helper).

- [ ] **Step 1: Failing tests**: JSON parse shape + text fallback (`Sink #0\n\tName: a\n\tDescription: Ardor`-style block parse); cycle picks the input after the current default; wrap-around; missing pactl string; not-selected string; `move-sink-input` called per stream; sinks-arg empty → `No audio output selected`.
- [ ] **Step 2: Run** — fails.
- [ ] **Step 3: Implement** + checklist UI (checked order = order in `sinks`; Manual… adds a free-text name) + delete the stub arm so M3.2's stub set is `{launch_app}` before Task 7 lands.
- [ ] **Step 4: Verify** suites + fmt.
- [ ] **Step 5: Commit** `feat(tauri-m3): audio output picker and pactl switching`

### Task 9: Cross-suite verification, .deb rebuild, notes

**Files:**
- Modify: `app-tauri/m3-notes.md` (owner smoke checklist for M3.2)

- [ ] **Step 1:** Full local verification in one pass: core `cargo test -p octopad-core --features serial,ble` (baseline + all new), `cargo test -p octopad-core` (no features), `cargo check --workspace`, `cargo fmt --all -- --check`, `cargo test -p octopad-tauri`, `npm test`, `npm run check`, `npm run build`.
- [ ] **Step 2:** Push; wait for the three CI jobs green — the Windows job is the first *execution* of the HA client and test buttons on Windows; report its result, never assume.
- [ ] **Step 3:** `PKG_CONFIG_PATH=/tmp/opencode/pkgconfig npx tauri build --bundles deb`; record artifacts.
- [ ] **Step 4:** Rewrite the M3.2 part of `app-tauri/m3-notes.md` — what works, what stays stubbed (`launch_app` until Task 7 — if all landed, nothing), owner manual checklist: configure HA in Settings → Test Connection → lock-screen test warning, entity picker press, sink switch with playing audio, Test all on a mixed chain, bash notify-send failure popup case.
- [ ] **Step 5:** Commit `docs(tauri-m3.2): M3.2 owner checklist and verification status`
