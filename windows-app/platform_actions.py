#!/usr/bin/env python3
"""
OctoPad - Windows System Actions & Automation Engine.

Executes the actions configured in the app on Windows:
  * keyboard shortcuts / typed text  -> SendInput (user32)
  * media keys                       -> SendInput (system media keys)
  * speaker / microphone volume      -> pycaw (Core Audio), media-key fallback
  * switching the default output     -> IPolicyConfig (Core Audio)
  * screenshots / lock / terminal     -> native Windows entry points
  * scripts                          -> cmd.exe
  * Home Assistant                   -> REST client (home_assistant.py)
"""

import ctypes
import json
import subprocess
import sys
import time
import urllib.request
from typing import Any, Dict, List, Optional, Tuple

try:  # wintypes only exists on Windows; the module must still import elsewhere.
    from ctypes import wintypes
    _HAS_WINTYPES = True
except Exception:  # noqa: BLE001
    wintypes = None  # type: ignore[assignment]
    _HAS_WINTYPES = False

_IS_WINDOWS = sys.platform.startswith("win")

_CREATE_NEW_CONSOLE = 0x00000010

# ---------------------------------------------------------------------------
# Built-in system action presets (ids kept identical to the Linux app so that
# profiles stay portable across platforms).
# ---------------------------------------------------------------------------
SYSTEM_ACTION_PRESETS: List[Dict[str, str]] = [
    {"id": "mic_toggle_mute", "name": "Toggle Microphone Mute", "category": "Audio",
     "description": "Mute/Unmute the current microphone (Meeting/Discord)"},
    {"id": "volume_up_5", "name": "Volume Up (+5%)", "category": "Audio",
     "description": "Raise the output volume by 5%"},
    {"id": "volume_down_5", "name": "Volume Down (-5%)", "category": "Audio",
     "description": "Lower the output volume by 5%"},
    {"id": "volume_toggle_mute", "name": "Toggle Speaker Mute", "category": "Audio",
     "description": "Mute/Unmute speaker audio output"},
    {"id": "lock_screen", "name": "Lock Windows Session", "category": "System",
     "description": "Lock the Windows session instantly (Win+L)"},
    {"id": "open_terminal", "name": "Open New Terminal", "category": "System",
     "description": "Open a new Windows Terminal / Command Prompt window"},
    {"id": "screenshot_interactive", "name": "Screenshot (Area)", "category": "System",
     "description": "Take a screenshot of a selected area (Snipping Tool overlay)"},
    {"id": "media_play_pause", "name": "Media Play / Pause", "category": "Media",
     "description": "Play or pause the active media player"},
    {"id": "media_next", "name": "Media Next Track", "category": "Media",
     "description": "Skip to the next track"},
    {"id": "media_prev", "name": "Media Previous Track", "category": "Media",
     "description": "Go back to the previous track"},
]

# ---------------------------------------------------------------------------
# SendInput structures (defined only on Windows; guarded everywhere else)
# ---------------------------------------------------------------------------
KEYEVENTF_EXTENDEDKEY = 0x0001
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004
INPUT_KEYBOARD = 1

_MODIFIER_VK = {"ctrl": 0x11, "alt": 0x12, "shift": 0x10, "super": 0x5B}
_VK_SPECIAL = {
    "ENTER": 0x0D, "ESC": 0x1B, "BACKSPACE": 0x08, "TAB": 0x09, "SPACE": 0x20,
    "UP": 0x26, "DOWN": 0x28, "LEFT": 0x25, "RIGHT": 0x27, "HOME": 0x24,
    "END": 0x23, "PAGE_UP": 0x21, "PAGE_DOWN": 0x22, "DELETE": 0x2E,
}
_EXTENDED_VK = {
    0x26, 0x28, 0x25, 0x27, 0x24, 0x23, 0x21, 0x22, 0x2E,
    0xAD, 0xAE, 0xAF, 0xB0, 0xB1, 0xB2, 0xB3,
}
_VK_MEDIA = {
    "MUTE": 0xAD, "VOL_UP": 0xAF, "VOL_DOWN": 0xAE,
    "PLAY_PAUSE": 0xB3, "NEXT_TRACK": 0xB0, "PREV_TRACK": 0xB1,
}

if _HAS_WINTYPES:
    _ULONG_PTR = (ctypes.c_ulonglong if ctypes.sizeof(ctypes.c_void_p) == 8
                  else ctypes.c_ulong)

    class KEYBDINPUT(ctypes.Structure):
        _fields_ = [("wVk", wintypes.WORD), ("wScan", wintypes.WORD),
                    ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD),
                    ("dwExtraInfo", _ULONG_PTR)]

    class MOUSEINPUT(ctypes.Structure):
        _fields_ = [("dx", ctypes.c_long), ("dy", ctypes.c_long),
                    ("mouseData", wintypes.DWORD), ("dwFlags", wintypes.DWORD),
                    ("time", wintypes.DWORD), ("dwExtraInfo", _ULONG_PTR)]

    class HARDWAREINPUT(ctypes.Structure):
        _fields_ = [("uMsg", wintypes.DWORD), ("wParamL", wintypes.WORD),
                    ("wParamH", wintypes.WORD)]

    class _INPUTUNION(ctypes.Union):
        _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT), ("hi", HARDWAREINPUT)]

    class INPUT(ctypes.Structure):
        _anonymous_ = ("u",)
        _fields_ = [("type", wintypes.DWORD), ("u", _INPUTUNION)]


def _make_key_input(vk: int, scan: int, flags: int):
    if not _HAS_WINTYPES:
        return None
    inp = INPUT()
    inp.type = INPUT_KEYBOARD
    inp.ki = KEYBDINPUT(vk, scan, flags, 0, 0)
    return inp


def _key_event(vk: int, key_up: bool) -> None:
    if not _HAS_WINTYPES:
        return
    flags = KEYEVENTF_KEYUP if key_up else 0
    if vk in _EXTENDED_VK:
        flags |= KEYEVENTF_EXTENDEDKEY
    inp = _make_key_input(vk, 0, flags)
    ctypes.windll.user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))


def _type_unicode_char(char: str) -> None:
    if not _HAS_WINTYPES:
        return
    code = ord(char)
    down = _make_key_input(0, code, KEYEVENTF_UNICODE)
    up = _make_key_input(0, code, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)
    ctypes.windll.user32.SendInput(1, ctypes.byref(down), ctypes.sizeof(INPUT))
    ctypes.windll.user32.SendInput(1, ctypes.byref(up), ctypes.sizeof(INPUT))


def _vk_for(key: str) -> Optional[int]:
    """Map a config key name to a Windows virtual-key code."""
    k = (key or "").upper()
    if len(k) == 1 and ("A" <= k <= "Z" or "0" <= k <= "9"):
        return ord(k)
    if len(k) >= 2 and k[0] == "F" and k[1:].isdigit():
        number = int(k[1:])
        if 1 <= number <= 24:
            return 0x70 + number - 1
    return _VK_SPECIAL.get(k)


def _send_text(text: str) -> Tuple[bool, str]:
    if not _IS_WINDOWS:
        return False, "Keyboard simulation is only available on Windows"
    for char in text:
        if char == "\n":
            _key_event(0x0D, False)
            _key_event(0x0D, True)
        elif char == "\t":
            _key_event(0x09, False)
            _key_event(0x09, True)
        else:
            _type_unicode_char(char)
    return True, "Text typed"


def _send_shortcut(modifiers: List[str], key: str) -> Tuple[bool, str]:
    if not _IS_WINDOWS:
        return False, "Keyboard simulation is only available on Windows"
    vk = _vk_for(key)
    if vk is None:
        return False, "Target key not selected yet"
    mods = [_MODIFIER_VK[m] for m in modifiers if m in _MODIFIER_VK]
    for mod in mods:
        _key_event(mod, False)
    _key_event(vk, False)
    _key_event(vk, True)
    for mod in reversed(mods):
        _key_event(mod, True)
    return True, "Shortcut sent"


def _media_key(media_key: str) -> Tuple[bool, str]:
    vk = _VK_MEDIA.get(media_key)
    if vk is None:
        return False, f"Unknown media key: {media_key}"
    if not _IS_WINDOWS:
        return False, "Media keys are only available on Windows"
    _key_event(vk, False)
    _key_event(vk, True)
    return True, f"Media key {media_key} sent"


# ---------------------------------------------------------------------------
# Core Audio helpers (pycaw)
# ---------------------------------------------------------------------------
def _speaker_volume():
    if not _IS_WINDOWS:
        return None
    try:
        from pycaw.utils import AudioUtilities
        return AudioUtilities.GetSpeakers().EndpointVolume
    except Exception:  # noqa: BLE001
        return None


def _microphone_volume():
    if not _IS_WINDOWS:
        return None
    try:
        from pycaw.utils import AudioUtilities
        return AudioUtilities.GetMicrophone().EndpointVolume
    except Exception:  # noqa: BLE001
        return None


def _change_speaker_volume(delta: float) -> Tuple[bool, str]:
    volume = _speaker_volume()
    if volume is None:
        return _media_key("VOL_UP" if delta > 0 else "VOL_DOWN")
    try:
        current = float(volume.GetMasterVolumeLevelScalar())
        target = max(0.0, min(1.0, current + delta))
        volume.SetMasterVolumeLevelScalar(target, None)
        return True, f"Volume {int(round(target * 100))}%"
    except Exception as exc:  # noqa: BLE001
        return False, f"Volume change failed: {exc}"


def _toggle_speaker_mute() -> Tuple[bool, str]:
    volume = _speaker_volume()
    if volume is None:
        return _media_key("MUTE")
    try:
        muted = bool(volume.GetMute())
        volume.SetMute(0 if muted else 1, None)
        return True, "Speaker unmuted" if muted else "Speaker muted"
    except Exception as exc:  # noqa: BLE001
        return False, f"Mute toggle failed: {exc}"


def _toggle_mic_mute() -> Tuple[bool, str]:
    volume = _microphone_volume()
    if volume is None:
        return False, "Microphone control requires pycaw and a default input device"
    try:
        muted = bool(volume.GetMute())
        volume.SetMute(0 if muted else 1, None)
        return True, "Microphone unmuted" if muted else "Microphone muted"
    except Exception as exc:  # noqa: BLE001
        return False, f"Microphone mute failed: {exc}"


# ---------------------------------------------------------------------------
# Default output switching (IPolicyConfig) and enumeration
# ---------------------------------------------------------------------------
class _GUID(ctypes.Structure):
    _fields_ = [("Data1", ctypes.c_ulong), ("Data2", ctypes.c_ushort),
                ("Data3", ctypes.c_ushort), ("Data4", ctypes.c_ubyte * 8)]


def _guid(text: str) -> "_GUID":
    import uuid
    value = uuid.UUID(text)
    guid = _GUID()
    guid.Data1 = value.time_low
    guid.Data2 = value.time_mid
    guid.Data3 = value.time_hi_version
    guid.Data4 = (ctypes.c_ubyte * 8)(*value.bytes[8:])
    return guid


_CLSID_POLICYCONFIG = "{870af99c-171d-4f9e-af0d-e63df40c2bc9}"
_IID_IPOLICYCONFIG = "{f8679f50-850a-41cf-9c72-430f290290c8}"
_CLSCTX_ALL = 0x17
# SetDefaultEndpoint is the 11th IPolicyConfig method (vtable index 3 + 10).
_SET_DEFAULT_ENDPOINT_INDEX = 13


def set_default_output(device_id: str) -> Tuple[bool, str]:
    """Set the default render endpoint (all roles) via IPolicyConfig."""
    if not _IS_WINDOWS:
        return False, "Audio output switching is only available on Windows"
    if not device_id:
        return False, "No audio output id provided"
    try:
        ole32 = ctypes.windll.ole32
        clsid = _guid(_CLSID_POLICYCONFIG)
        iid = _guid(_IID_IPOLICYCONFIG)
        ole32.CoInitialize(None)
        ptr = ctypes.c_void_p()
        hr = ole32.CoCreateInstance(ctypes.byref(clsid), None, _CLSCTX_ALL,
                                    ctypes.byref(iid), ctypes.byref(ptr))
        if hr != 0 or not ptr.value:
            return False, f"Cannot access the Windows audio policy service (0x{hr & 0xFFFFFFFF:08X})"
        vtable = ctypes.cast(
            ptr, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p))).contents
        prototype = ctypes.WINFUNCTYPE(ctypes.c_long, ctypes.c_void_p,
                                       ctypes.c_wchar_p, ctypes.c_int)
        set_default = prototype(vtable[_SET_DEFAULT_ENDPOINT_INDEX])
        for role in (0, 1, 2):  # eConsole, eMultimedia, eCommunications
            (ctypes.c_long)(set_default(ptr, device_id, role))
        return True, "Default audio output switched"
    except Exception as exc:  # noqa: BLE001
        return False, f"Audio output switch failed: {exc}"


def list_audio_outputs() -> List[Dict[str, str]]:
    """Active render devices as [{"name": device_id, "description": friendly}]."""
    if not _IS_WINDOWS:
        return []
    try:
        from pycaw.utils import AudioUtilities
        devices = AudioUtilities.GetAllDevices()
    except Exception as exc:  # noqa: BLE001
        print(f"[AUDIO] Failed to enumerate devices: {exc}")
        return []

    outputs = []
    for device in devices:
        try:
            data_flow = getattr(device, "data_flow", None)  # 0 = render
            state = getattr(device, "state", None)          # 1 = active
            if data_flow is not None and int(data_flow) != 0:
                continue
            if state is not None and int(state) != 1:
                continue
            device_id = getattr(device, "id", "") or ""
            friendly = getattr(device, "FriendlyName", "") or device_id
            if device_id:
                outputs.append({"name": device_id, "description": friendly})
        except Exception:  # noqa: BLE001
            continue
    return outputs


def _default_output_id() -> str:
    if not _IS_WINDOWS:
        return ""
    try:
        from pycaw.utils import AudioUtilities
        return AudioUtilities.GetSpeakers().id or ""
    except Exception:  # noqa: BLE001
        return ""


def _switch_audio_output(action_data: Dict[str, Any]) -> Tuple[bool, str]:
    sinks = [s for s in (action_data.get("sinks") or [])
             if isinstance(s, dict) and s.get("name")]
    names = [s["name"] for s in sinks]
    if not names:
        return False, "No audio output selected"
    try:
        index = names.index(_default_output_id())
    except ValueError:
        index = -1
    target = names[(index + 1) % len(names)]
    ok, message = set_default_output(target)
    if not ok:
        return False, message
    label = next((s.get("description") or s["name"]
                  for s in sinks if s["name"] == target), target)
    return True, f"Audio output: {label}"


# ---------------------------------------------------------------------------
# Built-in preset implementations
# ---------------------------------------------------------------------------
def _open_terminal() -> Tuple[bool, str]:
    import shutil
    if shutil.which("wt"):
        try:
            subprocess.Popen(["wt.exe"], creationflags=_CREATE_NEW_CONSOLE)
            return True, "Opened Windows Terminal"
        except Exception:  # noqa: BLE001
            pass
    try:
        subprocess.Popen("start cmd", shell=True)
        return True, "Opened Command Prompt"
    except Exception as exc:  # noqa: BLE001
        return False, f"Failed to open a terminal: {exc}"


def _lock_screen() -> Tuple[bool, str]:
    try:
        ctypes.windll.user32.LockWorkStation()
        return True, "Locked the Windows session"
    except Exception as exc:  # noqa: BLE001
        return False, f"Failed to lock the session: {exc}"


def _screenshot() -> Tuple[bool, str]:
    try:
        import os
        os.startfile("ms-screenclip:")
        return True, "Snipping Tool opened"
    except Exception:  # noqa: BLE001
        try:
            subprocess.Popen(["explorer.exe", "ms-screenclip:"])
            return True, "Snipping Tool opened"
        except Exception as exc:  # noqa: BLE001
            return False, f"Failed to open the screenshot tool: {exc}"


def _execute_system_preset(preset_id: str) -> Tuple[bool, str]:
    if preset_id == "mic_toggle_mute":
        return _toggle_mic_mute()
    if preset_id == "volume_toggle_mute":
        return _toggle_speaker_mute()
    if preset_id == "volume_up_5":
        return _change_speaker_volume(0.05)
    if preset_id == "volume_down_5":
        return _change_speaker_volume(-0.05)
    if preset_id == "lock_screen":
        return _lock_screen()
    if preset_id == "open_terminal":
        return _open_terminal()
    if preset_id == "screenshot_interactive":
        return _screenshot()
    if preset_id in ("media_play_pause", "media_next", "media_prev"):
        return _media_key({
            "media_play_pause": "PLAY_PAUSE",
            "media_next": "NEXT_TRACK",
            "media_prev": "PREV_TRACK",
        }[preset_id])
    return False, f"Unknown preset: {preset_id}"


# ---------------------------------------------------------------------------
# Dispatcher
# ---------------------------------------------------------------------------
def execute_action(action_type: str, action_data: Dict[str, Any]) -> Tuple[bool, str]:
    """Execute a configured action based on its type and payload."""
    try:
        if action_type == "mode_toggle":
            return True, "Mode layer is controlled by the macropad firmware (LED)"

        if action_type == "delay":
            try:
                seconds = float(action_data.get("seconds", 0.5))
            except (TypeError, ValueError):
                return False, "Delay needs a valid duration."
            seconds = max(0.0, min(seconds, 600.0))
            time.sleep(seconds)
            return True, f"Waited {seconds:g}s."

        if action_type == "shortcut":
            return _send_shortcut(action_data.get("modifiers", []),
                                  action_data.get("key", ""))

        if action_type == "text":
            text = action_data.get("text", "")
            if not text:
                return False, "Empty text"
            return _send_text(text)

        if action_type == "media":
            media_key = action_data.get("mediaKey", "")
            if media_key == "MUTE":
                return _toggle_speaker_mute()
            if media_key == "VOL_UP":
                return _change_speaker_volume(0.05)
            if media_key == "VOL_DOWN":
                return _change_speaker_volume(-0.05)
            return _media_key(media_key)

        if action_type == "audio_output":
            return _switch_audio_output(action_data)

        if action_type == "launch_app":
            from app_scanner import launch_app
            target = action_data.get("desktopId") or action_data.get("path")
            return launch_app(target)

        if action_type == "system_action":
            return _execute_system_preset(action_data.get("presetId"))

        if action_type == "bash_script":
            script_text = action_data.get("script", "").strip()
            if not script_text:
                return False, "Script content is empty"
            subprocess.Popen(script_text, shell=True)
            return True, "Script dispatched in background"

        if action_type == "ha_webhook":
            url = action_data.get("url", "").strip()
            payload = action_data.get("payload", {})
            if not url:
                return False, "Home Assistant Webhook URL is empty"
            data_bytes = json.dumps(payload).encode("utf-8") if payload else b"{}"
            request = urllib.request.Request(
                url, data=data_bytes,
                headers={"Content-Type": "application/json"}, method="POST")
            with urllib.request.urlopen(request, timeout=4) as response:
                status = response.status
            return True, f"Webhook sent to Home Assistant (HTTP {status})"

        if action_type in ("home_assistant", "ha_service"):
            from home_assistant import ha_client
            domain = action_data.get("domain", "")
            service = action_data.get("service", "toggle")
            entity_id = action_data.get("entityId") or action_data.get("entity_id", "")
            data = action_data.get("data") or {}
            if not domain and "." in entity_id:
                domain = entity_id.split(".")[0]
            if not domain or not service:
                return False, "Home Assistant domain or service not set"
            return ha_client.call_service(domain, service, entity_id, data)

        return False, f"Unknown action type: {action_type}"

    except Exception as exc:  # noqa: BLE001
        return False, f"Action execution error: {exc}"
