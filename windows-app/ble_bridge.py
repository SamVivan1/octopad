#!/usr/bin/env python3
"""
OctoPad - BLE Bridge (GATT client via the `bleak` library).

Connects to the macropad over its custom BLE service
(6f63746f-7061-4430-8001-000000000001; TX notify/read = ...0002, RX write = ...0003).
The app identifies the pad from the service UUID in the advertisement, so the
user-configured device name can be anything. All actions are executed by the
application, exactly like USB serial mode.
"""

import asyncio
import os
import re
import threading
import time
from typing import Callable, Optional, Tuple

import state
from serial_listener import execute_button_trigger, parse_event_line

SERVICE_UUID = "6f63746f-7061-4430-8001-000000000001"
TX_UUID = "6f63746f-7061-4430-8001-000000000002"
RX_UUID = "6f63746f-7061-4430-8001-000000000003"

LEGACY_SERVICE_UUID = "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
LEGACY_TX_UUID = "6e400002-b5a3-f393-e0a9-e50e24dcca9e"

BRAND_TAG = "OCTOPAD"
DEFAULT_DEVICE_NAME = "OctoPad"

_SCAN_TIMEOUT_S = 10.0
_RECONNECT_S = 3.0
_RECONNECT_MAX_S = 15.0

_LOG_PATH = os.path.join(os.environ.get("APPDATA") or os.path.expanduser("~"),
                         "OctoPad", "ble.log")
_LOG_MAX_BYTES = 256 * 1024


def _log(message: str) -> None:
    line = f"{time.strftime('%Y-%m-%d %H:%M:%S')} {message}\n"
    print(f"[BLE] {message}")
    try:
        os.makedirs(os.path.dirname(_LOG_PATH), exist_ok=True)
        if os.path.exists(_LOG_PATH) and os.path.getsize(_LOG_PATH) > _LOG_MAX_BYTES:
            os.replace(_LOG_PATH, _LOG_PATH + ".1")
        with open(_LOG_PATH, "a", encoding="utf-8") as handle:
            handle.write(line)
    except OSError:
        pass


_LINE_PATTERN = re.compile(
    r"\[(?:DESKTOP|HA)\]\s+Button\s+\d+\s+->\s+(?:SINGLE|DOUBLE|HOLD)[^\n\[]*"
    r"|MODE:\s*(?:HOME ASSISTANT|DESKTOP)"
    r"|HELLO:[^\n\[]*",
    re.IGNORECASE)

_HELLO_PATTERN = re.compile(
    r"HELLO:([A-Za-z0-9_-]+):([^:\n]*)(?::[^\n]*)?", re.IGNORECASE)


class BleBridge:
    """Background thread that bridges button events from the pad over BLE."""

    def __init__(self, on_event_callback: Optional[Callable[[str], None]] = None):
        self.on_event_callback = on_event_callback
        self._thread: Optional[threading.Thread] = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._stop_event: Optional[asyncio.Event] = None
        self._running = False
        self._client = None
        self._rx_char = None
        self._connected = False
        self._pending = ""
        self._reconnect_delay = _RECONNECT_S
        self._last_status: Optional[Tuple[bool, str]] = None
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
        loop = self._loop
        stop_event = self._stop_event
        if loop is not None and loop.is_running():
            try:
                loop.call_soon_threadsafe(self._signal_stop)
            except Exception:  # noqa: BLE001
                pass
        _ = stop_event

    def _signal_stop(self) -> None:
        if self._stop_event is not None:
            self._stop_event.set()

    def _set_status(self, connected: bool, message: str) -> None:
        if (connected, message) != self._last_status:
            self._last_status = (connected, message)
            _log(f"status connected={connected} message={message!r}")
        state.set_ble_status(connected, message)

    # ---------------------------------------------------------------- thread
    def _thread_main(self) -> None:
        loop = asyncio.new_event_loop()
        self._loop = loop
        asyncio.set_event_loop(loop)
        try:
            loop.run_until_complete(self._main())
        except Exception as exc:  # noqa: BLE001
            _log(f"bridge stopped: {exc}")
        finally:
            try:
                loop.close()
            except Exception:  # noqa: BLE001
                pass

    async def _main(self) -> None:
        self._stop_event = asyncio.Event()
        while self._running and not self._stop_event.is_set():
            self._set_status(False, "Searching for BLE pad...")
            device = await self._find_device()
            if device is None:
                await self._backoff()
                continue
            try:
                await self._connect(device)
                self._reconnect_delay = _RECONNECT_S
                self._set_status(True, "Connected via Bluetooth")
                await self._stop_event.wait()
            except Exception as exc:  # noqa: BLE001
                _log(f"connect attempt failed: {exc}")
                self._set_status(False, "Pad disconnected - retrying...")
                await self._backoff()

    async def _backoff(self) -> None:
        delay = self._reconnect_delay
        self._reconnect_delay = min(self._reconnect_delay * 2, _RECONNECT_MAX_S)
        _log(f"reconnect scheduled in {delay:.1f}s")
        try:
            await asyncio.wait_for(self._stop_event.wait(), timeout=delay)
        except asyncio.TimeoutError:
            pass

    # ---------------------------------------------------------------- bleak
    async def _find_device(self):
        from bleak import BleakScanner

        target_uuids = {SERVICE_UUID.lower(), LEGACY_SERVICE_UUID.lower()}
        try:
            found = await BleakScanner.discover(
                timeout=_SCAN_TIMEOUT_S,
                service_uuids=[SERVICE_UUID, LEGACY_SERVICE_UUID])
        except Exception as exc:  # noqa: BLE001
            _log(f"scan failed: {exc}")
            found = []

        for device in found:
            if self._is_pad(device, target_uuids):
                _log(f"found pad: {device.name} [{device.address}]")
                return device

        # Some adapters do not surface advertised service UUIDs; fall back to a
        # name match over a plain scan.
        try:
            found = await BleakScanner.discover(timeout=_SCAN_TIMEOUT_S)
        except Exception:  # noqa: BLE001
            return None
        for device in found:
            if self._is_pad(device, target_uuids):
                return device
        return None

    @staticmethod
    def _is_pad(device, target_uuids) -> bool:
        uuids = getattr(device, "metadata", {}).get("uuids") or []
        uuids = {str(u).lower() for u in uuids}
        if uuids & target_uuids:
            return True
        name = (device.name or "").strip()
        return name.casefold() == DEFAULT_DEVICE_NAME.casefold()

    async def _connect(self, device) -> None:
        from bleak import BleakClient

        def _on_disconnect(_client) -> None:
            if self._connected:
                _log("link dropped - will retry")

        client = BleakClient(device, disconnected_callback=_on_disconnect)
        await client.connect()
        self._client = client
        _log(f"connected device={device.address}")

        characteristic = self._find_tx_characteristic(client)
        if characteristic is None:
            await client.disconnect()
            raise RuntimeError("BLE service missing")

        try:
            raw = await client.read_gatt_char(characteristic)
            for segment in bytes(raw).split(b"\n"):
                line = segment.decode("utf-8", errors="ignore").strip().strip("\x00")
                if line:
                    self._feed_line(line)
        except Exception as exc:  # noqa: BLE001
            _log(f"TX read failed: {exc}")

        self._pending = ""
        self._rx_char = self._find_rx_characteristic(client)
        await client.start_notify(characteristic, self._on_notify)
        self._connected = True
        # Push the configured timing parameters so the wireless pad matches the
        # app's Settings page (no-op if the firmware is too old for TIMING).
        command = self._timing_command()
        if command:
            await self._write(command, log=False)

    def _find_tx_characteristic(self, client):
        wanted = {TX_UUID.lower(), LEGACY_TX_UUID.lower()}
        try:
            for service in client.services:
                for characteristic in service.characteristics:
                    if str(characteristic.uuid).lower() in wanted:
                        return characteristic
        except Exception as exc:  # noqa: BLE001
            _log(f"service discovery failed: {exc}")
        return None

    def _find_rx_characteristic(self, client):
        try:
            for service in client.services:
                for characteristic in service.characteristics:
                    if str(characteristic.uuid).lower() == RX_UUID.lower():
                        return characteristic
        except Exception as exc:  # noqa: BLE001
            _log(f"RX discovery failed: {exc}")
        return None

    @staticmethod
    def _timing_command() -> Optional[str]:
        cfg = getattr(state, "cfg", None) or {}
        try:
            debounce = int(cfg.get("debounceMs", 25))
            click = int(cfg.get("clickTimeoutMs", 250))
            hold = int(cfg.get("holdTimeoutMs", 450))
        except (TypeError, ValueError):
            return None
        return f"TIMING:{debounce},{click},{hold}"

    async def _write(self, command: str, log: bool = True) -> None:
        client = self._client
        if client is None or self._rx_char is None:
            return
        payload = (command.strip() + "\n").encode("utf-8")
        try:
            await client.write_gatt_char(self._rx_char, payload, response=False)
            if log:
                state.emit("serial-log", line=f"[TX-BLE] {command.strip()}")
        except Exception as exc:  # noqa: BLE001
            _log(f"BLE write failed: {exc}")

    def send_command(self, command: str, log: bool = True) -> bool:
        """Queue a command to the pad over Bluetooth (thread-safe)."""
        loop = self._loop
        if not self._running or not self._connected or self._rx_char is None or loop is None:
            return False
        try:
            asyncio.run_coroutine_threadsafe(self._write(command, log=log), loop)
            return True
        except Exception as exc:  # noqa: BLE001
            _log(f"BLE write scheduling failed: {exc}")
            return False

    def sync_timings(self) -> bool:
        """Push the configured timing parameters to the wireless pad."""
        command = self._timing_command()
        if not command:
            return False
        return self.send_command(command, log=False)

    def _on_notify(self, _characteristic, data: bytes) -> None:
        self._pending += bytes(data).decode("utf-8", errors="ignore")
        events, self._pending = self._split_events(self._pending)
        for event in events:
            self._feed_line(event)

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

    def _feed_line(self, line: str) -> None:
        if self.on_event_callback is not None:
            self.on_event_callback(line)
        else:
            state.emit("serial-log", line=line)

        match = _HELLO_PATTERN.search(line)
        if match and match.group(1).upper() == BRAND_TAG:
            state.set_device_name(match.group(2).strip() or DEFAULT_DEVICE_NAME)
            return

        event = parse_event_line(line)
        if event is None:
            return

        def log_line(message: str) -> None:
            state.emit("serial-log", line=message)

        execute_button_trigger(state.cfg, event["mode"], event["index"],
                               event["trigger"], on_log=log_line)
