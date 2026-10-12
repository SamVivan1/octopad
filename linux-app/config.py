#!/usr/bin/env python3
"""
OctoPad - Button configuration (default, preset, load/save).

Single source of truth for the native GTK4 application:
~/.config/octopad/config.json
"""

import copy
import json
import os
import re
from typing import Any, Dict, List, Optional, Tuple

CONFIG_DIR = os.path.expanduser("~/.config/octopad")
CONFIG_FILE = os.path.join(CONFIG_DIR, "config.json")
PRESET_DIR = os.path.join(CONFIG_DIR, "presets")

# Physical pinout for buttons B1..B8 (same as octopad.ino)
PINOUT = [20, 9, 2, 1, 21, 10, 3, 0]

TRIGGERS: List[Tuple[str, str]] = [
    ("single", "Single Click (1x)"),
    ("double", "Double Click (2x)"),
    ("hold", "Hold / Long Press"),
]

ACTION_TYPES: List[Tuple[str, str]] = [
    ("none", "None / Off"),
    ("shortcut", "Keyboard Shortcut"),
    ("text", "Text / Auto String"),
    ("media", "Media Control"),
    ("audio_output", "Audio Output (Switch)"),
    ("launch_app", "Launch Application"),
    ("system_action", "System Action (Preset)"),
    ("bash_script", "Bash Script"),
    ("home_assistant", "Home Assistant (Native)"),
    ("ha_webhook", "HA Webhook"),
    ("delay", "Delay / Wait"),
    ("mode_toggle", "Toggle Mode (Layer)"),
]

ACTION_TYPE_LABEL: Dict[str, str] = dict(ACTION_TYPES)

MODIFIERS = ["ctrl", "alt", "shift", "super"]

KEY_GROUPS: List[Tuple[str, List[str]]] = [
    ("Alphabet", list("ABCDEFGHIJKLMNOPQRSTUVWXYZ")),
    ("Numbers", list("0123456789")),
    ("Function Keys", [f"F{i}" for i in range(1, 25)]),
    ("Navigation and Control", [
        "ENTER", "ESC", "BACKSPACE", "TAB", "SPACE", "UP", "DOWN", "LEFT",
        "RIGHT", "HOME", "END", "PAGE_UP", "PAGE_DOWN", "DELETE",
    ]),
]

MEDIA_ACTIONS: List[Tuple[str, str]] = [
    ("MUTE", "Mute / Unmute"),
    ("VOL_UP", "Volume Up (+)"),
    ("VOL_DOWN", "Volume Down (-)"),
    ("PLAY_PAUSE", "Play / Pause"),
    ("NEXT_TRACK", "Next Track"),
    ("PREV_TRACK", "Previous Track"),
]

MEDIA_ACTION_LABEL: Dict[str, str] = dict(MEDIA_ACTIONS)

# Hardware button labels (B1..B8)
DEFAULT_LABELS = [
    "Mode / Passcode",
    "Terminal",
    "Screenshot",
    "Mute Mic",
    "Vol Up",
    "Vol Down",
    "Lock Screen",
    "Macro 7",
]

HA_KEY_LABELS = [f"HA Key {i + 1}" for i in range(7)]


def default_action(action_type: str) -> Dict[str, Any]:
    """Default action for a type — with no hardcoded data (e.g. HA entity)."""
    if action_type == "shortcut":
        return {"type": "shortcut", "modifiers": ["ctrl", "alt", "shift"], "key": "A"}
    if action_type == "text":
        return {"type": "text", "text": "Hello World"}
    if action_type == "media":
        return {"type": "media", "mediaKey": "VOL_UP"}
    if action_type == "audio_output":
        # User picks the devices to cycle through via the output picker.
        return {"type": "audio_output", "sinks": []}
    if action_type == "launch_app":
        return {"type": "launch_app", "desktopId": "code.desktop", "appName": "Visual Studio Code"}
    if action_type == "system_action":
        return {"type": "system_action", "presetId": "mic_toggle_mute",
                "presetName": "Toggle Microphone Mute"}
    if action_type == "bash_script":
        return {"type": "bash_script", "script": 'notify-send "OctoPad" "Action!"'}
    if action_type == "home_assistant":
        # No built-in entity: the user selects one via the Entity Picker.
        return {"type": "home_assistant", "domain": "", "service": "",
                "entityId": "", "friendlyName": ""}
    if action_type == "ha_webhook":
        return {"type": "ha_webhook", "url": ""}
    if action_type == "delay":
        return {"type": "delay", "seconds": 0.5}
    if action_type == "mode_toggle":
        return {"type": "mode_toggle"}
    return {"type": "none"}


def _button(index: int, label: str, single: Dict[str, Any],
            double: Dict[str, Any] | None = None,
            hold: Dict[str, Any] | None = None) -> Dict[str, Any]:
    return {
        "index": index,
        "pin": PINOUT[index],
        "label": label,
        "single": single,
        "double": double or {"type": "none"},
        "hold": hold or {"type": "none"},
    }


def default_config() -> Dict[str, Any]:
    """Default configuration. HA mode intentionally has no entities — the user picks them."""
    desktop_buttons = [
        _button(0, DEFAULT_LABELS[0], {"type": "mode_toggle"}, hold={"type": "text", "text": "031004"}),
        _button(1, DEFAULT_LABELS[1], {"type": "system_action", "presetId": "open_terminal",
                                       "presetName": "Open New Terminal"}),
        _button(2, DEFAULT_LABELS[2], {"type": "system_action", "presetId": "screenshot_interactive",
                                       "presetName": "Interactive Screenshot (Area)"}),
        _button(3, DEFAULT_LABELS[3], {"type": "system_action", "presetId": "mic_toggle_mute",
                                       "presetName": "Toggle Microphone Mute"}),
        _button(4, DEFAULT_LABELS[4], {"type": "system_action", "presetId": "volume_up_5",
                                       "presetName": "Volume Up (+5%)"}),
        _button(5, DEFAULT_LABELS[5], {"type": "system_action", "presetId": "volume_down_5",
                                       "presetName": "Volume Down (-5%)"}),
        _button(6, DEFAULT_LABELS[6], {"type": "system_action", "presetId": "lock_screen",
                                       "presetName": "Lock Ubuntu Session"}),
        _button(7, DEFAULT_LABELS[7],
                {"type": "shortcut", "modifiers": ["ctrl", "alt", "shift"], "key": "8"},
                {"type": "shortcut", "modifiers": ["ctrl", "alt", "shift"], "key": "U"},
                {"type": "shortcut", "modifiers": ["ctrl", "alt", "shift"], "key": "J"}),
    ]
    ha_buttons = [
        _button(0, DEFAULT_LABELS[0], {"type": "mode_toggle"}, hold={"type": "text", "text": "031004"}),
    ]
    for i, label in enumerate(HA_KEY_LABELS):
        ha_buttons.append(_button(i + 1, label, default_action("home_assistant")))

    return {
        "version": 2.5,
        "device": "OctoPad",
        "debounceMs": 25,
        "clickTimeoutMs": 250,
        "holdTimeoutMs": 450,
        "activeModeIndex": 0,
        "modes": [
            {"id": "desktop", "name": "Desktop Mode", "ledState": True, "buttons": desktop_buttons},
            {"id": "ha", "name": "Home Assistant Mode", "ledState": False, "buttons": ha_buttons},
        ],
    }


def apply_preset(key: str) -> Dict[str, Any]:
    """Build a configuration from a built-in preset ('default' | 'productivity' | 'media')."""
    config = default_config()
    if key == "productivity":
        btns = config["modes"][0]["buttons"]
        btns[1]["label"] = "Terminal"
        btns[1]["single"] = {"type": "system_action", "presetId": "open_terminal",
                             "presetName": "Open New Terminal"}
        btns[2]["label"] = "Screenshot"
        btns[2]["single"] = {"type": "system_action", "presetId": "screenshot_interactive",
                             "presetName": "Interactive Screenshot (Area)"}
        btns[3]["label"] = "Lock Screen"
        btns[3]["single"] = {"type": "system_action", "presetId": "lock_screen",
                             "presetName": "Lock Ubuntu Session"}
        btns[4]["label"] = "VS Code"
        btns[4]["single"] = {"type": "launch_app", "desktopId": "code.desktop",
                             "appName": "Visual Studio Code"}
    elif key == "media":
        btns = config["modes"][0]["buttons"]
        btns[1]["label"] = "Play / Pause"
        btns[1]["single"] = {"type": "media", "mediaKey": "PLAY_PAUSE"}
        btns[2]["label"] = "Vol Down"
        btns[2]["single"] = {"type": "system_action", "presetId": "volume_down_5",
                             "presetName": "Volume Down (-5%)"}
        btns[3]["label"] = "Vol Up"
        btns[3]["single"] = {"type": "system_action", "presetId": "volume_up_5",
                             "presetName": "Volume Up (+5%)"}
        btns[4]["label"] = "Mute"
        btns[4]["single"] = {"type": "system_action", "presetId": "volume_toggle_mute",
                             "presetName": "Toggle Speaker Mute"}
    return config


PRESET_ITEMS: List[Tuple[str, str, str]] = [
    ("default", "Default (Desktop and Native HA)",
     "Factory default: system buttons + empty HA slots to fill in yourself."),
    ("productivity", "Ubuntu Productivity",
     "Terminal, screenshot, lock screen, and the VS Code launcher."),
    ("media", "Media and Streaming",
     "Play/pause, volume, and mute for music control."),
]


def actions_list(raw: Any) -> List[Dict[str, Any]]:
    """Normalize a trigger slot into a list of actions.

    A single trigger may now hold multiple actions (legacy dicts still accepted).
    """
    if isinstance(raw, dict):
        return [raw]
    if isinstance(raw, list):
        return [action for action in raw if isinstance(action, dict)]
    return []


def _merge_button(base: Dict[str, Any], template: Dict[str, Any]) -> Dict[str, Any]:
    merged = dict(template)
    merged.update({k: v for k, v in base.items() if k in ("index", "pin", "label")})
    for trig in ("single", "double", "hold"):
        action = base.get(trig)
        if isinstance(action, list):
            clean = [a for a in action if isinstance(a, dict)]
            merged[trig] = clean or [{"type": "none"}]
        elif isinstance(action, dict) and action.get("type"):
            merged[trig] = action
        else:
            merged[trig] = template.get(trig, {"type": "none"})
    return merged


def normalize_config(data: Any) -> Tuple[bool, str]:
    """Validate the structure of an imported profile. Returns (ok, message)."""
    if not isinstance(data, dict):
        return False, "Not a JSON object."
    modes = data.get("modes")
    if not isinstance(modes, list) or len(modes) < 2:
        return False, "Invalid 'modes' field (2 modes required)."
    for mode in modes:
        if not isinstance(mode, dict) or not isinstance(mode.get("buttons"), list):
            return False, "Invalid 'modes[].buttons' structure."
        for button in mode["buttons"]:
            if not isinstance(button, dict):
                return False, "Invalid button entry."
            for trig in ("single", "double", "hold"):
                action = button.get(trig)
                if action is None:
                    continue
                if isinstance(action, dict):
                    continue
                if (isinstance(action, list)
                        and all(isinstance(a, dict) for a in action)):
                    continue
                return False, f"Action '{trig}' is invalid."
    return True, "OK"


def load_config() -> Dict[str, Any]:
    """Load the user configuration, filling in any missing default fields."""
    config = default_config()
    if not os.path.exists(CONFIG_FILE):
        return config
    try:
        with open(CONFIG_FILE, "r", encoding="utf-8") as handle:
            stored = json.load(handle)
    except Exception as exc:
        print(f"[CONFIG] Failed to read {CONFIG_FILE}: {exc}")
        return config
    if not isinstance(stored, dict):
        return config

    for key in ("version", "device", "debounceMs", "clickTimeoutMs", "holdTimeoutMs",
                "activeModeIndex", "activePreset"):
        if key in stored:
            config[key] = stored[key]

    stored_modes = stored.get("modes")
    if isinstance(stored_modes, list) and len(stored_modes) >= 2:
        for slot, stored_mode in enumerate(stored_modes[:2]):
            if not isinstance(stored_mode, dict):
                continue
            mode = config["modes"][slot]
            for key in ("id", "name", "ledState"):
                if key in stored_mode:
                    mode[key] = stored_mode[key]
            stored_buttons = stored_mode.get("buttons")
            if isinstance(stored_buttons, list):
                buttons = []
                for index in range(8):
                    template = mode["buttons"][index]
                    raw = stored_buttons[index] if index < len(stored_buttons) else None
                    if isinstance(raw, dict):
                        buttons.append(_merge_button(raw, template))
                    else:
                        buttons.append(copy.deepcopy(template))
                mode["buttons"] = buttons
    return config


def save_config(config: Dict[str, Any]) -> bool:
    """Save the configuration to disk (used by the listener & next application run)."""
    try:
        os.makedirs(CONFIG_DIR, exist_ok=True)
        with open(CONFIG_FILE, "w", encoding="utf-8") as handle:
            json.dump(config, handle, indent=2, ensure_ascii=False)
        return True
    except Exception as exc:
        print(f"[CONFIG] Failed to write {CONFIG_FILE}: {exc}")
        return False


def get_button(config: Dict[str, Any], mode_index: int, index: int) -> Dict[str, Any]:
    return config["modes"][mode_index]["buttons"][index]


# ---------------------------------------------------------------------------
# User presets (configuration snapshots that can be created/overwritten from the UI)
# ---------------------------------------------------------------------------
def preset_slug(name: str) -> str:
    """Build a safe slug for a preset name (used as the file name)."""
    cleaned = "".join(
        ch if (ch.isalnum() or ch in " _-") else "-"
        for ch in name.strip().lower()
    )
    slug = re.sub(r"[-_ ]+", "-", cleaned).strip("-")
    return slug or "preset"


def list_presets() -> List[str]:
    """Names of saved user presets (alphabetical order)."""
    if not os.path.isdir(PRESET_DIR):
        return []
    names = []
    for filename in sorted(os.listdir(PRESET_DIR)):
        if filename.endswith(".json"):
            names.append(os.path.splitext(filename)[0])
    return names


def preset_file(name: str) -> str:
    return os.path.join(PRESET_DIR, f"{preset_slug(name)}.json")


def save_preset(name: str, data: Dict[str, Any]) -> bool:
    """Save a preset snapshot to the user preset directory."""
    try:
        os.makedirs(PRESET_DIR, exist_ok=True)
        with open(preset_file(name), "w", encoding="utf-8") as handle:
            json.dump(data, handle, indent=2, ensure_ascii=False)
        return True
    except Exception as exc:
        print(f"[CONFIG] Failed to write preset '{name}': {exc}")
        return False


def load_preset(name: str) -> Optional[Dict[str, Any]]:
    """Load a user preset snapshot; None if it does not exist."""
    try:
        with open(preset_file(name), "r", encoding="utf-8") as handle:
            data = json.load(handle)
    except Exception as exc:
        print(f"[CONFIG] Failed to read preset '{name}': {exc}")
        return None
    return data if isinstance(data, dict) else None


def delete_preset(name: str) -> bool:
    """Delete a preset file. Returns False if it did not exist."""
    try:
        path = preset_file(name)
        if os.path.exists(path):
            os.remove(path)
            return True
        return False
    except Exception as exc:
        print(f"[CONFIG] Failed to delete preset '{name}': {exc}")
        return False


def action_summary(action: Any) -> str:
    """Short summary for a chip/in the inspector (single action or list)."""
    actions = actions_list(action)
    if not actions:
        return "None"
    if len(actions) == 1:
        return _action_summary_single(actions[0])
    return f"{_action_summary_single(actions[0])} (+{len(actions) - 1})"


def _action_summary_single(action: Dict[str, Any]) -> str:
    """Short summary of a single action."""
    if not isinstance(action, dict):
        return "None"
    kind = action.get("type", "none")
    if kind == "none":
        return "None"
    if kind == "shortcut":
        mods = "+".join(m.upper() for m in action.get("modifiers", []))
        key = action.get("key", "")
        return f"{mods}+{key}" if mods else key
    if kind == "text":
        return f"Text: {action.get('text', '')}"
    if kind == "media":
        return f"Media: {MEDIA_ACTION_LABEL.get(action.get('mediaKey'), action.get('mediaKey'))}"
    if kind == "launch_app":
        return f"App: {action.get('appName') or action.get('desktopId') or '-'}"
    if kind == "system_action":
        return f"System: {action.get('presetName') or action.get('presetId') or '-'}"
    if kind == "bash_script":
        script = (action.get("script") or "").strip().splitlines()
        return f"Script: {script[0] if script else '-'}"
    if kind == "home_assistant":
        name = action.get("friendlyName") or action.get("entityId")
        if not name:
            return "HA: no entity selected yet"
        return f"HA: {name} → {action.get('service', '-')}"
    if kind == "ha_webhook":
        return f"Webhook: {action.get('url') or '-'}"
    if kind == "audio_output":
        sinks = [s for s in (action.get("sinks") or []) if isinstance(s, dict)]
        names = [s.get("description") or s.get("name", "") for s in sinks]
        if not names:
            return "Audio: no output selected"
        return "Audio: " + " → ".join(names)
    if kind == "delay":
        return f"Delay {_format_seconds(action.get('seconds', 0.5))}"
    if kind == "mode_toggle":
        return "Toggle mode layer"
    return kind


def _format_seconds(value: Any) -> str:
    """Render a delay in seconds compactly (e.g. 1.5s, 2s, 500ms)."""
    try:
        seconds = float(value)
    except (TypeError, ValueError):
        seconds = 0.0
    if seconds < 1.0:
        return f"{int(round(seconds * 1000))}ms"
    return f"{seconds:g}s"
