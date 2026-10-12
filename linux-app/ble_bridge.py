#!/usr/bin/env python3
"""
OctoPad - BLE Bridge (GATT client via bluez D-Bus).

Connects to the macropad over its custom BLE service
(6f63746f-7061-4430-8001-000000000001; TX notify/read = ...0002, RX write = ...0003).
The application identifies the pad from the service UUID in the advertisement,
so the user-configured device name can be anything.
All actions are still executed by the application, exactly like USB serial mode.

No extra Python library is used: Gio is already available because of GTK4.
"""

import os
import re
import threading
import time
from typing import Any, Callable, Dict, Optional

import gi
gi.require_version("Gio", "2.0")
gi.require_version("GLib", "2.0")
from gi.repository import Gio, GLib

import config as config_module
import state
from serial_listener import (execute_button_trigger, parse_event_line,
                             parse_hello)

SERVICE_UUID = "6f63746f-7061-4430-8001-000000000001"
TX_UUID = "6f63746f-7061-4430-8001-000000000002"
RX_UUID = "6f63746f-7061-4430-8001-000000000003"

# Firmware before the rebrand advertised the Nordic-UART UUIDs. Accept them too
# so existing hand-built pads keep working after an app update.
LEGACY_SERVICE_UUID = "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
LEGACY_TX_UUID = "6e400003-b5a3-f393-e0a9-e50e24dcca9e"
LEGACY_RX_UUID = "6e400002-b5a3-f393-e0a9-e50e24dcca9e"

# Default device name used when the user has not renamed their pad.
DEFAULT_DEVICE_NAME = "OctoPad"

_BLUEZ = "org.bluez"
_OBJMAN = "org.freedesktop.DBus.ObjectManager"
_PROPS = "org.freedesktop.DBus.Properties"
_DEV = "org.bluez.Device1"
_ADAPTER = "org.bluez.Adapter1"
_GSVC = "org.bluez.GattService1"
_GCHAR = "org.bluez.GattCharacteristic1"

_SCAN_TIMEOUT_S = 10.0
_RECONNECT_S = 3.0
_RECONNECT_MAX_S = 15.0
_SERVICE_READY_S = 5.0

# Persistent BLE log so we can tell an app bug apart from a flaky BLE link.
_LOG_PATH = os.path.expanduser("~/.config/octopad/ble.log")
_LOG_MAX_BYTES = 256 * 1024


def _log(message: str) -> None:
    line = f"{time.strftime('%Y-%m-%d %H:%M:%S')} {message}\n"
    print(f"[BLE] {message}")
    try:
        os.makedirs(os.path.dirname(_LOG_PATH), exist_ok=True)
        if os.path.exists(_LOG_PATH) and os.path.getsize(_LOG_PATH) > _LOG_MAX_BYTES:
            os.replace(_LOG_PATH, _LOG_PATH + ".1")
        with open(_LOG_PATH, "a", encoding="utf-8") as fh:
            fh.write(line)
    except OSError:
        pass

# Complete event lines emitted by the firmware, used to split a byte stream
# that may carry several notifications without a newline delimiter.
_LINE_PATTERN = re.compile(
    r'\[(?:DESKTOP|HA)\]\s+Button\s+\d+\s+->\s+(?:SINGLE|DOUBLE|HOLD)'
    r'[^\n[]*'
    r'|MODE:\s*(?:HOME ASSISTANT|DESKTOP)'
    r'|HELLO:[^\n[]*',
    re.IGNORECASE)


class BleBridge:
    """D-Bus thread that bridges button-click events from the pad over BLE."""

    def __init__(self, on_event_callback: Optional[Callable[[str], None]] = None):
        self.on_event_callback = on_event_callback
        self._thread: Optional[threading.Thread] = None
        self._running = False
        self._ctx: Optional[GLib.MainContext] = None
        self._loop: Optional[GLib.MainLoop] = None
        self._bus: Optional[Gio.DBusConnection] = None
        self._device_path: Optional[str] = None
        self._tx_path: Optional[str] = None
        self._rx_path: Optional[str] = None
        self._notify_fd: Optional[int] = None
        self._pending = ""
        self._connected = False
        self._watch_source = None
        self._reconnect_source = None
        self._reconnect_delay = _RECONNECT_S
        self._last_status = None
        self.current_mode = "desktop"

    # ------------------------------------------------------------------ API
    def start(self) -> None:
        if self._running:
            return
        self._running = True
        self._thread = threading.Thread(target=self._thread_main,
                                        name="ble-bridge", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._running = False
        self._close_notify()
        self._cancel_reconnect()
        if self._loop is not None and self._loop.is_running():
            try:
                self._loop.quit()
            except Exception as exc:  # noqa: BLE001
                _log(f"Failed to stop loop: {exc}")

    def send_command(self, command: str, log: bool = True) -> bool:
        """Write a text command to the pad over Bluetooth (RX characteristic).

        Mirrors ``SerialDaemonListener.send_command`` so the state layer can use
        either transport transparently.
        """
        if not self._connected or not self._rx_path or self._bus is None:
            return False
        payload = (command.strip() + "\n").encode("utf-8")
        try:
            self._call(self._rx_path, _GCHAR, "WriteValue",
                       GLib.Variant("(aya{sv})", (payload, {})), timeout_ms=5000)
            if log:
                state.emit("serial-log", line=f"[TX-BLE] {command.strip()}")
            return True
        except Exception as exc:  # noqa: BLE001
            _log(f"BLE write failed: {exc}")
            return False

    def sync_timings(self) -> bool:
        """Push the configured timing parameters to the wireless pad."""
        command = config_module.timing_command(getattr(state, "cfg", None) or {})
        if command is None:
            return False
        return self.send_command(command, log=False)

    def _set_status(self, connected: bool, message: str) -> None:
        """Update UI status, logging only when it actually changes."""
        if (connected, message) != self._last_status:
            self._last_status = (connected, message)
            _log(f"status connected={connected} message={message!r}")
        state.set_ble_status(connected, message)

    # ---------------------------------------------------------------- thread
    def _thread_main(self) -> None:
        self._ctx = GLib.MainContext.new()
        self._ctx.push_thread_default()
        try:
            self._bus = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
        except Exception as exc:  # noqa: BLE001
            _log(f"Cannot access system D-Bus (bluez?): {exc}")
            self._set_status(False, "Cannot access system D-Bus (bluez?)")
            self._ctx.pop_thread_default()
            return

        self._bus.signal_subscribe(
            _BLUEZ, _PROPS, "PropertiesChanged", None, None,
            Gio.DBusSignalFlags.NONE, self._on_props_changed)

        self._loop = GLib.MainLoop.new(self._ctx, False)
        self._tick_connect()
        try:
            self._loop.run()
        finally:
            if self._bus is not None:
                try:
                    self._bus.close()
                except Exception:  # noqa: BLE001
                    pass
            self._ctx.pop_thread_default()

    def _schedule_reconnect(self) -> None:
        if not (self._running and self._ctx is not None):
            return
        if self._reconnect_source is not None:
            return  # a reconnect is already pending — never stack timers
        delay = self._reconnect_delay
        _log(f"reconnect scheduled in {delay:.1f}s")
        # Attach to *our* main context: GLib.timeout_add() would target the
        # process-wide default context and never fire inside this thread.
        source = GLib.timeout_source_new(delay * 1000)
        source.set_callback(self._on_reconnect_timeout)
        source.attach(self._ctx)
        self._reconnect_source = source

    def _on_reconnect_timeout(self) -> bool:
        self._reconnect_source = None
        self._tick_connect()
        return False

    def _cancel_reconnect(self) -> None:
        if self._reconnect_source is not None:
            try:
                self._reconnect_source.destroy()
            except Exception:  # noqa: BLE001
                pass
            self._reconnect_source = None

    def _tick_connect(self) -> None:
        if not self._running or self._connected:
            return
        self._set_status(False, "Searching for BLE pad…")
        try:
            self._try_connect()
        except Exception as exc:  # noqa: BLE001
            _log(f"connect attempt failed: {exc}")
        if self._connected:
            return
        # Exponential backoff (capped) so a flaky link is not hammered.
        self._reconnect_delay = min(self._reconnect_delay * 2, _RECONNECT_MAX_S)
        self._schedule_reconnect()

    # ------------------------------------------------------------- bluez ops
    def _managed_objects(self) -> Dict[str, Dict[str, Dict[str, Any]]]:
        res = self._bus.call_sync(
            _BLUEZ, "/", _OBJMAN, "GetManagedObjects",
            GLib.Variant("()", ()),
            GLib.VariantType("(a{oa{sa{sv}}})"),
            Gio.DBusCallFlags.NONE, 15000, None)
        return res.unpack()[0]

    def _call(self, path: str, iface: str, member: str, in_variant: GLib.Variant,
              timeout_ms: int = 10000) -> Any:
        return self._bus.call_sync(
            _BLUEZ, path, iface, member, in_variant, None,
            Gio.DBusCallFlags.NONE, timeout_ms, None)

    def _try_connect(self) -> None:
        if self._bus is None:
            return
        objects = self._managed_objects()

        # 1. Find an adapter (usually /org/bluez/hci0) and make sure it is powered.
        adapter = None
        for path, ifaces in objects.items():
            if path.startswith("/org/bluez/") and _ADAPTER in ifaces:
                adapter = path
                break
        if not adapter:
            self._set_status(False, "Bluetooth adapter not found")
            return
        self._ensure_powered(adapter)

        # 2. Find the device advertising our OctoPad service UUID.
        device_path = self._find_device_path(objects, scan=True, adapter=adapter)
        if not device_path:
            self._set_status(False,
                             "BLE pad not found — make sure the pad is on")
            return
        self._device_path = device_path

        # 3. Connect if not already connected (and wait for GATT resolution).
        self._ensure_connected(device_path)

        # 4. Re-read the object tree now that GATT may have been resolved:
        #    the snapshot from step 1 predates the connection and often lacks
        #    the service/characteristic objects.
        try:
            objects = self._managed_objects()
        except Exception as exc:  # noqa: BLE001
            _log(f"Failed to refresh objects: {exc}")

        # 5. Find the TX characteristic on the NUS service.
        tx_path = self._find_tx_path(objects, device_path)
        if not tx_path:
            # Keep retrying instead of latching a broken "connected" state.
            self._connected = False
            self._set_status(
                False,
                "Pad connected but BLE service missing — retrying…")
            return
        self._tx_path = tx_path

        # Locate the RX (write) characteristic as well so commands can be sent
        # over Bluetooth (best effort: a pad without an RX char still works).
        self._rx_path = self._find_rx_path(objects, device_path)

        # 6. Read the TX value: the firmware exposes its identity line
        #    (HELLO:OCTOPAD:<name>:v<ver>) there, so we learn the pad's
        #    user-configured name as soon as we connect.
        self._read_tx_value(tx_path)

        # 7. Subscribe to notifications. BlueZ (>= 5.85) does not emit
        #    PropertiesChanged for GATT values, so AcquireNotify — which
        #    streams into a file descriptor — is the reliable path.
        if not self._start_notify(tx_path):
            self._connected = False
            return
        self._connected = True
        self._reconnect_delay = _RECONNECT_S
        _log(f"connected device={device_path} tx={tx_path} rx={self._rx_path}")
        self._set_status(True, "Connected via Bluetooth")
        # Push the configured timing parameters so the wireless pad matches the
        # app's Settings page (no-op if the firmware is too old for TIMING).
        self.sync_timings()

    def _read_tx_value(self, tx_path: str) -> None:
        """Read the TX characteristic once to fetch the identity (HELLO) line."""
        try:
            res = self._call(tx_path, _GCHAR, "ReadValue",
                             GLib.Variant("(a{sv})", ({},)), timeout_ms=5000)
            raw = bytes(res.unpack()[0])
        except Exception as exc:  # noqa: BLE001
            _log(f"TX read failed: {exc}")
            return
        for segment in raw.split(b"\n"):
            line = segment.decode("utf-8", errors="ignore").strip().strip("\x00")
            if line:
                self._feed_line(line)

    def _start_notify(self, tx_path: str) -> bool:
        try:
            res, fd_list = self._bus.call_with_unix_fd_list_sync(
                _BLUEZ, tx_path, _GCHAR, "AcquireNotify",
                GLib.Variant("(a{sv})", ({},)),
                GLib.VariantType("(hq)"),
                Gio.DBusCallFlags.NONE, 5000, None, None)
            fd = fd_list.get(res.get_child_value(0).get_handle())
            os.set_blocking(fd, False)
            source = GLib.unix_fd_source_new(
                fd, GLib.IOCondition.IN | GLib.IOCondition.HUP
                | GLib.IOCondition.ERR)
            source.set_callback(self._on_notify_fd)
            source.attach(self._ctx)
            self._watch_source = source
            self._notify_fd = fd
            self._pending = ""
            _log("Notifying via AcquireNotify (fd)")
            return True
        except Exception as exc:  # noqa: BLE001
            _log(f"AcquireNotify unavailable ({exc}); falling back to StartNotify")
        try:
            self._call(tx_path, _GCHAR, "StartNotify", GLib.Variant("()", ()),
                       timeout_ms=5000)
            _log("Notifying via StartNotify")
            return True
        except Exception as exc:  # noqa: BLE001
            _log(f"StartNotify failed: {exc}")
            return False

    def _close_notify(self) -> None:
        if self._watch_source is not None:
            try:
                self._watch_source.destroy()
            except Exception:  # noqa: BLE001
                pass
            self._watch_source = None
        fd = self._notify_fd
        self._notify_fd = None
        if fd is not None:
            try:
                os.close(fd)
            except OSError:
                pass
        self._pending = ""

    def _on_notify_fd(self, *_args) -> bool:
        """GLib watch on the notify fd (runs inside the bridge main loop)."""
        fd = self._notify_fd
        if fd is None:
            return False
        try:
            data = os.read(fd, 4096)
        except BlockingIOError:
            return True
        except OSError:
            self._handle_disconnect()
            return False
        if not data:
            self._handle_disconnect()
            return False
        self._pending += data.decode("utf-8", errors="ignore")
        events, self._pending = self._split_events(self._pending)
        for event in events:
            self._feed_line(event)
        return True

    @staticmethod
    def _split_events(text: str):
        """Return (complete_events, remainder) from a possibly partial stream."""
        events = []
        last_end = 0
        for match in _LINE_PATTERN.finditer(text):
            events.append(match.group(0).strip())
            last_end = match.end()
        if not events:
            return [], text
        return events, text[last_end:]

    def _handle_disconnect(self) -> None:
        was_connected = self._connected
        self._close_notify()
        self._connected = False
        self._rx_path = None
        if was_connected:
            _log("link dropped — will retry")
            self._set_status(False, "Pad disconnected — retrying…")
        self._schedule_reconnect()

    def _ensure_powered(self, adapter: str) -> None:
        try:
            self._call(adapter, _PROPS, "Set",
                       GLib.Variant("(ssv)", (_ADAPTER, "Powered",
                                              GLib.Variant("b", True))),
                       timeout_ms=8000)
        except Exception:  # noqa: BLE001
            pass

    def _is_pad(self, device: Dict[str, Any]) -> bool:
        """True if a BlueZ device entry is our macropad.

        The advertised service UUID is unique to the project, so it is the
        primary signal (works whatever the user named their pad). The legacy
        Nordic-UART UUID and the default name are kept as fallbacks.
        """
        uuids = device.get("UUIDs") or []
        if SERVICE_UUID in uuids or LEGACY_SERVICE_UUID in uuids:
            return True
        name = (device.get("Name") or device.get("Alias") or "").strip()
        return name.casefold() == DEFAULT_DEVICE_NAME.casefold()

    def _find_device_path(self, objects, scan: bool,
                          adapter: str = "/org/bluez/hci0") -> Optional[str]:
        for path, ifaces in objects.items():
            device = ifaces.get(_DEV)
            if device and self._is_pad(device):
                return path

        if not scan:
            return None

        we_started = False
        try:
            self._call(adapter, _ADAPTER, "StartDiscovery",
                       GLib.Variant("()", ()), timeout_ms=5000)
            we_started = True
            _log("discovery started")
        except Exception:  # noqa: BLE001
            pass  # already discovering (started by someone else) — leave it

        try:
            deadline = time.monotonic() + _SCAN_TIMEOUT_S
            while time.monotonic() < deadline:
                try:
                    objs = self._managed_objects()
                    for path, ifaces in objs.items():
                        device = ifaces.get(_DEV)
                        if device and self._is_pad(device):
                            return path
                except Exception:  # noqa: BLE001
                    pass
                time.sleep(0.4)
            return None
        finally:
            # Always stop discovery we started: a lingering scan destabilises
            # an active BLE link and causes connect/disconnect flapping.
            if we_started:
                try:
                    self._call(adapter, _ADAPTER, "StopDiscovery",
                               GLib.Variant("()", ()), timeout_ms=5000)
                    _log("discovery stopped")
                except Exception:  # noqa: BLE001
                    pass

    def _ensure_connected(self, device_path: str) -> None:
        props = self._device_props(device_path)
        if not (props and props.get("Connected")):
            try:
                _log("connecting device…")
                self._call(device_path, _DEV, "Connect",
                           GLib.Variant("()", ()), timeout_ms=20000)
            except Exception as exc:  # noqa: BLE001
                # A timeout here is common when BlueZ is auto-reconnecting a
                # trusted device in parallel; the link may still be up, so
                # re-check before treating it as a hard failure.
                props = self._device_props(device_path)
                if not (props and props.get("Connected")):
                    _log(f"Connect failed: {exc}")
                    raise

        # Wait for the service to finish resolving (GATT discovery).
        deadline = time.monotonic() + _SERVICE_READY_S
        while time.monotonic() < deadline:
            props = self._device_props(device_path)
            if props and props.get("ServicesResolved"):
                return
            time.sleep(0.3)

    def _device_props(self, device_path: str) -> Dict[str, Any]:
        try:
            res = self._call(device_path, _PROPS, "GetAll",
                             GLib.Variant("(s)", (_DEV,)), timeout_ms=3000)
            return res.unpack()[0]
        except Exception:  # noqa: BLE001
            return {}

    def _find_rx_path(self, objects, device_path: str) -> Optional[str]:
        """Locate the writable (RX) characteristic of our service, if present."""
        for path, ifaces in objects.items():
            char = ifaces.get(_GCHAR)
            if not char or char.get("UUID") not in (RX_UUID, LEGACY_RX_UUID):
                continue
            service_path = path.rsplit("/", 1)[0]
            service = objects.get(service_path, {}).get(_GSVC)
            if service and service.get("UUID") in (SERVICE_UUID, LEGACY_SERVICE_UUID):
                return path
        return None

    def _find_tx_path(self, objects, device_path: str) -> Optional[str]:
        service_path = None
        for path, ifaces in objects.items():
            svc = ifaces.get(_GSVC)
            if svc and svc.get("UUID") in (SERVICE_UUID, LEGACY_SERVICE_UUID) and \
                    path.startswith(device_path + "/"):
                service_path = path
                break
        if not service_path:
            return None
        for path, ifaces in objects.items():
            char = ifaces.get(_GCHAR)
            if char and char.get("UUID") in (TX_UUID, LEGACY_TX_UUID) and \
                    path.startswith(service_path + "/"):
                return path
        return None

    # ------------------------------------------------------------- signals
    def _on_props_changed(self, connection, sender_name, object_path,
                          interface_name, signal_name, parameters) -> None:
        if interface_name == _GCHAR and object_path == self._tx_path:
            props, _, _ = parameters.unpack()
            value = props.get("Value")
            if value is None:
                return
            try:
                raw = bytes(value)
            except Exception:  # noqa: BLE001
                return
            for segment in raw.split(b"\n"):
                line = segment.decode("utf-8", errors="ignore").strip().strip("\x00")
                if line:
                    self._feed_line(line)
        elif interface_name == _DEV and object_path == self._device_path:
            props, _, _ = parameters.unpack()
            if props.get("Connected") is False:
                self._handle_disconnect()

    def _feed_line(self, line: str) -> None:
        """Process a single BLE event line: log/UI + execute the action."""
        # The shared serial handler owns logging, mode switching and the
        # hardware-event signal (state.emit is thread-safe: it marshals UI
        # callbacks onto the GTK main loop).
        if self.on_event_callback is not None:
            self.on_event_callback(line)
        else:
            state.emit("serial-log", line=line)

        hello = parse_hello(line)
        if hello is not None:
            state.set_device_name(hello["name"] or DEFAULT_DEVICE_NAME)
            state.set_firmware_version(hello["version"])
            return

        event = parse_event_line(line)
        if event is None:
            return

        def log_line(msg: str) -> None:
            state.emit("serial-log", line=msg)

        execute_button_trigger(state.cfg, event["mode"], event["index"],
                               event["trigger"], on_log=log_line)