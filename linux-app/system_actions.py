#!/usr/bin/env python3
"""
OctoPad - Ubuntu System Actions & Automation Engine
Handles audio controls, desktop window management, screenshots, bash scripts, and Home Assistant webhooks.
"""

import subprocess
import shutil
import time
import urllib.request
import urllib.parse
import json
from typing import Dict, Any, Tuple, List, Optional

# List of predefined interactive actions that require no coding
SYSTEM_ACTION_PRESETS = [
    {
        "id": "mic_toggle_mute",
        "name": "Toggle Microphone Mute",
        "category": "Audio",
        "description": "Mute/Unmute the current microphone (Meeting/Discord)"
    },
    {
        "id": "volume_up_5",
        "name": "Volume Up (+5%)",
        "category": "Audio",
        "description": "Raise the output volume by 5%"
    },
    {
        "id": "volume_down_5",
        "name": "Volume Down (-5%)",
        "category": "Audio",
        "description": "Lower the output volume by 5%"
    },
    {
        "id": "volume_toggle_mute",
        "name": "Toggle Speaker Mute",
        "category": "Audio",
        "description": "Mute/Unmute speaker audio output"
    },
    {
        "id": "lock_screen",
        "name": "Lock Ubuntu Session",
        "category": "System",
        "description": "Lock the Ubuntu desktop session instantly"
    },
    {
        "id": "open_terminal",
        "name": "Open New Terminal",
        "category": "System",
        "description": "Open a new terminal window"
    },
    {
        "id": "screenshot_interactive",
        "name": "Interactive Screenshot (Area)",
        "category": "System",
        "description": "Take a screenshot of a selected screen area"
    },
    {
        "id": "media_play_pause",
        "name": "Media Play / Pause",
        "category": "Media",
        "description": "Play or pause the music player / YouTube via playerctl"
    },
    {
        "id": "media_next",
        "name": "Media Next Track",
        "category": "Media",
        "description": "Next track via playerctl"
    },
    {
        "id": "media_prev",
        "name": "Media Previous Track",
        "category": "Media",
        "description": "Previous track via playerctl"
    }
]

# ---------------------------------------------------------------------------
# Keyboard & media simulation (used by the shortcut / text / media actions)
# ---------------------------------------------------------------------------
# GNOME Wayland does not allow xdotool, so the preference order is:
# wtype (Wayland) -> ydotool (uinput) -> xdotool (X11).
KEY_NAME_MAP = {
    "ENTER": "Return", "ESC": "Escape", "BACKSPACE": "BackSpace", "TAB": "Tab",
    "SPACE": "space", "UP": "Up", "DOWN": "Down", "LEFT": "Left", "RIGHT": "Right",
    "HOME": "Home", "END": "End", "PAGE_UP": "Page_Up", "PAGE_DOWN": "Page_Down",
    "DELETE": "Delete",
}


def _injector() -> str:
    for candidate in ("wtype", "ydotool", "xdotool"):
        if shutil.which(candidate):
            return candidate
    return ""


def _injector_help() -> str:
    return ("Keyboard simulation requires one of: `wtype` (Wayland, "
            "sudo apt install wtype), `ydotool` (recommended on GNOME Wayland: "
            "sudo apt install ydotool then run the daemon `sudo ydotoold` "
            "or `systemctl --user start ydotool`), or `xdotool` (X11).")


def _xkey(key: str) -> str:
    return KEY_NAME_MAP.get(key, key.lower() if len(key) == 1 else key)


# ---------------------------------------------------------------------------
# ydotool uses Linux/evdev keycodes (not key names), e.g. 29:1 = press Ctrl
# ---------------------------------------------------------------------------
_EVDEV_MODS = {"ctrl": 29, "alt": 56, "shift": 42, "super": 125}
_EVDEV_KEYS = {
    "return": 28, "escape": 1, "backspace": 14, "tab": 15, "space": 57,
    "up": 103, "down": 108, "left": 105, "right": 106,
    "home": 102, "end": 107, "page_up": 104, "page_down": 109, "delete": 111,
    "xf86audioplay": 207, "xf86audionext": 163, "xf86audioprev": 165,
}


def _evdev_key(name: str) -> Optional[int]:
    lower = (name or "").lower()
    if lower in _EVDEV_KEYS:
        return _EVDEV_KEYS[lower]
    if lower.startswith("f") and lower[1:].isdigit():
        num = int(lower[1:])
        if 1 <= num <= 24:
            return 58 + num  # F1=59
    if len(lower) == 1:
        code = ord(lower)
        if "a" <= lower <= "z":
            return code - 96 + 29  # a=30
        if lower == "0":
            return 11
        if "1" <= lower <= "9":
            return code - 48 + 1  # 1=2
    return None


def _ydotool_combo(mods: List[str], key: str) -> List[str]:
    """Build the `ydotool key` arguments (press then release every key)."""
    codes = []
    for mod in mods:
        if mod in _EVDEV_MODS:
            codes.append(_EVDEV_MODS[mod])
    key_code = _evdev_key(key)
    if key_code is not None:
        codes.append(key_code)
    elif len(codes) == 0:
        return []
    args = [f"{code}:1" for code in codes]
    args += [f"{code}:0" for code in reversed(codes)]
    return args


def _run(cmd: List[str]) -> Tuple[bool, str]:
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=6)
        if result.returncode == 0:
            return True, " ".join(cmd[:3])
        detail = (result.stderr or result.stdout or "").strip().splitlines()
        return False, detail[0] if detail else f"{cmd[0]} exit {result.returncode}"
    except FileNotFoundError:
        return False, f"{cmd[0]} not found"
    except Exception as exc:
        return False, str(exc)


def _send_text(text: str) -> Tuple[bool, str]:
    tool = _injector()
    if not tool:
        return False, _injector_help()
    if tool == "wtype":
        return _run(["wtype", "--", text])
    if tool == "ydotool":
        return _run(["ydotool", "type", "--", text])
    return _run(["xdotool", "type", "--clearmodifiers", "--", text])


def _send_shortcut(modifiers: List[str], key: str) -> Tuple[bool, str]:
    tool = _injector()
    if not tool:
        return False, _injector_help()
    mods = [m for m in MODIFIER_ORDER if m in (modifiers or [])]
    xkey = _xkey(key or "")
    if not xkey:
        return False, "Target key not selected yet"

    if tool == "wtype":
        cmd = ["wtype"]
        for mod in mods:
            cmd += ["-M", mod]
        cmd += ["-P", xkey, "-p", xkey]
        for mod in reversed(mods):
            cmd += ["-m", mod]
        return _run(cmd)

    if tool == "ydotool":
        args = _ydotool_combo(mods, xkey)
        if not args:
            return False, f"Key '{key}' not recognized by ydotool"
        return _run(["ydotool", "key", *args])

    combo = "+".join(mods + [xkey])
    return _run(["xdotool", "key", "--clearmodifiers", combo])


MODIFIER_ORDER = ["ctrl", "alt", "shift", "super"]

MEDIA_XF86 = {
    "PLAY_PAUSE": "XF86AudioPlay",
    "NEXT_TRACK": "XF86AudioNext",
    "PREV_TRACK": "XF86AudioPrev",
}


def _send_media(media_key: str) -> Tuple[bool, str]:
    """Media control: wpctl/pactl for volume, playerctl for the music player."""
    if media_key in ("VOL_UP", "VOL_DOWN", "MUTE"):
        step = "5%+" if media_key == "VOL_UP" else ("5%-" if media_key == "VOL_DOWN" else None)
        if shutil.which("wpctl"):
            if step:
                return _run(["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", step])
            return _run(["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
        if shutil.which("pactl"):
            if step:
                return _run(["pactl", "set-sink-volume", "@DEFAULT_AUDIO_SINK@",
                             "+5%" if media_key == "VOL_UP" else "-5%"])
            return _run(["pactl", "set-sink-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])

    if media_key in ("PLAY_PAUSE", "NEXT_TRACK", "PREV_TRACK"):
        if shutil.which("playerctl"):
            command = {"PLAY_PAUSE": "play-pause", "NEXT_TRACK": "next",
                       "PREV_TRACK": "previous"}[media_key]
            return _run(["playerctl", command])

    # Fallback: XF86 media keys via the keyboard simulator
    xf86 = MEDIA_XF86.get(media_key)
    if xf86:
        tool = _injector()
        if tool == "wtype":
            return _run(["wtype", "-P", xf86, "-p", xf86])
        if tool == "xdotool":
            return _run(["xdotool", "key", xf86])
        if tool == "ydotool":
            args = _ydotool_combo([], xf86)
            if args:
                return _run(["ydotool", "key", *args])
            return False, f"Media key '{media_key}' not recognized by ydotool"
    if media_key in ("VOL_UP", "VOL_DOWN", "MUTE"):
        return False, "Volume requires wpctl or pactl (pipewire/pulseaudio)."
    return False, "The music player requires playerctl (sudo apt install playerctl)."


# ---------------------------------------------------------------------------
# Audio output switching (PipeWire / PulseAudio via pactl)
# ---------------------------------------------------------------------------
def _capture(cmd: List[str], timeout: int = 5) -> Tuple[bool, str]:
    """Run a command and return (ok, stdout-or-error)."""
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        if result.returncode == 0:
            return True, result.stdout
        detail = (result.stderr or result.stdout or "").strip().splitlines()
        return False, detail[0] if detail else f"{cmd[0]} exit {result.returncode}"
    except FileNotFoundError:
        return False, f"{cmd[0]} not found"
    except Exception as exc:
        return False, str(exc)


def list_audio_sinks() -> List[Dict[str, str]]:
    """Available audio output devices as [{"name", "description"}].

    Uses `pactl` (PipeWire/PulseAudio); returns an empty list when unavailable.
    """
    if not shutil.which("pactl"):
        return []
    ok, out = _capture(["pactl", "-f", "json", "list", "sinks"])
    if ok and out.strip():
        try:
            data = json.loads(out)
            sinks: List[Dict[str, str]] = []
            for sink in data:
                name = sink.get("name", "")
                if not name:
                    continue
                sinks.append({
                    "name": name,
                    "description": sink.get("description") or name,
                })
            return sinks
        except (ValueError, TypeError, AttributeError):
            pass
    # Fallback: parse the plain-text listing.
    ok, out = _capture(["pactl", "list", "sinks"])
    if not ok:
        return []
    sinks = []
    name = desc = ""
    for line in out.splitlines():
        stripped = line.strip()
        if stripped.startswith("Name:") and not name:
            name = stripped.split(":", 1)[1].strip()
        elif stripped.startswith("Description:") and not desc:
            desc = stripped.split(":", 1)[1].strip()
        elif stripped == "" and name:
            sinks.append({"name": name, "description": desc or name})
            name = desc = ""
    if name:
        sinks.append({"name": name, "description": desc or name})
    return sinks


def default_audio_sink() -> str:
    """Name of the current default output sink ('' if it cannot be read)."""
    if shutil.which("pactl"):
        ok, out = _capture(["pactl", "get-default-sink"])
        if ok:
            return out.strip()
    if shutil.which("wpctl"):
        ok, out = _capture(["wpctl", "inspect", "@DEFAULT_AUDIO_SINK@"])
        if ok:
            for line in out.splitlines():
                stripped = line.strip()
                if stripped.startswith("node.name"):
                    return stripped.split("=", 1)[1].strip().strip('"')
    return ""


def _set_default_audio_sink(name: str) -> Tuple[bool, str]:
    if shutil.which("pactl"):
        ok, out = _capture(["pactl", "set-default-sink", name])
        return (True, f"Default output set to {name}") if ok else (False, out)
    return False, "pactl is required to switch the audio output"


def _move_streams_to_sink(name: str) -> None:
    """Move currently playing streams to the new sink (best effort)."""
    if not shutil.which("pactl"):
        return
    ok, out = _capture(["pactl", "list", "short", "sink-inputs"])
    if not ok:
        return
    for line in out.splitlines():
        parts = line.split("\t")
        if parts and parts[0].strip().isdigit():
            subprocess.run(["pactl", "move-sink-input", parts[0].strip(), name],
                           check=False)


def _switch_audio_output(action_data: Dict[str, Any]) -> Tuple[bool, str]:
    """Cycle the default output to the next user-selected sink."""
    sinks = [s for s in (action_data.get("sinks") or [])
             if isinstance(s, dict) and s.get("name")]
    names = [s["name"] for s in sinks]
    if not names:
        return False, "No audio output selected"
    try:
        index = names.index(default_audio_sink())
    except ValueError:
        index = -1
    target = names[(index + 1) % len(names)]
    ok, message = _set_default_audio_sink(target)
    if not ok:
        return False, message
    _move_streams_to_sink(target)
    label = next((s.get("description") or s["name"]
                  for s in sinks if s["name"] == target), target)
    return True, f"Audio output: {label}"


def execute_action(action_type: str, action_data: Dict[str, Any]) -> Tuple[bool, str]:
    """Execute a configured action based on its type and payload."""
    try:
        # 0. Toggle the Desktop <-> Home Assistant layer (handled by firmware via LED)
        if action_type == 'mode_toggle':
            return True, "Mode layer is controlled by the macropad firmware (LED)"

        # 0a. Delay / wait — pauses the action sequence for N seconds.
        elif action_type == 'delay':
            try:
                seconds = float(action_data.get("seconds", 0.5))
            except (TypeError, ValueError):
                return False, "Delay needs a valid duration."
            seconds = max(0.0, min(seconds, 600.0))
            time.sleep(seconds)
            return True, f"Waited {seconds:g}s."

        # 0b. Keyboard / text / media simulation
        elif action_type == 'shortcut':
            return _send_shortcut(action_data.get('modifiers', []), action_data.get('key', ''))

        elif action_type == 'text':
            text = action_data.get('text', '')
            if not text:
                return False, "Empty text"
            return _send_text(text)

        elif action_type == 'media':
            return _send_media(action_data.get('mediaKey', ''))

        elif action_type == 'audio_output':
            return _switch_audio_output(action_data)

        # 1. Launch native desktop application
        elif action_type == 'launch_app':
            from app_scanner import launch_app
            desktop_id = action_data.get('desktopId') or action_data.get('path')
            return launch_app(desktop_id)

        # 2. Preset system actions (Mic, Audio, Lock, etc.)
        elif action_type == 'system_action':
            preset_id = action_data.get('presetId')
            return _execute_system_preset(preset_id)

        # 3. Custom Bash Script execution
        elif action_type == 'bash_script':
            script_text = action_data.get('script', '').strip()
            if not script_text:
                return False, "Script content is empty"
            subprocess.Popen(script_text, shell=True, executable='/bin/bash', start_new_session=True)
            return True, "Bash script dispatched in background"

        # 4. Home Assistant Webhook / REST Call
        elif action_type == 'ha_webhook':
            url = action_data.get('url', '').strip()
            payload = action_data.get('payload', {})
            if not url:
                return False, "Home Assistant Webhook URL is empty"
            
            data_bytes = json.dumps(payload).encode('utf-8') if payload else b'{}'
            req = urllib.request.Request(
                url,
                data=data_bytes,
                headers={'Content-Type': 'application/json'},
                method='POST'
            )
            with urllib.request.urlopen(req, timeout=4) as response:
                status = response.status
            return True, f"Webhook sent to Home Assistant (HTTP {status})"

        # 5. Native Home Assistant Service Call (No Bash scripts needed!)
        elif action_type in ['home_assistant', 'ha_service']:
            from home_assistant import ha_client
            domain = action_data.get('domain', '')
            service = action_data.get('service', 'toggle')
            entity_id = action_data.get('entityId') or action_data.get('entity_id', '')
            data = action_data.get('data') or {}

            if not domain and '.' in entity_id:
                domain = entity_id.split('.')[0]

            if not domain or not service:
                return False, "Home Assistant domain or service not set"

            return ha_client.call_service(domain, service, entity_id, data)

        return False, f"Unknown action type: {action_type}"

    except Exception as e:
        return False, f"Action execution error: {str(e)}"

def _execute_system_preset(preset_id: str) -> Tuple[bool, str]:
    """Execute built-in hardware & OS automation commands."""
    has_wpctl = bool(shutil.which('wpctl'))
    has_pactl = bool(shutil.which('pactl'))
    has_playerctl = bool(shutil.which('playerctl'))

    # Microphone Mute Toggle
    if preset_id == 'mic_toggle_mute':
        if has_wpctl:
            subprocess.run(['wpctl', 'set-mute', '@DEFAULT_AUDIO_SOURCE@', 'toggle'], check=False)
            return True, "Toggled mic mute via wpctl"
        elif has_pactl:
            subprocess.run(['pactl', 'set-source-mute', '@DEFAULT_SOURCE@', 'toggle'], check=False)
            return True, "Toggled mic mute via pactl"
        else:
            subprocess.run(['amixer', 'set', 'Capture', 'toggle'], check=False)
            return True, "Toggled mic mute via amixer"

    # Speaker Mute Toggle
    elif preset_id == 'volume_toggle_mute':
        if has_wpctl:
            subprocess.run(['wpctl', 'set-mute', '@DEFAULT_AUDIO_SINK@', 'toggle'], check=False)
            return True, "Toggled speaker mute via wpctl"
        elif has_pactl:
            subprocess.run(['pactl', 'set-sink-mute', '@DEFAULT_SINK@', 'toggle'], check=False)
            return True, "Toggled speaker mute via pactl"

    # Volume Up 5%
    elif preset_id == 'volume_up_5':
        if has_wpctl:
            subprocess.run(['wpctl', 'set-volume', '@DEFAULT_AUDIO_SINK@', '5%+'], check=False)
            return True, "Volume +5% via wpctl"
        elif has_pactl:
            subprocess.run(['pactl', 'set-sink-volume', '@DEFAULT_SINK@', '+5%'], check=False)
            return True, "Volume +5% via pactl"

    # Volume Down 5%
    elif preset_id == 'volume_down_5':
        if has_wpctl:
            subprocess.run(['wpctl', 'set-volume', '@DEFAULT_AUDIO_SINK@', '5%-'], check=False)
            return True, "Volume -5% via wpctl"
        elif has_pactl:
            subprocess.run(['pactl', 'set-sink-volume', '@DEFAULT_SINK@', '-5%'], check=False)
            return True, "Volume -5% via pactl"

    # Lock Screen
    elif preset_id == 'lock_screen':
        if shutil.which('loginctl'):
            subprocess.run(['loginctl', 'lock-session'], check=False)
            return True, "Locked screen via loginctl"
        elif shutil.which('xdg-screensaver'):
            subprocess.run(['xdg-screensaver', 'lock'], check=False)
            return True, "Locked screen via xdg-screensaver"

    # Open Terminal
    elif preset_id == 'open_terminal':
        terms = ['gnome-terminal', 'x-terminal-emulator', 'alacritty', 'kitty', 'konsole', 'xfce4-terminal']
        for t in terms:
            if shutil.which(t):
                subprocess.Popen([t], start_new_session=True)
                return True, f"Opened terminal: {t}"

    # Screenshot
    elif preset_id == 'screenshot_interactive':
        if shutil.which('gnome-screenshot'):
            subprocess.Popen(['gnome-screenshot', '-a'], start_new_session=True)
            return True, "Triggered interactive screenshot"
        else:
            # Trigger via keyboard simulation or notify
            subprocess.Popen(['notify-send', 'OctoPad', 'Use PrintScreen to take a screenshot'], start_new_session=True)
            return True, "Screenshot requested"

    # Media Controls via playerctl
    elif preset_id in ['media_play_pause', 'media_next', 'media_prev']:
        if has_playerctl:
            cmd = 'play-pause' if preset_id == 'media_play_pause' else ('next' if preset_id == 'media_next' else 'previous')
            subprocess.run(['playerctl', cmd], check=False)
            return True, f"playerctl {cmd}"
        else:
            return False, "playerctl not installed (sudo apt install playerctl)"

    return False, f"Unknown preset: {preset_id}"
