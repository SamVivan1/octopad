#!/usr/bin/env python3
"""
OctoPad - Application runtime state (Qt-free data, Qt marshalled events).

All UI subscribes to changes via subscribe(). Events emitted from worker
threads (serial listener, BLE bridge, async scans) are marshalled back onto the
Qt main thread through a lightweight QObject bridge.
"""

import copy
import threading
from typing import Any, Callable, Dict, List, Optional

from PySide6.QtCore import QObject, Signal, Slot

import config as config_module


class _Bridge(QObject):
    """Signal carrier that safely hops from worker threads to the Qt loop."""

    dispatch = Signal(str, dict)
    async_done = Signal(object, object)


# Created on the main thread at import time.
_bridge = _Bridge()

_subscribers: List[Callable[..., None]] = []


@Slot(str, dict)
def _run_dispatch(event: str, data: Dict[str, Any]) -> None:
    for callback in list(_subscribers):
        try:
            callback(event, **data)
        except Exception as exc:  # the UI must never take down the listener
            print(f"[STATE] subscriber {callback} failed: {exc}")


@Slot(object, object)
def _run_async_done(callback: Any, result: Any) -> None:
    if callback is None:
        return
    try:
        callback(result)
    except Exception as exc:  # noqa: BLE001
        print(f"[STATE] async callback failed: {exc}")


_bridge.dispatch.connect(_run_dispatch)
_bridge.async_done.connect(_run_async_done)


# ---------------------------------------------------------------------------
# Data
# ---------------------------------------------------------------------------
cfg: Dict[str, Any] = config_module.load_config()
_saved_cfg: Dict[str, Any] = copy.deepcopy(cfg)
mode_index: int = 0          # 0 = Desktop, 1 = Home Assistant
selected_index: int = 0      # button B1..B8 currently being edited (0-7)
dirty: bool = False          # there are unsaved changes
active_preset: Optional[str] = cfg.get("activePreset")
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

device_name: Optional[str] = None

ble_connected: bool = False
ble_message: str = ""

listener: Any = None         # SerialDaemonListener (set by main.py)


# ---------------------------------------------------------------------------
# Simple bus
# ---------------------------------------------------------------------------
def subscribe(callback: Callable[..., None]) -> Callable[..., None]:
    if callback not in _subscribers:
        _subscribers.append(callback)
    return callback


def unsubscribe(callback: Callable[..., None]) -> None:
    """Remove a listener registered with subscribe() (e.g. a closed dialog)."""
    try:
        _subscribers.remove(callback)
    except ValueError:
        pass


def emit(event: str, **data: Any) -> None:
    """Emit an event to every subscriber (marshalled onto the Qt main thread)."""
    _bridge.dispatch.emit(event, data)


def run_async(work: Callable[[], Any],
              on_done: Optional[Callable[[Any], None]] = None) -> None:
    """Run `work` on a worker thread; post the result back to the Qt main loop."""
    def target() -> None:
        try:
            result = work()
        except Exception as exc:  # noqa: BLE001
            result = (False, str(exc))
        _bridge.async_done.emit(on_done, result)

    threading.Thread(target=target, daemon=True).start()


# ---------------------------------------------------------------------------
# Data access helpers
# ---------------------------------------------------------------------------
def current_mode() -> Dict[str, Any]:
    return cfg["modes"][mode_index]


def current_button() -> Dict[str, Any]:
    return config_module.get_button(cfg, mode_index, selected_index)


def mark_dirty() -> None:
    _sync_dirty()


_NON_EDIT_KEYS = ("activeModeIndex", "activePreset")


def _config_signature(config: Dict[str, Any]) -> Dict[str, Any]:
    return {key: value for key, value in config.items()
            if key not in _NON_EDIT_KEYS}


def has_changes() -> bool:
    return _config_signature(cfg) != _config_signature(_saved_cfg)


def _sync_dirty() -> bool:
    global dirty
    new_dirty = has_changes()
    if new_dirty != dirty:
        dirty = new_dirty
        emit("dirty-changed", dirty=dirty)
        return True
    return False


def _mark_saved() -> None:
    global _saved_cfg
    _saved_cfg = copy.deepcopy(cfg)
    _sync_dirty()


def set_config(new_config: Dict[str, Any], source: str = "editor") -> None:
    """Replace the whole configuration (import/preset/reset)."""
    global cfg
    cfg = new_config
    if listener is not None:
        try:
            listener.config = cfg
            listener.sync_timings()
        except Exception as exc:
            print(f"[STATE] failed to apply config to listener: {exc}")
    emit("config-replaced", source=source)
    _sync_dirty()


def set_label(text: str) -> None:
    current_button()["label"] = text
    mark_dirty()
    emit("button-changed", index=selected_index)


def set_action(trigger: str, index: int, action: Dict[str, Any]) -> None:
    actions = get_actions(trigger)
    if index < len(actions):
        actions[index] = action
    else:
        actions.append(action)
    current_button()[trigger] = actions
    mark_dirty()
    emit("button-changed", index=selected_index)


def add_action(trigger: str, action: Optional[Dict[str, Any]] = None) -> None:
    actions = get_actions(trigger)
    actions.append(action if isinstance(action, dict)
                   else config_module.default_action("none"))
    current_button()[trigger] = actions
    mark_dirty()
    emit("button-changed", index=selected_index)


def remove_action(trigger: str, index: int) -> None:
    actions = get_actions(trigger)
    if index < len(actions):
        del actions[index]
    if not actions:
        actions = [config_module.default_action("none")]
    current_button()[trigger] = actions
    mark_dirty()
    emit("button-changed", index=selected_index)


def move_action(trigger: str, index: int, delta: int) -> None:
    actions = get_actions(trigger)
    target = index + delta
    if index < 0 or index >= len(actions) or target < 0 or target >= len(actions):
        return
    actions[index], actions[target] = actions[target], actions[index]
    current_button()[trigger] = actions
    mark_dirty()
    emit("button-changed", index=selected_index)


def get_actions(trigger: str) -> List[Dict[str, Any]]:
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
                listener.sync_timings()
            except Exception as exc:
                print(f"[STATE] failed to sync to listener: {exc}")
        emit("config-saved", ok=True)
    else:
        emit("config-saved", ok=False)
    return ok


def save_preset(name: str, overwrite: bool = False) -> bool:
    """Save the configuration as a user preset (see Linux app for semantics)."""
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
                listener.sync_timings()
            except Exception as exc:
                print(f"[STATE] failed to sync to listener: {exc}")
        emit("presets-changed", name=preset_name)
        emit("config-saved", ok=True)
    else:
        emit("config-saved", ok=False)
    return ok


def apply_user_preset(name: str) -> bool:
    global active_preset
    data = config_module.load_preset(name)
    if not isinstance(data, dict):
        return False
    active_preset = name
    data["activePreset"] = name
    set_config(data, source="preset")
    return True


def delete_user_preset(name: str) -> bool:
    global active_preset
    if name not in config_module.list_presets():
        return False
    if not config_module.delete_preset(name):
        return False
    if active_preset == name:
        active_preset = None
        cfg.pop("activePreset", None)
        config_module.save_config(cfg)
        _mark_saved()
    emit("presets-changed", name=name)
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


def set_serial_status(connected: bool, port: Optional[str]) -> None:
    global serial_connected, serial_port
    if connected == serial_connected and port == serial_port:
        return
    serial_connected = connected
    serial_port = port
    emit("serial-status-changed", connected=connected, port=port)


def set_ble_status(connected: bool, message: str) -> None:
    global ble_connected, ble_message
    if connected == ble_connected and message == ble_message:
        return
    ble_connected = connected
    ble_message = message
    emit("ble-status-changed", connected=connected, message=message)


def set_device_name(name: Optional[str]) -> None:
    global device_name
    name = (name or "").strip() or None
    if name == device_name:
        return
    device_name = name
    emit("device-name-changed", name=name)
