#!/usr/bin/env python3
"""
OctoPad - Application runtime state (no GTK dependency).

All UI subscribes to changes via subscribe(); this module does not import
gi/Gtk so it can be used from anywhere (including unit tests).
"""

from typing import Any, Callable, Dict, List, Optional

import copy
import threading
import time

import config as config_module
from link_manager import BLUETOOTH, WIRED, LinkManager

# Thread that owns the GTK main loop (captured at import time). Emits coming
# from worker threads (e.g. the BLE bridge) are marshalled back onto it.
_MAIN_THREAD = threading.get_ident()

# ---------------------------------------------------------------------------
# Data
# ---------------------------------------------------------------------------
cfg: Dict[str, Any] = config_module.load_config()
# Snapshot of the config exactly as it was last loaded/saved. The "dirty" flag
# is derived by comparing the working config against this baseline, so undoing
# a change (or re-selecting the same value) clears it automatically.
_saved_cfg: Dict[str, Any] = copy.deepcopy(cfg)
mode_index: int = 0          # 0 = Desktop, 1 = Home Assistant
selected_index: int = 0      # button B1..B8 currently being edited (0-7)
dirty: bool = False          # there are unsaved changes
active_preset: Optional[str] = cfg.get("activePreset")
# Ignore a stale active preset whose file no longer exists.
if active_preset and active_preset not in config_module.list_presets():
    active_preset = None

installed_apps: List[Dict[str, str]] = []
system_presets: List[Dict[str, Any]] = []
ha_entities: List[Dict[str, Any]] = []
ha_entity_source: str = "empty"   # "live" | "cache" | "empty"
ha_connected: bool = False
ha_message: str = ""

serial_port: Optional[str] = None
serial_connected: bool = False
serial_manually_disconnected: bool = False  # released so the IDE can flash

device_name: Optional[str] = None  # user-configured pad name (from identity handshake)
firmware_version: Optional[str] = None  # firmware version reported in the HELLO line

ble_connected: bool = False
ble_message: str = ""

pad_config_synced: Optional[bool] = None  # action table copied to the pad?

listener: Any = None         # SerialDaemonListener (set by main.py)
ble: Any = None              # BleBridge (set by main.py)

_subscribers: List[Callable[..., None]] = []

# Which transport owns the buttons (USB serial or BLE). Both may be connected
# at once; only the active link executes an action.
links = LinkManager(policy=cfg.get("inputLink", "auto"))
cfg["inputLink"] = links.policy
_sync_lock = threading.RLock()

_FIRMWARE_LINK = {"auto": "AUTO", "wired": "USB", "bluetooth": "BLE"}


def link_command(policy: str) -> str:
    """The firmware command that mirrors an input-link policy."""
    return "LINK:" + _FIRMWARE_LINK.get((policy or "").strip().lower(), "AUTO")


def allow_button(link: str, event: Dict[str, Any]) -> bool:
    """True when this event from this link should execute exactly once."""
    return links.allows(link, event)


def _persist_input_link(policy: str) -> None:
    """Store the preference without flushing unsaved edits from the editor."""
    stored = config_module.load_config()
    stored["inputLink"] = policy
    config_module.save_config(stored)


def sync_input_link() -> bool:
    """Tell the pad which transport should carry button events."""
    return send_device_command(link_command(links.policy), log=False)


def set_input_link(policy: str, persist: bool = True) -> str:
    """Apply an input-link policy now and (optionally) persist the choice."""
    canonical = links.set_policy(policy)
    if cfg.get("inputLink") != canonical:
        cfg["inputLink"] = canonical
        _saved_cfg["inputLink"] = canonical
        if persist:
            _persist_input_link(canonical)
    emit("input-link-changed", policy=canonical, active=links.active_link)
    sync_input_link()
    return canonical


def set_link_connected(link: str, connected: bool) -> bool:
    if not links.set_connected(link, connected):
        return False
    emit("input-link-changed", policy=links.policy, active=links.active_link)
    sync_input_link()
    return True


def ping_pad() -> bool:
    """Tell the firmware the app is alive (it falls back to HID when not)."""
    return send_device_command("PING", log=False)


def sync_firmware_actions() -> bool:
    """Copy the action table to the pad over USB so it works without the app.

    Long payloads are sent over the cable only: the BLE RX characteristic is
    limited by the connection MTU, and the primary app link is USB anyway.
    The small pause between lines matters: the ESP32-C3 USB CDC drops bytes
    when the whole table arrives in one burst.
    """
    global pad_config_synced
    with _sync_lock:
        target = listener
        if target is None:
            pad_config_synced = False
            emit("pad-config-changed", synced=False)
            return False
        lines = config_module.firmware_actions(cfg)
        try:
            for index, line in enumerate(lines):
                if not target.send_command(line, log=False):
                    pad_config_synced = False
                    emit("pad-config-changed", synced=False)
                    return False
                if index + 1 < len(lines):
                    time.sleep(0.005)
        except Exception as exc:  # noqa: BLE001
            print(f"[STATE] firmware action sync failed: {exc}")
            pad_config_synced = False
            emit("pad-config-changed", synced=False)
            return False
        pad_config_synced = True
        emit("pad-config-changed", synced=True)
        return True


def _sync_timings() -> None:
    """Push the current timing parameters to every connected transport.

    The serial listener and the BLE bridge each expose ``sync_timings``; either
    may be absent or not connected, so failures are ignored on purpose.
    """
    for target in (listener, ble):
        if target is None:
            continue
        try:
            target.sync_timings()
        except Exception as exc:  # noqa: BLE001
            print(f"[STATE] failed to sync timings: {exc}")


def send_device_command(command: str, log: bool = True) -> bool:
    """Send a text command to the pad, preferring USB serial over BLE."""
    for target in (listener, ble):
        if target is None:
            continue
        try:
            if target.send_command(command, log=log):
                return True
        except Exception as exc:  # noqa: BLE001
            print(f"[STATE] failed to send device command: {exc}")
    return False


def rename_device(name: str) -> bool:
    """Rename the pad over whichever transport is connected (USB or BLE)."""
    name = name.strip()
    if not name or len(name) > 24:
        return False
    if send_device_command(f"NAME:{name}"):
        set_device_name(name)
        return True
    return False


# ---------------------------------------------------------------------------
# Simple bus
# ---------------------------------------------------------------------------
def subscribe(callback: Callable[..., None]) -> Callable[..., None]:
    _subscribers.append(callback)
    return callback


def unsubscribe(callback: Callable[..., None]) -> None:
    """Remove a listener registered with subscribe() (e.g. a closed dialog)."""
    try:
        _subscribers.remove(callback)
    except ValueError:
        pass


def emit(event: str, **data: Any) -> None:
    if threading.get_ident() == _MAIN_THREAD:
        _dispatch(event, data)
        return

    # Called from a worker thread (e.g. the BLE bridge): GTK must only be
    # touched on the main thread, so re-enter the loop before notifying UI.
    try:
        from gi.repository import GLib  # noqa: PLC0415
        GLib.idle_add(_dispatch, event, data)
    except Exception:  # noqa: BLE001
        _dispatch(event, data)


def _dispatch(event: str, data: Dict[str, Any]) -> None:
    for callback in list(_subscribers):
        try:
            callback(event, **data)
        except Exception as exc:  # the UI must never take down the listener
            print(f"[STATE] subscriber {callback} failed: {exc}")


# ---------------------------------------------------------------------------
# Mutator
# ---------------------------------------------------------------------------
def current_mode() -> Dict[str, Any]:
    return cfg["modes"][mode_index]


def current_button() -> Dict[str, Any]:
    return config_module.get_button(cfg, mode_index, selected_index)


def mark_dirty() -> None:
    """Re-evaluate whether the working config differs from the saved baseline."""
    _sync_dirty()


# Runtime metadata that is toggled by navigation/preset bookkeeping rather than
# by editing. On its own it must not make the configuration look "modified".
_NON_EDIT_KEYS = ("activeModeIndex", "activePreset")


def _config_signature(config: Dict[str, Any]) -> Dict[str, Any]:
    """Normalized view of the config used to detect real user edits."""
    return {key: value for key, value in config.items()
            if key not in _NON_EDIT_KEYS}


def has_changes() -> bool:
    """True when the working config differs from the last saved/loaded one."""
    return _config_signature(cfg) != _config_signature(_saved_cfg)


def _sync_dirty() -> bool:
    """Recompute the dirty flag; emit dirty-changed only when it flips."""
    global dirty
    new_dirty = has_changes()
    if new_dirty != dirty:
        dirty = new_dirty
        emit("dirty-changed", dirty=dirty)
        return True
    return False


def _mark_saved() -> None:
    """Remember the current config as the new saved baseline."""
    global _saved_cfg
    _saved_cfg = copy.deepcopy(cfg)
    _sync_dirty()


def set_config(new_config: Dict[str, Any], source: str = "editor") -> None:
    """Replace the entire configuration (import/preset/reset) and apply it."""
    global cfg
    cfg = new_config
    if listener is not None:
        try:
            listener.config = cfg
            _sync_timings()
        except Exception as exc:
            print(f"[STATE] failed to apply config to listener: {exc}")
    emit("config-replaced", source=source)
    _sync_dirty()


def set_label(text: str) -> None:
    current_button()["label"] = text
    mark_dirty()
    emit("button-changed", index=selected_index)


def set_action(trigger: str, index: int, action: Dict[str, Any]) -> None:
    """Replace the action at position `index` in the trigger slot (appended at the old position if empty)."""
    actions = get_actions(trigger)
    if index < len(actions):
        actions[index] = action
    else:
        actions.append(action)
    current_button()[trigger] = actions
    mark_dirty()
    emit("button-changed", index=selected_index)


def add_action(trigger: str, action: Optional[Dict[str, Any]] = None) -> None:
    """Add a new action to the trigger slot (default: 'none', editable in the editor)."""
    actions = get_actions(trigger)
    actions.append(action if isinstance(action, dict)
                   else config_module.default_action("none"))
    current_button()[trigger] = actions
    mark_dirty()
    emit("button-changed", index=selected_index)


def remove_action(trigger: str, index: int) -> None:
    """Remove an action from the trigger slot; ensure at least one slot always exists."""
    actions = get_actions(trigger)
    if index < len(actions):
        del actions[index]
    if not actions:
        actions = [config_module.default_action("none")]
    current_button()[trigger] = actions
    mark_dirty()
    emit("button-changed", index=selected_index)


def move_action(trigger: str, index: int, delta: int) -> None:
    """Move an action within the trigger slot by `delta` positions (no-op if out of range)."""
    actions = get_actions(trigger)
    target = index + delta
    if index < 0 or index >= len(actions) or target < 0 or target >= len(actions):
        return
    actions[index], actions[target] = actions[target], actions[index]
    current_button()[trigger] = actions
    mark_dirty()
    emit("button-changed", index=selected_index)


def get_actions(trigger: str) -> List[Dict[str, Any]]:
    """List of actions in the trigger slot (legacy dicts normalized to a list)."""
    return config_module.actions_list(current_button().get(trigger))


def set_timing(field: str, value: int) -> None:
    cfg[field] = value
    mark_dirty()
    emit("timing-changed", field=field, value=value)


def set_mode(index: int) -> None:
    global mode_index
    if index == mode_index:
        return
    mode_index = index
    cfg["activeModeIndex"] = index
    emit("mode-changed", mode_index=index)


def select_button(index: int) -> None:
    global selected_index
    if index == selected_index:
        return
    selected_index = index
    emit("selection-changed", index=index)


def save() -> bool:
    """Save to disk and activate the listener (action execution path)."""
    ok = config_module.save_config(cfg)
    if ok:
        _mark_saved()
        if listener is not None:
            try:
                listener.config = cfg
                _sync_timings()
            except Exception as exc:
                print(f"[STATE] failed to sync to listener: {exc}")
        sync_firmware_actions()
        emit("config-saved", ok=True)
    else:
        emit("config-saved", ok=False)
    return ok


def save_preset(name: str, overwrite: bool = False) -> bool:
    """Save the configuration as a user preset.

    overwrite=True replaces an existing preset: if ``name`` is given it targets
    that preset, otherwise the currently active preset is used.
    Otherwise ``name`` is slugified and a new preset is created.
    """
    global active_preset
    if overwrite:
        preset_name = config_module.preset_slug(name) if name else active_preset
        if not preset_name:
            emit("config-saved", ok=False)
            return False
    else:
        preset_name = config_module.preset_slug(name)
    snapshot = copy.deepcopy(cfg)
    snapshot.pop("activePreset", None)
    ok = config_module.save_preset(preset_name, snapshot)
    if ok:
        active_preset = preset_name
        cfg["activePreset"] = preset_name
        config_module.save_config(cfg)
        _mark_saved()
        if listener is not None:
            try:
                listener.config = cfg
                _sync_timings()
            except Exception as exc:
                print(f"[STATE] failed to sync to listener: {exc}")
        emit("config-saved", ok=True, preset=preset_name)
    else:
        emit("config-saved", ok=False)
    return ok


def apply_user_preset(name: str) -> bool:
    """Load a user preset into the editor (marks the configuration as changed)."""
    global active_preset
    data = config_module.load_preset(name)
    if data is None:
        return False
    # Set the active flag *before* notifying listeners, so the UI rebuilds its
    # preset list with the correct "active" indicator.
    active_preset = name
    data["activePreset"] = name
    set_config(data, source="preset")
    return True


def delete_user_preset(name: str) -> bool:
    """Delete a user preset (clears the active flag if it was active)."""
    global active_preset
    if name not in config_module.list_presets():
        return False
    ok = config_module.delete_preset(name)
    if not ok:
        return False
    if active_preset == name:
        active_preset = None
        cfg.pop("activePreset", None)
        # Persist the metadata change, but never clobber unsaved edits.
        if not dirty:
            config_module.save_config(cfg)
            _mark_saved()
    # Always notify so the UI rebuilds its preset list (and drops the row).
    emit("presets-changed", name=name)
    _sync_dirty()
    return True


def set_apps(apps: List[Dict[str, str]]) -> None:
    global installed_apps
    installed_apps = apps
    emit("apps-changed", count=len(apps))


def set_ha_entities(entities: List[Dict[str, Any]], source: str) -> None:
    global ha_entities, ha_entity_source
    ha_entities = entities
    ha_entity_source = source
    emit("ha-entities-changed", count=len(entities), source=source)


def set_ha_status(connected: bool, message: str) -> None:
    global ha_connected, ha_message
    ha_connected = connected
    ha_message = message
    emit("ha-status-changed", connected=connected, message=message)


def disconnect_serial() -> bool:
    """Release the USB port so Arduino IDE / arduino-cli can flash.

    The BLE bridge is untouched: the pad stays reachable as a keyboard and,
    while this app keeps running, events still arrive over Bluetooth.
    """
    global serial_manually_disconnected
    serial_manually_disconnected = True
    target = listener
    if target is not None:
        try:
            target.stop()
        except Exception as exc:  # noqa: BLE001
            print(f"[STATE] failed to release the serial port: {exc}")
    set_serial_status(False, None)
    emit("serial-manual-changed", released=True)
    return True


def reconnect_serial() -> bool:
    """Reopen the USB port after flashing (the listener reconnects by itself)."""
    global serial_manually_disconnected
    serial_manually_disconnected = False
    target = listener
    if target is not None:
        try:
            target.start()
        except Exception as exc:  # noqa: BLE001
            print(f"[STATE] failed to reopen the serial port: {exc}")
            emit("serial-manual-changed", released=False, ok=False)
            return False
    emit("serial-manual-changed", released=False)
    return True


def set_serial_status(connected: bool, port: Optional[str]) -> None:
    global serial_connected, serial_port
    if connected != serial_connected or port != serial_port:
        serial_connected = connected
        serial_port = port
        emit("serial-status-changed", connected=connected, port=port)
    set_link_connected(WIRED, connected)


def set_ble_status(connected: bool, message: str) -> None:
    """Bluetooth connection status (BLE bridge). Does not emit if unchanged."""
    global ble_connected, ble_message
    if connected != ble_connected or message != ble_message:
        ble_connected = connected
        ble_message = message
        emit("ble-status-changed", connected=connected, message=message)
    set_link_connected(BLUETOOTH, connected)


def set_device_name(name: Optional[str]) -> None:
    """Update the pad's user-configured name (learned via the identity line)."""
    global device_name
    name = (name or "").strip() or None
    if name == device_name:
        return
    device_name = name
    emit("device-name-changed", name=name)


def set_firmware_version(version: Optional[str]) -> None:
    """Update the firmware version reported by the pad (from the HELLO line)."""
    global firmware_version
    version = (version or "").strip() or None
    if version == firmware_version:
        return
    firmware_version = version
    emit("device-info-changed", version=version)
