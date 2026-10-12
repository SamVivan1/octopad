#!/usr/bin/env python3
"""
OctoPad - Tray (in-process StatusNotifierItem).

Implements the freedesktop StatusNotifierItem + com.canonical.dbusmenu D-Bus
protocols directly with Gio, so the tray lives inside the GTK4 application.
This replaces the old GTK3 + AyatanaAppIndicator helper process: no second
process, no GTK3, and roughly 60 MB less RAM.

If no StatusNotifierWatcher is present (i.e. no AppIndicator shell extension),
start() returns False and the application simply runs without a tray.
"""

from typing import Callable, List

import os

import gi
gi.require_version("GLib", "2.0")
gi.require_version("Gio", "2.0")
from gi.repository import GLib, Gio  # noqa: E402

import state  # noqa: E402

_WATCHER_SERVICE = "org.kde.StatusNotifierWatcher"
_WATCHER_PATH = "/StatusNotifierWatcher"
_WATCHER_IFACE = "org.kde.StatusNotifierWatcher"
_SNI_IFACE = "org.kde.StatusNotifierItem"
_SNI_PATH = "/StatusNotifierItem"
_MENU_IFACE = "com.canonical.dbusmenu"
_MENU_PATH = "/MenuBar"

# dbusmenu item ids
_ID_OPEN = 1
_ID_QUIT = 2
_ID_STATUS_BASE = 10
_ID_SEPARATOR = 100

_SNI_XML = f"""
<node>
  <interface name="{_SNI_IFACE}">
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="OverlayIconName" type="s" access="read"/>
    <property name="OverlayIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionIconName" type="s" access="read"/>
    <property name="AttentionIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionMovieName" type="s" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <method name="Activate">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="SecondaryActivate">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="Scroll">
      <arg name="delta" type="i" direction="in"/>
      <arg name="orientation" type="s" direction="in"/>
    </method>
    <method name="ContextMenu">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <signal name="NewIcon"/>
    <signal name="NewStatus">
      <arg name="status" type="s"/>
    </signal>
    <signal name="NewTitle"/>
    <signal name="NewToolTip"/>
  </interface>
</node>
"""

_MENU_XML = """
<node>
  <interface name="com.canonical.dbusmenu">
    <property name="Version" type="u" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconThemePath" type="as" access="read"/>
    <method name="GetLayout">
      <arg name="parentId" type="i" direction="in"/>
      <arg name="recursionDepth" type="i" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="revision" type="u" direction="out"/>
      <arg name="layout" type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="properties" type="a(ia{sv})" direction="out"/>
    </method>
    <method name="Event">
      <arg name="id" type="i" direction="in"/>
      <arg name="eventId" type="s" direction="in"/>
      <arg name="data" type="v" direction="in"/>
      <arg name="timestamp" type="u" direction="in"/>
    </method>
    <method name="AboutToShow">
      <arg name="id" type="i" direction="in"/>
      <arg name="needUpdate" type="b" direction="out"/>
    </method>
    <method name="AboutToShowGroup">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="updatesNeeded" type="ai" direction="out"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <signal name="LayoutUpdated">
      <arg name="revision" type="u"/>
      <arg name="parent" type="i"/>
    </signal>
  </interface>
</node>
"""


def _s(value: str) -> GLib.Variant:
    return GLib.Variant("s", value)


class StatusNotifierTray:
    """In-process tray icon + menu over D-Bus (StatusNotifierItem)."""

    def __init__(self, on_show: Callable[[], None],
                 on_quit: Callable[[], None],
                 status_labels: Callable[[], List[str]]):
        self._on_show = on_show
        self._on_quit = on_quit
        self._status_labels = status_labels
        self._bus: Gio.DBusConnection | None = None
        self._service_name = f"org.kde.StatusNotifierItem-{os.getpid()}-1"
        self._revision = 0
        self._current_state: str | None = None
        self._sni_info: Gio.DBusInterfaceInfo | None = None
        self._menu_info: Gio.DBusInterfaceInfo | None = None

    # ------------------------------------------------------------------ API
    def start(self) -> bool:
        try:
            self._bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        except Exception as exc:  # noqa: BLE001
            print(f"[TRAY] No session bus: {exc}")
            return False

        if not self._watcher_available():
            print("[TRAY] No StatusNotifierWatcher; running without a tray.")
            return False

        try:
            if not self._own_name():
                return False
            self._register_objects()
            if not self._register_with_watcher():
                return False
        except Exception as exc:  # noqa: BLE001
            print(f"[TRAY] Failed to start tray: {exc}")
            self.stop()
            return False

        state.subscribe(self._on_state_event)
        self.refresh()
        return True

    def refresh(self) -> None:
        """Update icon + menu labels when connection status changes."""
        if self._bus is None:
            return
        state_key = self._device_state()
        if state_key != self._current_state:
            self._current_state = state_key
            self._emit(_SNI_IFACE, _SNI_PATH, "NewIcon", None)
            self._emit(_SNI_IFACE, _SNI_PATH, "NewStatus",
                       GLib.Variant("(s)", ("Active",)))
            self._emit(_SNI_IFACE, _SNI_PATH, "NewToolTip", None)
        self._revision += 1
        self._emit(_MENU_IFACE, _MENU_PATH, "LayoutUpdated",
                   GLib.Variant("(ui)", (self._revision, 0)))

    def stop(self) -> None:
        state.unsubscribe(self._on_state_event)
        if self._bus is None:
            return
        for path in (_SNI_PATH, _MENU_PATH):
            try:
                self._bus.unregister_object(path)
            except Exception:  # noqa: BLE001
                pass
        try:
            self._bus.call_sync(
                "org.freedesktop.DBus", "/org/freedesktop/DBus",
                "org.freedesktop.DBus", "ReleaseName",
                GLib.Variant("(s)", (self._service_name,)),
                None, Gio.DBusCallFlags.NONE, 1000, None)
        except Exception:  # noqa: BLE001
            pass
        self._bus = None

    # ------------------------------------------------------------- internal
    @staticmethod
    def _device_state() -> str:
        if state.serial_connected or state.ble_connected:
            return "ok"
        ble_msg = (state.ble_message or "").lower()
        if "searching" in ble_msg or "retry" in ble_msg:
            return "checking"
        return "off"

    def _icon_name(self) -> str:
        return {
            "ok": "octopad-tray",
            "checking": "octopad-tray-checking",
            "off": "octopad-tray-off",
        }.get(self._current_state or "off", "octopad-tray-off")

    def _tooltip(self) -> str:
        mode = "Home Assistant" if state.mode_index == 1 else "Desktop"
        return f"OctoPad — {mode} mode"

    def _watcher_available(self) -> bool:
        try:
            reply = self._bus.call_sync(
                "org.freedesktop.DBus", "/org/freedesktop/DBus",
                "org.freedesktop.DBus", "NameHasOwner",
                GLib.Variant("(s)", (_WATCHER_SERVICE,)),
                None, Gio.DBusCallFlags.NONE, 2000, None)
            return bool(reply.unpack()[0])
        except Exception:  # noqa: BLE001
            return False

    def _own_name(self) -> bool:
        reply = self._bus.call_sync(
            "org.freedesktop.DBus", "/org/freedesktop/DBus",
            "org.freedesktop.DBus", "RequestName",
            GLib.Variant("(su)", (self._service_name, 0x4)),
            None, Gio.DBusCallFlags.NONE, 2000, None)
        result = reply.unpack()[0]
        if result not in (1, 4):  # PRIMARY_OWNER / ALREADY_OWNER
            print(f"[TRAY] Could not own {self._service_name} (code {result}).")
            return False
        return True

    def _register_objects(self) -> None:
        node = Gio.DBusNodeInfo.new_for_xml(_SNI_XML)
        self._sni_info = node.lookup_interface(_SNI_IFACE)
        node = Gio.DBusNodeInfo.new_for_xml(_MENU_XML)
        self._menu_info = node.lookup_interface(_MENU_IFACE)
        self._bus.register_object(
            _SNI_PATH, self._sni_info, self._on_sni_method,
            self._get_sni_property, None)
        self._bus.register_object(
            _MENU_PATH, self._menu_info, self._on_menu_method,
            self._get_menu_property, None)

    def _register_with_watcher(self) -> bool:
        try:
            self._bus.call_sync(
                _WATCHER_SERVICE, _WATCHER_PATH, _WATCHER_IFACE,
                "RegisterStatusNotifierItem",
                GLib.Variant("(s)", (self._service_name,)),
                None, Gio.DBusCallFlags.NONE, 2000, None)
            return True
        except Exception as exc:  # noqa: BLE001
            print(f"[TRAY] Watcher registration failed: {exc}")
            return False

    def _emit(self, iface: str, path: str, signal: str,
              params: GLib.Variant | None) -> None:
        if self._bus is None:
            return
        try:
            self._bus.emit_signal(None, path, iface, signal, params)
        except Exception as exc:  # noqa: BLE001
            print(f"[TRAY] Failed to emit {signal}: {exc}")

    # -------------------------------------------------------- menu building
    def _status_items(self) -> list:
        try:
            labels = [str(x) for x in self._status_labels()]
        except Exception:  # noqa: BLE001
            labels = []
        items = []
        for i in range(3):
            text = labels[i] if i < len(labels) else "…"
            items.append((_ID_STATUS_BASE + i,
                          {"label": _s(text), "enabled": GLib.Variant("b", False)},
                          []))
        return items

    def _flat_items(self) -> list:
        items = [(_ID_OPEN, {"label": _s("Open Application")}, [])]
        items.extend(self._status_items())
        items.append((_ID_SEPARATOR, {"type": _s("separator")}, []))
        items.append((_ID_QUIT, {"label": _s("Quit")}, []))
        return items

    @staticmethod
    def _node(item_id: int, props: dict, children: list) -> GLib.Variant:
        return GLib.Variant.new_tuple(
            GLib.Variant("i", item_id),
            GLib.Variant("a{sv}", props),
            GLib.Variant.new_array(
                GLib.VariantType.new("v"),
                [GLib.Variant.new_variant(c) for c in children]))

    def _layout(self) -> GLib.Variant:
        children = [self._node(iid, props, kids)
                    for iid, props, kids in self._flat_items()]
        root = GLib.Variant.new_tuple(
            GLib.Variant("i", 0),
            GLib.Variant("a{sv}",
                         {"children-display": _s("submenu")}),
            GLib.Variant.new_array(
                GLib.VariantType.new("v"),
                [GLib.Variant.new_variant(c) for c in children]))
        return root

    # ------------------------------------------------------- method handlers
    def _on_sni_method(self, _conn, _sender, _path, _iface, method,
                       _params, invocation) -> None:
        if method in ("Activate", "SecondaryActivate"):
            self._on_show()
        invocation.return_value(None)

    def _on_menu_method(self, _conn, _sender, _path, _iface, method,
                        params, invocation) -> None:
        if method == "GetLayout":
            invocation.return_value(GLib.Variant.new_tuple(
                GLib.Variant("u", self._revision), self._layout()))
        elif method == "GetGroupProperties":
            ids = params.unpack()[0]
            wanted = set(ids) if ids else None
            result = [(iid, props) for iid, props, _kids in self._flat_items()
                      if wanted is None or iid in wanted]
            invocation.return_value(
                GLib.Variant("(a(ia{sv}))", (result,)))
        elif method == "Event":
            item_id, event_id, _data, _ts = params.unpack()
            if event_id in ("clicked", "activated"):
                if item_id == _ID_OPEN:
                    self._on_show()
                elif item_id == _ID_QUIT:
                    self._on_quit()
            invocation.return_value(None)
        elif method == "AboutToShow":
            invocation.return_value(GLib.Variant("(b)", (False,)))
        elif method == "AboutToShowGroup":
            invocation.return_value(GLib.Variant("(aiai)", ([], [])))
        else:
            invocation.return_value(None)

    # ----------------------------------------------------- property handlers
    def _get_sni_property(self, _conn, _sender, _path, _iface, prop):
        if prop == "Category":
            return _s("ApplicationStatus")
        if prop == "Id":
            return _s("io.github.samvivan1.octopad")
        if prop == "Title":
            return _s("OctoPad")
        if prop == "Status":
            return _s("Active")
        if prop == "IconName":
            return _s(self._icon_name())
        if prop in ("IconPixmap", "OverlayIconPixmap", "AttentionIconPixmap"):
            return GLib.Variant("a(iiay)", [])
        if prop in ("OverlayIconName", "AttentionIconName",
                    "AttentionMovieName"):
            return _s("")
        if prop == "ToolTip":
            return GLib.Variant("(sa(iiay)ss)",
                                (self._icon_name(), [], "OctoPad",
                                 self._tooltip()))
        if prop == "ItemIsMenu":
            return GLib.Variant("b", False)
        if prop == "Menu":
            return GLib.Variant("o", _MENU_PATH)
        return None

    @staticmethod
    def _get_menu_property(_conn, _sender, _path, _iface, prop):
        if prop == "Version":
            return GLib.Variant("u", 3)
        if prop == "Status":
            return _s("normal")
        if prop == "TextDirection":
            return GLib.Variant("s", "ltr")
        if prop == "IconThemePath":
            return GLib.Variant("as", [])
        return None

    # --------------------------------------------------------------- events
    def _on_state_event(self, event_name: str, **_kwargs) -> None:
        if event_name in ("serial-status-changed", "ble-status-changed",
                          "mode-changed", "config-saved"):
            self.refresh()