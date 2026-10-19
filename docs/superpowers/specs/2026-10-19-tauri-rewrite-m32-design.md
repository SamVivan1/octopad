# M3.2 — pickers, Home Assistant, and the action test button

Milestone: close the gap that makes the app feel unfinished. M3.1 made buttons
run actions; M3.2 makes them *trustworthy* — you can test before pressing, pick
instead of typing, and configure Home Assistant without free-text.

Owner decisions (2026-10-19, binding):

1. **Test runs for real.** The button executes the action on this machine, like
   the Django-era GTK app (`Test this action` / `Test every action on this
   trigger`). A test of `lock_screen` will lock the screen. Consequence accepted.
2. **Home Assistant settings live in the existing SettingsPage** as a section,
   not a new top-level tab.
3. **Selectors get a manual fallback.** Scanned lists fed by dropdowns, plus a
   manual entry option for anything the scan misses.
4. **The cross button always works.** Type can always return to `None`, and a
   slot can always be cleared.
5. Additional owner-reported issues are deferred; the owner will review and
   list them after this milestone ships.

Platform priority is unchanged: **Linux first** (Ubuntu reference, Debian base);
Windows parity follows at the next parity push. `home_assistant` is pure HTTP
and works identically on both.

## 1. Action test button

The core already owns execution (`exec::execute_action`, production deps via
`ChainDeps::production`); the UI has never been able to call it. New Tauri
commands behind `commands::` with thin bodies:

- `test_action_cmd(action)` — runs one action off the UI thread
  (`tokio::task::spawn_blocking`), returns `ActionOutcome { ok, reason }`.
- `test_trigger_cmd(trigger)` — runs every non-`none` action of the selected
  button's trigger, in list order, same result shape (list of outcomes). A
  failing action must not stop the rest, exactly like the real chain.

UI (parity with `linux-app/ui/action_editor.py`):

- Per action card: a **Test** button (`data-testid="action-test"`).
- Per trigger list: **Test all** (`data-testid="action-test-all"`).
- Result surfaces twice: a toast (green/red, `Test: {reason}` per action, ` | `
  joined for test-all — the Python string) and a console line
  `[ACTION] B{n} (TEST): {reason} [OK|FAIL]`, reusing the M3.1 format and the
  existing classifier (a `TEST` label only changes the trigger text; `[FAIL]`
  must stay red).

Reason strings come from the M3.1 dispatcher, already Python-parity.

## 2. Home Assistant section in Settings

New core module `src/ha.rs`, a faithful port of `linux-app/home_assistant.py`
(not pinning its urllib internals — pinning its *endpoints, timeouts, strings,
files, and semantics*).

**Storage (compatibility first):**

- Credentials: `~/.config/octopad/ha_config.json`, `chmod 600`, keys
  `url` / `token`. Read fallback: `~/.config/home-assistant/env` lines
  `HA_URL=` / `HA_TOKEN=` (read-only migration; the old file is never written).
- Entity cache: `~/.config/octopad/ha_entities_cache.json` (`url`, `entities`),
  invalidated when the URL changes — so an old install's scan survives
  migration and a stale HA's entities never show.

**Client behaviours pinned from `home_assistant.py`:**

| Behaviour | Value |
| --- | --- |
| Connection test | `GET {url}/api/`, `Bearer {token}`; 200 → `Connected to Home Assistant ({message})`; 401 → `Authentication failed: invalid Token (401 Unauthorized)`; other → `Server responded with HTTP {n}`; timeout → `Connection timeout to {url} (check the LAN)`; unreachable → `Failed to reach Home Assistant: {e}`; missing credential → `Home Assistant URL or Token not configured` |
| Timeouts | test 3 s, status ping 2 s, manual scan 8 s, first background scan 5 s, service call 5 s |
| Scan retry guard | after a failed scan, no live retry for 30 s (cache serves instead) |
| Entity filter | only domains in `INTERACTIVE_SERVICES` (light, switch, group, input_boolean, fan, cover, climate, humidifier, water_heater, lock, vacuum, media_player, remote, scene, script, automation, button, input_button, timer, number, input_number, select, input_select, text, siren, alarm_control_panel, update, tts) — sensors never appear |
| Entity sort | `(domain, friendly_name.lower())` |
| Entity shape | `entity_id`, `domain`, `friendly_name`, `state`, `icon` |
| Service call | `POST {url}/api/services/{domain}/{service}` with `data or {}` plus `entity_id` when set; 200 → `Service {domain}.{service} executed successfully`; failure strings same table style; HTTP error body contributes its message when present |

**Settings UI** (new `HaSection` component in `SettingsPage`):

- `URL` + `Token` inputs (token is a password field), **Test Connection**
  button testing the *unsaved draft* if both fields are filled, else the saved
  values (Python `check_connection(url=…, token=…)`).
- **Save** persists credentials, clears the entity cache when the URL changed,
  and emits `octopad://ha-status-changed`
  (`{configured, url, connected, message, cached}`).
- Status line always visible: not configured / connected / last message —
  never a made-up state (the page's existing truth rules).

Settings commands: `ha_get_settings_cmd`, `ha_save_settings_cmd(url, token)`,
`ha_test_connection_cmd(url, token)` (empty string = use saved value).

## 3. Entity picker in the action editor

The Home Assistant action becomes pick-driven, replacing free text:

- `ActionFields` for `home_assistant` (and legacy alias `ha_service`) render an
  entity **select** fed by `octopad://ha-entities-changed` (list of scanned
  entities), ordered as the client sorts them, showing
  `{friendly_name} ({entity_id})`.
- The choice writes `entityId`; `domain` is derived from `entity_id` at
  execution time (Python: `domain = entity_id.split('.')[0]` when empty).
- **Service select** per domain from `INTERACTIVE_SERVICES` (defaults to
  `toggle`). Manual service text stays available for anything the table lacks.
- **Rescan** button (`data-testid="ha-rescan"`) lives on the action card —
  forced live scan, not the throttled one; nothing is typed here.
- Not-configured state: the picker is rendered but disabled with the HA
  settings link ("configure Home Assistant in Settings") — selecting an entity
  without credentials cannot pretend to work.
- Config keys unchanged (`domain`, `service`, `entityId`), so a config written
  by the Python app keeps working, including `ha_service`.

## 4. Application and audio-output pickers

New core command surface, Linux first:

- `list_applications_cmd` → emits `octopad://apps-changed` with
  `{id, name}` entries from `*.desktop` lookup across
  `$XDG_DATA_HOME`, `~/.local/share`, `/usr/share`, and both Flatpak export
  dirs; `NoDisplay`/`Hidden`/`TryExec`-missing entries are skipped. Sorted by
  `name.to_lowercase()`; duplicate ids deduplicated with first-wins (XDG
  precedence order).
- `list_audio_outputs_cmd` → `{name, description}` (from `pactl list short
  sinks` — PipeWire's pipewire-pulse serves the same list; `wpctl` has no
  stable machine-readable listing).
- Both scans run off the UI thread on demand and after each Rescan; the
  frontend caches the last list per page session (no background polling —
  startup must stay fast).

**UI in `ActionFields`** (replacing the current placeholders whose hints
already promise "the picker arrives in M3"):

- `launch_app`: **select** of scanned apps writing `desktopId` +
  `appName` in one change, plus a "Manual…" option that reveals the
  `desktopId`/`appName` text inputs (decisions 3 & 4). Default payload stays
  the existing `code.desktop` / `Visual Studio Code` example only as the
  manual path's placeholder, never auto-selected.
- `audio_output`: the existing `sinks` array becomes a checklist of scanned
  outputs (name + description), cycled in the checked order; "Manual…" adds a
  free-text sink name entry for anything the scan missed.
- Execution parity keeps `sinks` / desktopId semantics from the Python app;
  `audio_output` execution moves from the M3.2-stub arm to a real
  implementation in `exec/` (`wpctl set-default` / failure strings pinned to
  `windows-app`/`linux-app` behaviour — the Linux app switched via `wpctl set-default {sink}`).

## 5. The cross button (UX bug)

Root cause found in code: `ActionCard.svelte` gates `✕` behind
`canRemove = total > 1`, and the tooltip admits it ("A trigger always keeps one
action"). The core, however, already guarantees the slot never empties
(`remove_action` replaces the last survivor with `default_action("none")`).

Fix:

- Enable `✕` always. Tooltip on a single-action slot: "Clear to None".
  Behaviour identical to the core's own semantics — nothing new invented.
- A regression test pins the single-slot flow: `none` → `shortcut` → press ✕ →
  slot is `none` again (owner story reproduced as a failing test first). If the
  reported "cannot return to None via the dropdown" reproduces too, its root
  cause is fixed in the same task; if it does not reproduce, the ✕ fix alone
  closes the owner-visible gap and the dropdown path is documented as working.

## Deliverables

- Design (this doc) → plan → tasks. Every task TDD, tests-first, committed per
  task, reason strings pinned by tests against the Python sources listed
  above.
- After the milestone: rebuild `.deb` for owner smoke-test; checklist updated
  in `app-tauri/m3-notes.md`.
- Windows: `list_applications` and audio switching land with the Windows
  parity push (unchanged priority); HA + test button work there as soon as CI
  runs their tests, because both are platform-neutral.

## Non-goals

- Redesigning anything the owner did not flag: no appearance changes, no new
  pages except one section in Settings.
- No HA automations/scripts editor — services only, as the Python app did.
- No background entity polling; scans are user-triggered.
