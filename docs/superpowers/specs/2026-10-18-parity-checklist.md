# OctoPad Desktop — Feature Parity Checklist (Tauri rewrite, v1)

Date: 2026-10-18
Scope: v1 of the Tauri rewrite described in
[`2026-10-18-tauri-rewrite-design.md`](./2026-10-18-tauri-rewrite-design.md).
Sources of truth: the code in `linux-app/` and `windows-app/`. Where a README
disagrees with the code, the code wins and the disagreement is listed in
§7.

This checklist is the acceptance list for **M4 — Parity audit**: every row of
§3 is either "Yes" on the new app on both OSes, or it is in §6 (out of scope)
or §7 (open question, decision pending).

---

## 1. What "parity" means

Parity is the **union of the user-visible features of both existing apps** —
not the intersection:

- Every behavior a Linux user can see today must exist on Linux *and*
  Windows in the new app, and vice versa.
- Single-OS features become cross-platform requirements (see table below).
- Internal architecture is *not* part of parity: the new app may use one
  config shape, one state model, one design system.
- Descriptions in READMEs are not parity requirements unless the code backs
  them (the firmware protocol in `octopad.ino` is a separate contract and is
  frozen — see `2026-10-18-tauri-rewrite-design.md:54`).

Single-OS features that must land on **both** OSes:

| Feature | Exists only in | Evidence |
| --- | --- | --- |
| Profiles page (separate page with built-in + user presets) | `windows-app` (on Linux it is a section of the Settings page) | `windows-app/ui/profiles_page.py:16`, `linux-app/ui/settings_page.py:40` |
| GUI Profiles page vs. combined page | (see above) | – |
| Header/sidebar collapse toggle | `linux-app` | `linux-app/ui/window.py:88-93` |
| Audio-output switching action | `linux-app` only today | `linux-app/system_actions.py:353-372`, `windows-app/platform_actions.py:346-366` |
| Ctrl+S / Ctrl+Shift+S / Ctrl+Q accelerators | `linux-app` only today | `linux-app/main.py:183-185` |
| System-tray autostart entry | both, by different mechanism | `linux-app/install.sh:110-124`, `windows-app/install.ps1:50-57` |
| Single-instance enforcement | `windows-app` only today | `windows-app/main.py:89-108` |

---

## 2. Screens / pages inventory

Both apps have the same four logical pages, in the same order, with the same
sidebar labels. Linux splits the "Profiles" content into `settings_page.py`;
Windows has a dedicated `profiles_page.py`.

### 2.1 `linux-app/` (GTK4 + Libadwaita)

Windows/page list: `linux-app/ui/window.py:15-20`. Pages are lazily created
on first visit (`linux-app/ui/window.py:234-256`).

| Page | Source | What the user sees | Controls and behavior | Shortcuts |
| --- | --- | --- | --- | --- |
| **Buttons** | `linux-app/ui/keys_page.py` | 8-keycap grid + right-hand action inspector | Mode switch (2 linked toggle buttons, `keys_page.py:75-86`); timing summary label (`:96`); 8 keycap cards with index, label, and 3 trigger chips (`:115-167`); clicking a card selects it (`:171`); inspector shows button label entry (`:230-234`), then one `ActionEditor` per trigger | – |
| **Serial Console** | `linux-app/ui/console_page.py` | Timestamped, color-classified hardware log + command input | "Clear" button (`:25`); quick buttons `WHO`, `INFO`, `SAVE`, `HELP` (`:31-33`); command entry with placeholder `e.g. CMD:PING` (`:53`); "Send" button (`:57`); log auto-trims (`:110-114`) | `Enter` in the entry sends |
| **Profiles** | `linux-app/ui/settings_page.py:40-251` | Current-profile card, built-in presets, "My Presets" | Configuration-file path row (`:51-55`); dirty/saved state row (`:57-69`); "Save as Preset…" (`:58`), "Save" (`:64-68`); "Import…"/"Export…" (`:75-78`); 3 built-in preset rows with Apply (`:90-99`); user-preset rows with Load and Delete (`:149-167`); About group (`:111-117`) | Ctrl+S (save), Ctrl+Shift+S (save preset) |
| **Settings** | `linux-app/ui/settings_page.py:255-342` | Preferences page | Timing spin rows: debounce 0–200 step 5, click 50–2000 step 5, hold 100–3000 step 5 (`:270-279`); HA "Connection…" button (`:285`) + "Available entities" with Reload (`:291-297`); app scanner rescan (`:301-310`); device rename with max 24 chars (`:319-325`, `:366-380`); Information group with paths (`:327-338`) | – |
| Menu (header) | `linux-app/ui/window.py:136-172` | Hamburger menu | Save / Save as Preset / Import Profile… / Export Profile… / Apply built-in preset ×3 / About OctoPad | – |
| HA Connection dialog | `linux-app/ui/ha_dialogs.py:24-213` | URL + token form, status banner | "Test Connection" (`:72`), "Reload" (`:160`), Save (`:184`); status is read from cache instantly, live check runs in background (`:118-138`) | – |
| Entity Picker | `linux-app/ui/ha_dialogs.py:215-*` | Searchable entity card grid | search entry (`:245-248`), domain filter, reload (`:231`), clicking a card assigns the entity (`:329`) | – |
| App Picker | `linux-app/ui/app_picker.py:15-…` | Searchable list of `.desktop` apps | search entry (`:35-36`), row activate picks (`:99` does the launch, pick callback at `:16`) | – |
| Audio Output picker | `linux-app/ui/audio_picker.py:15-113` | Multi-select list of PipeWire/PulseAudio sinks | rows added from `list_audio_sinks()` (`:74`, `:115`); "Apply" returns the ordered list (`:107-110`) | – |

Header extras (Linux only): sidebar collapse toggle
(`linux-app/ui/window.py:88-93`), menu button (`:83-86`), connection pill with
a USB/Bluetooth transport icon (`:95-98`, `:210-225`), HA pill (`:100-102`),
Save button (`:104-106`).

### 2.2 `windows-app/` (PySide6 / Qt6)

Page list: `windows-app/ui/window.py:18-23`.

| Page | Source | What the user sees | Controls and behavior | Shortcuts |
| --- | --- | --- | --- | --- |
| **Buttons** | `windows-app/ui/keys_page.py` | Same layout as Linux: grid + inspector | Exclusive mode buttons (`:44-53`); timing tooltip label (`:57-58`); 8 keycap buttons (`:89-95`); inspector with label `QLineEdit` (`:125-127`) and one `ActionEditor` per trigger (`:117-135`) | – |
| **Serial Console** | `windows-app/ui/console_page.py` | Log + command input | Same feature set as Linux (clear, quick commands, send) | `Enter` in the entry sends |
| **Profiles** | `windows-app/ui/profiles_page.py:16-200` | Same content as the Linux Profiles page | Config path (`:29-34`), state + Save as Preset + Save (`:36-45`), Import/Export (`:47-55`), built-in presets (`:59-72`), user presets Load/Delete (`:118-128`), About (`:79-85`) | Ctrl+S |
| **Settings** | `windows-app/ui/settings_page.py` | Preferences page | Timing spin boxes: debounce 0–1000 step 5, click 50–2000 step 10, hold 50–3000 step 10 (`:30-38`); HA connection; entity reload; app scanner rescan; device rename | – |
| Menu / toolbar | `windows-app/ui/window.py:70-118` | `Menu` menu + a toolbar | Save (Ctrl+S), Save as Preset…, Profiles ▸ Import/Export, Built-in Presets ▸ …, About OctoPad (`:70-98`); toolbar Save / connection button / HA button (`:100-118`) | Ctrl+S (`:74`) |
| HA dialogs | `windows-app/ui/ha_dialogs.py` | Connection dialog + entity picker | Mirrors Linux | – |
| App picker | `windows-app/ui/app_picker.py` | Start Menu shortcut picker | Mirrors Linux, different source | – |
| Audio picker | `windows-app/ui/audio_picker.py` | Windows output-device picker | Mirrors Linux, different source | – |

---

## 3. Feature matrix

Status legend: **Yes** = user-visible feature present; **Partial** = present
but with a documented gap; **No** = absent. "Target" is always Yes/Fixed.

| # | Feature | linux-app | windows-app | Target (both) | Notes (source) |
| --- | --- | --- | --- | --- | --- |
| 1 | USB serial connection | Yes | Yes | Yes | `linux-app/serial_listener.py:269-340`; `windows-app/serial_listener.py:150-320`; 115200 baud both (`linux-app/serial_listener.py:117`, `windows-app/serial_listener.py:113`) |
| 2 | BLE connection | Yes (bluez D-Bus) | Yes (`bleak`) | Yes | `linux-app/ble_bridge.py:30-54,299`; `windows-app/ble_bridge.py:151-256`; both use service `6f63746f-…0001` + legacy NUS service (`linux-app/ble_bridge.py:36-38`, `windows-app/ble_bridge.py:27-28`); legacy TX UUID disagrees — see §7.16 |
| 3 | Device discovery / identification (`WHO` + `HELLO`) | Yes | Yes | Yes | `linux-app/serial_listener.py:28-67,244-289`; `windows-app/serial_listener.py:32-73,213-259`; BLE also parses HELLO (`linux-app/ble_bridge.py:570-574`) |
| 4 | Device name + firmware display | Yes | Yes | Yes | Linux Settings device row shows name + `firmware v…` (`linux-app/ui/settings_page.py:349-358`); Windows window polls `listener.device_name` (`windows-app/ui/window.py:284`) — **no firmware version shown** |
| 5 | Status indicators (connection + HA) | Yes (two pills, transport icon) | Yes (two buttons, tooltip only) | Yes | `linux-app/ui/window.py:95-114,210-225`; `windows-app/ui/window.py:100-118,236-270` |
| 5b | Single-instance / "focus existing window" | No | Yes | Yes | `windows-app/main.py:15,89-127`; nothing equivalent in `linux-app/main.py` (Adw.Application does not forward activation across processes) |
| 5c | Hide-to-tray on window close | Yes | Yes | Yes | `linux-app/ui/window.py:333-340` (`hide()`, returns True); `windows-app/ui/window.py:302-307` (`event.ignore(); self.hide()`) |
| 6 | Button grid (8 keys, index/GPIO/label/3 triggers) | Yes | Yes | Yes | `linux-app/ui/keys_page.py:115-207`; `windows-app/ui/keys_page.py:89-116` |
| 7 | Button label editing | Yes | Yes | Yes | `linux-app/ui/keys_page.py:230-234`; `windows-app/ui/keys_page.py:125-127` |
| 8 | Per-trigger action lists: add / remove / reorder | Yes | Yes | Yes | `linux-app/ui/action_editor.py:85-95`; `windows-app/ui/action_editor.py:80-89`; order is persisted as JSON array (`linux-app/config.py:237-246`) |
| 9 | Action "Test all" + per-action "Test" | Yes | Yes | Yes | `linux-app/ui/action_editor.py:46-49,228-248`; `windows-app/ui/action_editor.py:46-49,186-199` |
| 10 | Action editor fields per type (12 types) | Yes | Yes | Yes | see §3b |
| 11 | Timing controls (debounce / click / hold) | Yes | Yes | Yes | `linux-app/ui/settings_page.py:270-279`; `windows-app/ui/settings_page.py:30-38`; ranges differ (§7) |
| 12 | Timing pushed to the pad (`TIMING:` on connect + save) | Yes | Yes | Yes | `linux-app/state.py:58-70,284-287`; `windows-app/state.py` mirrors; BLE also syncs on connect (`linux-app/ble_bridge.py:306-308`, `windows-app/ble_bridge.py:251-255`) |
| 13 | Mode switching (Desktop / Home Assistant) | Yes | Yes | Yes | `linux-app/ui/keys_page.py:75-109`; `windows-app/ui/keys_page.py:44-75`; `MODE:` command supported |
| 14 | Built-in presets (3) | Yes | Yes | Yes | `linux-app/config.py:227-234`; `windows-app/config.py:232-239`; ids identical, labels differ (§7) |
| 15 | User presets (create / overwrite / load / delete) | Yes | Yes | Yes | `linux-app/state.py:296-364`; `windows-app/state.py:310-366`; slugged files in `presets/` |
| 16 | Profiles import / export (JSON) | Yes | Yes | Yes | `linux-app/ui/settings_page.py:202-245`; `windows-app/ui/profiles_page.py:158-189`; Windows import also saves immediately (`:174`), Linux does not |
| 17 | HA connection dialog + entity cache | Yes | Yes | Yes | `linux-app/ui/ha_dialogs.py:24-213`; `windows-app/ui/ha_dialogs.py` |
| 18 | HA entity picker (search + domain filter) | Yes | Yes | Yes | `linux-app/ui/ha_dialogs.py:215-329`; `windows-app/ui/ha_dialogs.py` |
| 19 | HA service calls (native REST) | Yes | Yes | Yes | `linux-app/home_assistant.py`; `windows-app/home_assistant.py`; action type `home_assistant` |
| 20 | HA webhooks | Yes | Yes | Yes | action type `ha_webhook` (`linux-app/config.py:65`, `windows-app/config.py:71`) |
| 21 | App scanner | Yes (`.desktop`) | Yes (Start Menu) | Yes | `linux-app/app_scanner.py:16-99`; `windows-app/app_scanner.py:13-62` |
| 22 | System actions (10 presets) | Yes | Yes | Yes | `linux-app/system_actions.py:25-77`; `windows-app/platform_actions.py:38-59`; ids identical |
| 23 | Keyboard shortcut / text typing | Yes (`wtype`→`ydotool`→`xdotool`) | Yes (`SendInput`) | Yes | `linux-app/system_actions.py:92-206`; `windows-app/platform_actions.py:110-125` |
| 24 | Media keys | Yes | Yes | Yes | `linux-app/system_actions.py:218-253`; `windows-app/platform_actions.py:79-82` |
| 25 | Audio output switching | Yes | Yes | Yes | `linux-app/system_actions.py:353-372` (`pactl`); `windows-app/platform_actions.py:346-366` (IPolicyConfig COM) |
| 26 | Screenshot / lock screen / terminal | Yes | Yes | Yes | `linux-app/system_actions.py:465+`; `windows-app/platform_actions.py:368-404` |
| 27 | Tray icon + menu (Open / status / Quit) | Yes (StatusNotifierItem) | Yes (`QSystemTrayIcon`) | Yes | `linux-app/tray.py:5,284-302`; `windows-app/tray.py:41-70`; both expose 3 status lines |
| 28 | Console / log page | Yes | Yes | Yes | `linux-app/ui/console_page.py`; `windows-app/ui/console_page.py` |
| 29 | Settings page | Yes | Yes | Yes | `linux-app/ui/settings_page.py:255`; `windows-app/ui/settings_page.py` |
| 30 | First-run / onboarding | No | No | Not required | no onboarding code found in either app; first run uses `default_config()` (`linux-app/config.py:154-191`) |
| 31 | Autostart in background / tray | Yes | Yes | Yes | `linux-app/install.sh:110-124` (`--hidden`), DEB `/etc/xdg/autostart` (`packaging/deb/build_deb.sh:71-83`); `windows-app/install.ps1:50-57` (`--hidden`) |
| 32 | Single instance | No | Yes | Yes | `windows-app/main.py:89-108`; Linux has no equivalent |
| 33 | Dark theme | Yes (forced dark + custom CSS) | Partial (default Qt palette, no custom CSS) | Yes | `linux-app/main.py:177-181`, `linux-app/ui/css.py`; Windows has no CSS module |
| 34 | About dialog | Yes (`Adw.AboutWindow`) | Yes (`QMessageBox.about`) | Yes | `linux-app/ui/window.py:174-186`; `windows-app/ui/window.py:120-127` |
| 35 | Device rename | Yes | Yes | Yes | `linux-app/ui/settings_page.py:360-390` (max 24 chars); `windows-app/ui/settings_page.py` |
| 36 | App menu with import/export/preset shortcuts | Yes | Yes | Yes | `linux-app/ui/window.py:136-172`; `windows-app/ui/window.py:70-98` |
| 37 | Toast / status feedback | Yes (`Adw.Toast`) | Yes (status bar) | Yes | `linux-app/ui/window.py:191-194`; `windows-app/ui/window.py:173-174` |

### 3b. Action types and their editor fields

Type ids are **identical** in both apps (`linux-app/config.py:55-68`,
`windows-app/config.py:61-74`). Only the display label for `bash_script`
differs ("Bash Script" vs "Script / Command").

| Type | Editor fields | Linux | Windows |
| --- | --- | --- | --- |
| `none` | – | `action_editor.py:253` | `:213` |
| `shortcut` | 4 modifier toggles + key dropdown (KEY_GROUPS: A–Z, 0–9, F1–F24, nav/control) | `:281-323` | `:239-275` |
| `text` | multi-line text | `:325-342` | `:277-287` |
| `media` | media dropdown (6 keys) | `:344-361` | `:289-302` |
| `audio_output` | "Pick outputs" dialog → ordered sink list | `:363-398` | `:304-332` |
| `launch_app` | app picker + `appName` | `:400-433` | `:334-356` |
| `system_action` | dropdown of 10 system presets | `:435-468` | `:358-378` |
| `bash_script` | script text (default `notify-send …` / `msg * …`) | `:470-487` | `:380-390` |
| `home_assistant` | entity picker + domain/service dropdown + Connection… | `:499-579` | `:403-469` |
| `ha_webhook` | URL entry | `:489-497` | `:392-401` |
| `delay` | seconds spin, 0–600 step 0.1 | `:263-279` | `:220-237` |
| `mode_toggle` | – | `:260-261` | `:216-218` |

---

## 4. Per-OS platform behavior

| Behavior | Linux | Windows | New app must |
| --- | --- | --- | --- |
| Keyboard shortcut injection | `wtype` → `ydotool` → `xdotool`, first found (`system_actions.py:92-96`); error message names all three (`:99-103`) | `SendInput` with a virtual-key table (`platform_actions.py:69-82,110-125`) | Use a native injector per OS |
| Text typing | `wtype -- <text>` / `ydotool type` / `xdotool type` (`system_actions.py:170-178`) | `KEYEVENTF_UNICODE` per character (`platform_actions.py:105-108`) | Same user result |
| Media keys | `wpctl`/`pactl` for volume, `playerctl` for transport, XF86 keys via injector (`system_actions.py:218-252`) | Media VK codes `0xAD/0xAE/0xAF/0xB0/0xB1/0xB3` (`platform_actions.py:79-82`) | Same 6 media actions |
| Audio-output switching | `pactl list sinks` (JSON, falls back to text) → `pactl set-default-sink`; moves existing streams with `pactl move-sink-input` (`system_actions.py:273-352`) | `pycaw` to enumerate, IPolicyConfig COM `SetDefaultEndpoint` for all 3 roles (`platform_actions.py:256-345`) | Both; Linux-only `_move_streams_to_sink` (`:339-351`) must be kept on Linux |
| Screenshot | system preset `screenshot_interactive` (`system_actions.py:53-58`) | `explorer.exe ms-screenclip:` (`platform_actions.py:391-402`) | Same "area screenshot" action |
| Lock screen | `lock_screen` preset (`system_actions.py:41-46`) | `LockWorkStation()` (`platform_actions.py:383-389`) | Same |
| Terminal launch | `open_terminal` preset; injector/tool specific (`system_actions.py:47-52`) | `wt.exe`, fallback `start cmd` (`platform_actions.py:368-381`) | Same |
| Scripts | `bash_script` run via shell (`system_actions.py:470-487`) | `subprocess.Popen(script, shell=True)` (`platform_actions.py:481`) | Same action type, OS-appropriate shell |
| Tray | freedesktop StatusNotifierItem + `com.canonical.dbusmenu` over D-Bus (`linux-app/tray.py:5,30-31`) | `QSystemTrayIcon` + `QMenu` (`windows-app/tray.py:41-70`) | Native tray per OS; same 3 status lines + Open + Quit |
| Autostart | `.desktop` with `Exec=… --hidden` in user `~/.config/autostart` (`linux-app/install.sh:110-124`) and `/etc/xdg/autostart` for the DEB (`packaging/deb/build_deb.sh:71-83`) | Startup-folder `.lnk` with `--hidden` (`windows-app/install.ps1:50-57`) | Background start with no window |
| BLE stack | BlueZ D-Bus + `AcquireNotify` fd watch, `StartNotify` fallback (`linux-app/ble_bridge.py:299-352`) | `bleak` client with `start_notify` (`windows-app/ble_bridge.py:221-256`) | Reconnect with backoff: both use 3 s → 15 s (`linux-app/ble_bridge.py:51-54`, `windows-app/ble_bridge.py:33-34`) |
| Serial port discovery | glob `/dev/ttyUSB*`, `/dev/ttyACM*` (`linux-app/serial_listener.py:187`) | `serial.tools.list_ports`, scored so macropad-like ports come first (`windows-app/serial_listener.py:150-167`) | Probe with `WHO` before selecting |
| Single instance | not implemented | `QLocalServer`/`QLocalSocket` named `OctoPad-single-instance` (`windows-app/main.py:15,89-108`) | Decide: required on both (§7) |
| Action-list failure handling | continues with the next action (`linux-app/serial_listener.py:96-109`; no `break`) | stops at the first failed action (`windows-app/serial_listener.py:93-105`, `break` at `:105`) | Must pick one; see §7 |

---

## 5. Config, preset and profile file formats

### 5.1 On-disk paths

| Item | Linux | Windows |
| --- | --- | --- |
| Config | `~/.config/octopad/config.json` (`linux-app/config.py:21-22`) | `%APPDATA%\OctoPad\config.json` (`windows-app/config.py:16-22`) |
| User presets | `~/.config/octopad/presets/<slug>.json` (`linux-app/config.py:23,374-420`) | `%APPDATA%\OctoPad\presets\<slug>.json` (`windows-app/config.py:23,390-435`) |
| HA credentials | `~/.config/octopad/ha_config.json` (`linux-app/home_assistant.py:38`) | `%APPDATA%\OctoPad\ha_config.json` (`windows-app/home_assistant.py:23`) |
| HA entity cache | `~/.config/octopad/ha_entities_cache.json` (`linux-app/home_assistant.py:39`) | `%APPDATA%\OctoPad\ha_entities_cache.json` (`windows-app/home_assistant.py:24`) |
| HA legacy fallback | `~/.config/home-assistant/env` (`HA_URL=`, `HA_TOKEN=`) (`linux-app/home_assistant.py:37,254-263`) | *none* (`windows-app/home_assistant.py:196-224`) |
| BLE log | `~/.config/octopad/ble.log`, rotates at 256 KiB (`linux-app/ble_bridge.py:57-58,66-67`) | `%APPDATA%\OctoPad\ble.log` (`windows-app/ble_bridge.py:36-38`) |
| Saved profile (export) | anywhere the user picks, default name `octopad-profile-<date>.json` (`linux-app/ui/settings_page.py:220-245`) | same default name (`windows-app/ui/profiles_page.py:177-189`) |

Both files are written with `indent=2, ensure_ascii=False`
(`linux-app/config.py:360`, `windows-app/config.py:376`).

### 5.2 Config JSON shape

Identical in both apps (`linux-app/config.py:154-191`,
`windows-app/config.py:187-197`):

```
{
  "version": 2.6,
  "schemaVersion": 1,
  "device": "OctoPad",
  "debounceMs": 25, "clickTimeoutMs": 250, "holdTimeoutMs": 450,
  "activeModeIndex": 0,
  "activePreset": "<slug>" | absent,
  "modes": [
    { "id": "desktop", "name": "Desktop Mode", "ledState": true,
      "buttons": [ { "index": 0, "pin": 20, "label": "Mode / Passcode",
                     "single": <action|list>, "double": …, "hold": … }, × 8 ] },
    { "id": "ha", "name": "Home Assistant Mode", "ledState": false,
      "buttons": [ …× 8 ] }
  ]
}
```

- A trigger slot holds **either** one action object **or** a list of action
  objects; legacy single dicts are still accepted (`linux-app/config.py:237-246`).
- `<action>` always has a `type` string; the remaining keys depend on the type
  (see §3b / `default_action` at `linux-app/config.py:110-138`,
  `windows-app/config.py:116-138`).
- Pins: `[20, 9, 2, 1, 21, 10, 3, 0]` for B1…B8 (`linux-app/config.py:25`).
- Current schema version is `1` on both sides (`linux-app/config.py:28`,
  `windows-app/config.py:28`).

### 5.3 What a legacy importer must handle

1. **Both platforms' directories.** Importer must find the Linux paths and
   the `%APPDATA%\OctoPad` paths, each with config, presets, HA credentials
   and entity cache.
2. **Both profile shapes.** They are structurally identical today, but the
   loaders differ (see below), so the importer must be tolerant of what either
   loader accepts.
3. **Single-action form.** A trigger slot may be one dict (older profiles) or
   an array. Both must import.
4. **Missing keys.** `load_config` fills absent fields from defaults and only
   copies a fixed key list (`linux-app/config.py:313-352`,
   `windows-app/config.py:317-368`).
5. **Renamed/reworded preset payloads.** Built-in preset labels and a few
   `system_action` display names differ between the apps, but the **ids** are
   stable (`lock_screen`, `screenshot_interactive`, `volume_up_5`, …), so
   import must map by id and ignore the label.
6. **`activePreset` bookkeeping.** It is stored in the config but excluded from
   the dirty check (`linux-app/state.py:153-159`) and stripped from preset
   snapshots (`linux-app/state.py:311-312`); an unknown slug is reset to `None`
   on load (`linux-app/state.py:33-34`).
7. **Schema migration.** `migrate_config` currently only stamps
   `schemaVersion` and must be re-run by the new importer for
   `schemaVersion: 0` files (`linux-app/config.py:290-310`,
   `windows-app/config.py:294-314`).
8. **Re-runnable.** Per the design, the importer may be run again later
   (`2026-10-18-tauri-rewrite-design.md:112-119`); a re-run must not duplicate
   or clobber newer data.
9. **HA credential fallback.** If `ha_config.json` is missing, Linux reads
   `~/.config/home-assistant/env` (`HA_URL=` / `HA_TOKEN=`)
   (`linux-app/home_assistant.py:254-263`). The importer should offer the same
   fallback on both OSes.

---

## 6. Deliberately out of scope for v1

Taken from `2026-10-18-tauri-rewrite-design.md:48-55,86-87`:

- **macOS support** — Windows 10+ x64 and Linux only.
- **Auto-update** — deferred to a later release.
- **Code signing** — none available; ship the SmartScreen workaround docs.
- **Firmware changes** — `octopad.ino` is frozen; the app only speaks the
  existing command set (`WHO`, `INFO`, `SAVE`, `HELP`, `TIMING:`, `NAME:`,
  `MODE:`, `RESET`, `RESTART`).
- **Home Assistant WebSocket** — REST only in v1.
- **New UI features beyond parity** (design doc: "New UI features beyond
  parity").

---

## 7. Open questions / ambiguities

Concrete decisions needed before M4 sign-off. Each is a real difference
between the two apps or between a README and the code.

1. **Action-list failure handling.** Linux continues after a failed action
   (`linux-app/serial_listener.py:96-109`, no `break`); Windows stops at the
   first failure (`windows-app/serial_listener.py:105`). Which becomes the
   single behavior? (Continuing is friendlier for `delay` chains; stopping
   surfaces errors earlier.)
2. **Profiles page placement.** Windows has a dedicated page
   (`windows-app/ui/profiles_page.py`); Linux folds it into Settings
   (`linux-app/ui/settings_page.py:40-251`). The new app must pick one
   structure for both. The design doc only says the Windows profiles feature
   must exist on Linux (`…design.md:37`), not that it must be a separate page.
3. **Timing ranges disagree.** Linux: debounce 0–200 step 5, click 50–2000
   step 5, hold 100–3000 step 5 (`linux-app/ui/settings_page.py:270-279`).
   Windows: debounce 0–1000 step 5, click 50–2000 step 10, hold 50–3000
   step 10 (`windows-app/ui/settings_page.py:30-38`). Which range is
   authoritative? (The firmware default is 25/250/450 in both.)
4. **Preset labels differ while ids match.** "Ubuntu Productivity" vs
   "Windows Productivity" (`linux-app/config.py:230`,
   `windows-app/config.py:235`); "Interactive Screenshot (Area)" vs
   "Screenshot (Area)" (`linux-app/system_actions.py:55`,
   `windows-app/platform_actions.py:51`); "Lock Ubuntu Session" vs
   "Lock Windows Session" (`linux-app/system_actions.py:43`,
   `windows-app/platform_actions.py:47`). Pick OS-neutral labels once, or keep
   per-OS wording; either way ids stay stable for profile portability.
5. **`bash_script` naming and defaults.** Linux label "Bash Script" with a
   `notify-send` default (`linux-app/config.py:63,127`); Windows "Script /
   Command" with `msg *` (`windows-app/config.py:69,133`). Confirm the shared
   label and whether the default text should be empty for parity.
6. **`load_config` validation differs.** Windows coerces timings to `int`
   and resets an out-of-range `activeModeIndex`
   (`windows-app/config.py:358-366`); Linux does neither
   (`linux-app/config.py:327-352`). The importer/core should validate on both
   sides — otherwise a hand-edited config behaves differently per OS.
7. **`normalize_config` strictness differs.** Linux rejects an action list
   unless every element is a dict (`linux-app/config.py:283-286`); Windows
   accepts any list (`windows-app/config.py:288-290`). Import results would
   diverge; decide one rule.
8. **Single-instance on Linux.** Windows enforces one instance
   (`windows-app/main.py:89-108`); Linux does not. Is it a parity
   requirement? (If yes it affects how `--hidden` autostart and "Open
   Application" from the tray interact.)
9. **README vs. code: closing the window does not exit.** `README.md:159`
   and `linux-app/README.md:13` say the process shuts down cleanly when the
   window is closed, but both apps **hide to the tray** instead
   (`linux-app/ui/window.py:333-340`, `windows-app/ui/window.py:302-307`), and
   the Linux app calls `hold()` to stay alive (`linux-app/main.py:215-217`).
   The docs (or the behavior) should change.
10. **README vs. code: Windows Python version.** `README.md:185` and
   `windows-app/README.md:11` say Python 3.10+ / 3.9+ respectively, while
    `install.ps1:22` errors with "Python 3.10+ not found". Confirm the real
    floor (the Windows README's "3.9+" looks stale).
11. **README vs. code: Linux is a daemon-free app.** `linux-app/README.md:6`
    says it is "not a daemon that keeps running in the background", but the
    tray + `hold()` + autostart `--hidden` behavior means it *does* stay
    resident after the window closes. Decide the wording and the actual
    autostart default (the DEB installs a system-wide autostart entry —
    `packaging/deb/build_deb.sh:71-83`).
12. **Forced dark theme on Linux only.** Linux forces dark mode and loads a
    custom CSS design system (`linux-app/main.py:177-181`,
    `linux-app/ui/css.py`); Windows uses the default Qt palette. The new app
    has one design system (`…design.md:82`), but confirm dark-only vs.
    follow-system.
13. **`firmware_version` display.** Linux shows "firmware v…" in Settings
    (`linux-app/ui/settings_page.py:352-353`); Windows tracks the value but
    does not display it (`windows-app/main.py`, `windows-app/ui/window.py:284`).
    Confirm the new app shows name + firmware on both.
14. **Windows import auto-saves, Linux does not.** Windows `_on_import`
    calls `state.save()` (`windows-app/ui/profiles_page.py:174`); Linux only
    loads into the editor and asks the user to Save
    (`linux-app/ui/settings_page.py:217-218`). Pick one flow.
15. **First-run.** Neither app has onboarding; a first run silently uses
    defaults. Decide whether the new app needs any first-run state at all
    (the design doc does not require it).
16. **Legacy BLE UUIDs are mapped differently (possible bug).** For the
    pre-rebrand Nordic UART service, Linux treats `…0003` as the TX/notify
    characteristic and `…0002` as RX/write
    (`linux-app/ble_bridge.py:36-38`), which matches the NUS convention.
    Windows declares `LEGACY_TX_UUID = 6e400002…` — the *write* characteristic
    — and looks only for that one as its notify source
    (`windows-app/ble_bridge.py:28,258`). Check which is correct against real
    legacy firmware before freezing the new app's UUID table.

## 7b. Resolved decisions (2026-10-18, after owner review)

The 16 questions above are kept as history. These are the decided answers;
they are binding for the rewrite (M2 UI structure, M3/M4 acceptance).

| # | Decision | Rationale |
| --- | --- | --- |
| 1 | **A failed action does not stop the chain** (Linux behavior); each failure is written to the console as an error line. | Multi-action chains with `delay` are the reason lists exist; aborting silently on one failure is worse than surfacing the error. |
| 2 | **Profiles is a dedicated top-level page** (Windows structure): current profile, built-in + user presets, import/export. Settings keeps only connection/timing/theme. | Folding it into Settings (Linux) overcrowds one page; a dedicated page has room for the preset list and the import/export flow. |
| 3 | Shared timing ranges: **debounce 0–1000 step 5, click 50–2000 step 5, hold 50–3000 step 5**; firmware defaults stay 25/250/450. Out-of-range values in a file are **clamped with a warning**. | Union of both apps so an imported Windows config is never rejected; one step value keeps the slider feel identical. |
| 4 | **OS-neutral labels**, ids unchanged: "Productivity" preset, "Screenshot (Area)", "Lock Session". | One shared UI; ids stay stable so presets/profiles stay portable between OSes. |
| 5 | Action type label is **"Script"**. The default command text stays OS-specific (`notify-send` on Linux, `msg *` on Windows) — a documented, intentional exception. | The command is platform-specific by nature; a shared default would fail on one OS. |
| 6 | One validation path for both OSes: timings coerced/clamped per decision 3; **out-of-range `activeModeIndex` resets to 0** with a warning; load warnings surface in the console page. | Removes the "hand-edited config behaves differently per OS" divergence. |
| 7 | Import validation follows the **stricter Linux rule** (action-list items must be objects); the lenient runtime merge still accepts legacy single dicts. | Strict at the import boundary, tolerant for incremental edits in real files. |
| 8 | **Single instance on both OSes.** A second launch focuses the existing window; a `--hidden`/autostart launch signals the running instance instead of starting a second one. | Prevents two processes fighting over the same serial port / BLE session. |
| 9–11 | **Fix the documentation to match the code**: closing the window hides to the tray (the app keeps running); the Windows Python floor is 3.10+; the Linux README stops claiming the app is not a resident background app. Autostart stays on by default (as the deb does today) and gains an in-app toggle. | The behavior is intentional and useful; the docs were stale. |
| 12 | **Dark-only custom theme in v1** (as Linux does today), built on design tokens so a light/system-following variant can be added later without rework. | One identical design on both OSes; smallest M2 surface. |
| 13 | Device name **and** firmware version are shown on both OSes (Linux behavior), in the header/status area. | Windows tracked but never showed it; showing it is the more useful behavior. |
| 14 | Import **loads into the editor and requires an explicit Save** (Linux flow). The automatic first-run legacy import (M1) is the one exception: no user interaction. | No silent overwrite of the user's current configuration. |
| 15 | **No onboarding wizard.** First run uses defaults (or the auto-imported legacy config) and the Profiles page makes that state visible. | Neither app had onboarding; a wizard is new-feature scope. |
| 16 | Legacy NUS mapping follows **Linux/standard**: notify `6e400003…`, write `6e400002…`. The Windows Python mapping is a bug and is not replicated. Verify against real legacy hardware if one becomes available. | Matches the Nordic UART convention and the firmware's own layout. |
| 17 | **Platform priority: Linux first.** Ubuntu is the reference distribution; Debian-derived systems are called out explicitly because most installs live there; Windows keeps full parity and a CI artifact per milestone, but Linux packaging regressions block a release first. CI reflects this: a `linux` job (Ubuntu, `.deb` + AppImage) and a `debian` container job (proves the `.deb` installs on Debian 12) sit alongside the `windows` job. | The owner's actual user base is Linux; Windows is the secondary audience. Priority decides where verification effort goes. |
