# Tauri Rewrite — M3.1 Action Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development (recommended)
> or superpowers:executing-plans, task by task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make a button press actually *do* something. M0–M2 parse button events and
log them; nothing is executed. This milestone ports the action dispatcher, the action
chain runner, keyboard/text/media injection and the ten built-in system presets, with
Linux and Windows parity.

**Spec:** `docs/superpowers/specs/2026-10-18-tauri-rewrite-design.md`,
`docs/superpowers/specs/2026-10-18-parity-checklist.md` (§7b decisions 1 and 4),
`docs/superpowers/plans/2026-10-19-memory-ui-m3.md` (Phase 3 / M3.1).
Behaviour source of truth: `linux-app/serial_listener.py:70`
(`execute_button_trigger`), `linux-app/system_actions.py:374` (`execute_action`),
`windows-app/platform_actions.py:431`.

**Architecture:** a new `exec` module inside `octopad-core`. The dispatcher is
platform-agnostic and takes an injectable `CommandRunner` plus an `InputInjector`
trait, so every branch is testable without hardware or a display. Platform code sits
behind `#[cfg]` feature flags; the Windows injector compiles only on Windows and is
validated by the Windows CI job.

**Tech stack:** tokio (blocking task for the chain so `delay` never freezes the UI),
`serde`/`serde_json`, and on Windows the `windows` crate for `SendInput`.

## Global Constraints

- **Exact parity of outcomes.** Every action returns `(ok, reason)` and the reason
  strings match the Python apps verbatim, because the console and the tests pin them.
- **A failed action never aborts the chain** (resolved parity decision 1): each failure
  is logged and the sequence continues.
- The chain runs **off the UI thread** (Python uses a daemon thread; we use a tokio
  blocking task) so a `delay` of up to 600 s cannot freeze the window.
- `none` actions are skipped, never executed.
- The chain must re-read the working config at execution time, never a cached copy, and
  must not hold a lock while sleeping.
- Types that belong to M3.2 (`audio_output`, `launch_app`, `home_assistant`,
  `ha_webhook`) must return a clear failure, never a silent success.
- Keyboard injection on Linux must reproduce the Python tool preference order:
  **wtype → ydotool → xdotool**, with the same help text when none is installed.
- `cargo test -p octopad-core --features serial,ble` must stay green (202 tests + new);
  `npm test` (141) must stay green; CI runs both.

## Review Focus

- **Re-entrancy:** a burst of presses must not interleave two chains halfway through.
- **Log honesty:** the `[ACTION] B<k> (<TRIGGER>): <reason> [OK|FAIL]` line must reflect
  what actually happened, including a failing action in the middle of a chain.
- **No silent success:** an unimplemented action type or a missing input tool must be a
  visible failure.
- **Windows code must not be claimed as verified by the Linux build**; only CI compiles
  and tests it.
- **Cancellation:** quitting the app must not leave a detached script or a sleeping chain
  holding the process alive.

---

### Task 1: Exec core — outcomes, dispatcher, command runner

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/exec/mod.rs`
- Create: `app-tauri/src-tauri/crates/core/src/exec/runner.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/exec_dispatch.rs`

**Interfaces:**
- `exec::ActionOutcome { pub ok: bool, pub reason: String }` with
  `ok(reason)` / `fail(reason)` constructors and `Display`.
- `exec::CommandRunner` trait:
  `fn run(&self, program: &str, args: &[&str]) -> ActionOutcome;`
  `fn spawn_detached(&self, program: &str, args: &[&str]) -> ActionOutcome;`
  plus `exec::RealCommandRunner` (std::process) and `exec::RecordingRunner` (test double
  recording every call and returning a scripted outcome).
- `exec::InputInjector` trait:
  `fn available(&self) -> Option<String>;` (returns the tool/name in use),
  `fn shortcut(&self, modifiers: &[String], key: &str) -> ActionOutcome;`,
  `fn text(&self, text: &str) -> ActionOutcome;`,
  `fn media(&self, media_key: &str) -> ActionOutcome;`
  and `exec::NoInputInjector` returning the Python help text.
- `exec::execute_action(action: &Action, deps: &ExecDeps) -> ActionOutcome` where
  `ExecDeps { runner: &dyn CommandRunner, input: &dyn InputInjector }`.

**Behaviour table (must match the Python reasons verbatim):**

| type | outcome |
| --- | --- |
| `none` | `ok("Nothing to do.")` (chains skip it before calling) |
| `mode_toggle` | `ok("Mode layer is controlled by the macropad firmware (LED)")` |
| `delay` | seconds parsed as f64, clamped `0.0..=600.0`, slept, then `ok("Waited {s}s.")`; unparsable → `fail("Delay needs a valid duration.")` |
| `shortcut` | `input.shortcut(modifiers, key)`; missing tool → fail with the Python help text |
| `text` | empty → `fail("Empty text")`, else `input.text(text)` |
| `media` | `input.media(mediaKey)` |
| `bash_script` | stripped empty → `fail("Script content is empty")`; else spawn detached via `/bin/sh -c` (Linux) or `cmd.exe /C` (Windows) and `ok("Bash script dispatched in background")` |
| `system_action` | preset dispatch — see Task 3; unknown id → `fail("Unknown system action: {id}")` |
| `audio_output`, `launch_app`, `home_assistant`, `ha_webhook` | `fail("Not available until M3.2: {type}")` — visible, never a silent ok |
| anything else | `fail("Unknown action type: {type}")` |
| any handler panic/error | `fail("Action execution error: {e}")` |

- [ ] **Step 1: Failing tests** in `tests/exec_dispatch.rs` asserting every row of that
  table, including the exact reason strings, the unknown-type fallback, the clamp
  (`0.0` and `600.0` accepted, `9999` clamped, `abc` fails), and that M3.2 types fail
  loudly. `delay` must be tested through an injected sleeper, never a real sleep.
- [ ] **Step 2: Verify FAIL.** `cargo test -p octopad-core --features serial,ble --test exec_dispatch`.
- [ ] **Step 3: Implement** the module, runner and dispatcher; keep `exec` always
  compiled (no feature gate) so both platforms test the same dispatcher.
- [ ] **Step 4: Verify PASS** plus the full core suite.
- [ ] **Step 5: Commit** `feat(tauri-m3): add action dispatcher with exact outcome parity`.

### Task 2: Linux input injection (wtype → ydotool → xdotool)

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/exec/input_linux.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/exec/mod.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/exec_input_linux.rs`

**Interfaces:**
- `exec::LinuxInjector::detect(runner: &dyn CommandRunner) -> Option<LinuxInjector>` —
  picks the first available tool in order **wtype, ydotool, xdotool** (probe via
  `which <tool>`), stored as an enum.
- `LinuxInjector::tool_name(&self) -> &'static str`.
- The help text string must match `linux-app/system_actions.py:99`.

Behaviour to port:
- `shortcut`: modifiers normalised to the order `ctrl, alt, shift, super`; single
  characters lowercased; wtype → `-M <mod>`… `-P <key> -p <key>` … `-m <mod>`;
  ydotool → evdev codes (`ctrl` 29, `alt` 56, `shift` 42, `super` 125) with a keycode
  map; xdotool → `key --clearmodifiers <mods+key>`.
- `text`: wtype → `wtype -- <text>`; ydotool → `ydotool type -- <text>`;
  xdotool → `xdotool type --clearmodifiers -- <text>`.
- `media`: `VOL_UP`/`VOL_DOWN`/`MUTE` via `wpctl` (preferred) or `pactl`; playback via
  `playerctl`; XF86 fallback through the injector when no tool handles it.

- [ ] **Step 1: Failing tests**: detection order (given all three available, wtype wins;
  given only ydotool, ydotool wins), the exact argv built for each tool, modifier
  ordering, single-char lowercasing, unknown key handling, and the help text when no
  tool exists.
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement.** Keep it `#[cfg(target_os = "linux")]`; provide
  `NoInputInjector` elsewhere so the dispatcher still compiles and is testable.
- [ ] **Step 4: Verify PASS** + full suite.
- [ ] **Step 5: Commit** `feat(tauri-m3): port the linux keyboard injection tool order`.

### Task 3: Built-in system presets (ten, identical ids on both OSes)

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/exec/system.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/exec_system.rs`

**Interfaces:**
- `exec::SYSTEM_PRESETS: &[SystemPreset]` — ten entries with the exact ids shared by
  both Python apps: `mic_toggle_mute`, `volume_up_5`, `volume_down_5`,
  `volume_toggle_mute`, `lock_screen`, `open_terminal`, `screenshot_interactive`,
  `media_play_pause`, `media_next`, `media_prev`, each with `id`, `name`,
  `category`, `description` (OS-neutral wording per parity decision 4: "Lock Session",
  "Screenshot (Area)", …).
- `exec::execute_system_preset(id, runner, input) -> ActionOutcome`.
- Unknown id → `fail("Unknown system action: {id}")`.

Linux behaviour to port from `system_actions.py`: volume and mute via `wpctl`
(preferred) or `pactl`; microphone mute via `pactl set-source-mute
@DEFAULT_AUDIO_SOURCE@ toggle`; lock via `loginctl lock-session`; terminal via the
configured launcher; screenshot interactive via `gnome-screenshot -a` /
`grim -g` / `spectacle -a` with fallbacks; media via the injector's media path.
Each preset must report *which* tool it used, and fail with a clear message when no
tool is available.

- [ ] **Step 1: Failing tests**: the id list matches the Python apps exactly (assert the
  ten ids in order), each preset issues the expected command with the fake runner, and
  the fallback order is respected.
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify PASS** + full suite.
- [ ] **Step 5: Commit** `feat(tauri-m3): add the ten built-in system presets`.

### Task 4: Action chain runner wired to button events

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/exec/chain.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/bridge.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/exec_chain.rs`

**Interfaces:**
- `exec::ChainDeps { state: Arc<AppState>, runner, input }`.
- `exec::spawn_trigger_chain(deps, mode: &str, button_index: i64, trigger: &str)`
  — spawns a tokio blocking task that: resolves mode → button → actions **from the
  working config at run time**, skips `none`, executes each action in order, and
  continues after a failure.
- Logging, mirroring Python exactly:
  - before each action: `Executing action for Button {n} ({trigger}) -> Type: {type}`
  - after each action: `[ACTION] B{n} ({TRIGGER}): {reason} [OK|FAIL]`
  - both emitted through `AppState::log_line` so the console page sees them.
- Out-of-range mode/button/trigger → no work, no log.

- [ ] **Step 1: Failing tests** in `tests/exec_chain.rs`: `none` is skipped; a chain of
  three actions runs in order; a failing middle action does not stop the chain and both
  its `[OK|FAIL]` line and the following `OK` appear; the trigger label is upper-cased in
  the log; an out-of-range index produces nothing; two concurrent presses do not
  interleave inside one chain.
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement** the chain runner and hook `bridge::connect_device_events` so
  `DeviceEvent::ButtonEvent` triggers it (it currently only logs).
- [ ] **Step 4: Verify PASS** + full suite + `cargo fmt --all -- --check`.
- [ ] **Step 5: Commit** `feat(tauri-m3): execute button action chains off the UI thread`.

### Task 5: Windows input injection

**Files:**
- Create: `app-tauri/src-tauri/crates/core/src/exec/input_windows.rs`
- Modify: `app-tauri/src-tauri/crates/core/src/exec/mod.rs`
- Test: `app-tauri/src-tauri/crates/core/tests/exec_input_windows.rs` (cfg-gated)

**Interfaces:**
- `WindowsInjector` implementing `InputInjector` with `SendInput`: modifier and key
  virtual-key maps ported from `windows-app/platform_actions.py`, `KEYEVENTF_UNICODE`
  for typed text, `VK` media keys, and `_MODIFIER_VK` ordering.
- Every fallback returns the Python reasons: "Target key not selected yet",
  "Text typed", "Shortcut sent", "Unknown media key: {k}".

- [ ] **Step 1: Write the tests** — the key/VK maps and media-key table, compiled only
  on Windows.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Verify on Linux** that the module is correctly cfg-gated out and the
  whole core suite still compiles and passes.
- [ ] **Step 4: Report explicitly that Windows verification is CI-only** — do not claim
  it locally. The Windows CI job runs `cargo test -p octopad-core --features serial,ble`.
- [ ] **Step 5: Commit** `feat(tauri-m3): add windows sendinput injector`.

### Task 6: UI feedback for execution

**Files:**
- Modify: `app-tauri/src/lib/pages/ConsolePage.svelte` (if needed)
- Test: existing frontend suites plus a focused test

**Interfaces:**
- Action result lines already arrive through `serial-log`; ensure the console
  classifies `[ACTION] … [FAIL]` as an error line and `[OK]` as a success line, and that
  `clearLogs` still works.

- [ ] **Step 1: Write the failing test** for the classification of an `[ACTION] … [FAIL]`
  line.
- [ ] **Step 2: Verify FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify** `npm test`, `npm run check`, `npm run build`.
- [ ] **Step 5: Commit** `feat(tauri-m3): show action outcomes in the console`.

### Final verification

- [ ] `cargo test -p octopad-core --features serial,ble` green.
- [ ] `cargo fmt --all -- --check` clean; `cargo check --workspace` clean.
- [ ] `npm test` (141+), `npm run check`, `npm run build` green.
- [ ] `.deb` still builds: `PKG_CONFIG_PATH=/tmp/opencode/pkgconfig npx tauri build --bundles deb`.
- [ ] Manual smoke with a real pad (owner): press a key configured with `shortcut`,
  `text` and a two-action chain with `delay`, and confirm the console lines.
- [ ] Record the Linux injector requirement (wtype/ydotool/xdotool) in
  `app-tauri/README.md`.

**Handoff:** report which action types work, which are still M3.2 stubs, and the
Windows verification status honestly.
