#!/usr/bin/env python3
"""
OctoPad - Hardware Serial Event Listener & Action Dispatcher (Windows).

Monitors the USB Serial (CDC) port, parses button clicks, and executes native
Windows actions. Reconnects automatically and identifies the pad by the
``HELLO:OCTOPAD:<name>:v<version>`` line printed by the firmware.
"""

import json
import os
import re
import threading
import time
from typing import Any, Callable, Dict, List, Optional

import serial
import serial.tools.list_ports

import config as config_module
from platform_actions import execute_action

CONFIG_DIR = os.path.join(os.environ.get("APPDATA") or os.path.expanduser("~"),
                          "OctoPad")
CONFIG_FILE = os.path.join(CONFIG_DIR, "config.json")

EVENT_PATTERN = re.compile(
    r"\[(DESKTOP|HA)\]\s+Button\s+(\d+)\s+->\s+(SINGLE|DOUBLE|HOLD)",
    re.IGNORECASE)

BRAND_TAG = "OCTOPAD"
HELLO_PATTERN = re.compile(
    r"HELLO:([A-Za-z0-9_-]+):([^:\n]*)(?::v?([0-9][0-9A-Za-z._-]*))?",
    re.IGNORECASE)

PROBE_TIMEOUT_S = 2.0

# Hints that a COM port is very likely a macropad (matches firmware / board).
_VID_PID_HINTS = ("2341", "2a03", "1a86", "10c4", "239a")
_DESCRIPTION_HINTS = ("octopad", "arduino", "usb serial", "usb-serial", "ch340",
                      "cp210", "micro", "seeed", "samd")


def parse_event_line(line: str):
    """Parse a single event line (USB serial or BLE) -> dict or None."""
    match = EVENT_PATTERN.search(line)
    if not match:
        return None
    return {
        "mode": match.group(1).lower(),
        "index": int(match.group(2)) - 1,
        "trigger": match.group(3).lower(),
    }


def parse_hello(line: str) -> Optional[Dict[str, str]]:
    """Extract the pad name and firmware version from an identity line."""
    match = HELLO_PATTERN.search(line)
    if not match:
        return None
    if match.group(1).upper() != BRAND_TAG:
        return None
    return {
        "name": match.group(2).strip() or "OctoPad",
        "version": (match.group(3) or "").strip(),
    }


def parse_hello_line(line: str) -> Optional[str]:
    """Extract the device name from an identity line, else None."""
    info = parse_hello(line)
    return info["name"] if info else None


def execute_button_trigger(config: Dict[str, Any], mode: str, button_index: int,
                           trigger_event: str,
                           on_log: Optional[Callable[[str], None]] = None) -> None:
    """Execute the actions bound to a button trigger (shared by serial + BLE)."""
    mode_key = "desktop" if mode == "desktop" else "ha"
    modes_list = config.get("modes", [])
    active_mode_config = next(
        (m for m in modes_list if m.get("id") == mode_key), None)
    if not active_mode_config:
        return

    buttons = active_mode_config.get("buttons", [])
    if button_index >= len(buttons):
        return

    btn_config = buttons[button_index]
    actions = config_module.actions_list(btn_config.get(trigger_event))

    def _run_actions() -> None:
        for action in actions:
            if not isinstance(action, dict) or action.get("type") in ("none", None):
                continue
            action_type = action.get("type")
            print(f"[DAEMON] Executing action: {action_type}")
            ok, message = execute_action(action_type, action)
            log_line = f"[ACTION] Button {button_index + 1} / {trigger_event}: {message}"
            print(f"[DAEMON] {log_line}")
            if on_log:
                on_log(log_line)
            if not ok:
                break

    threading.Thread(target=_run_actions, daemon=True).start()


class SerialDaemonListener:
    def __init__(self, on_event_callback: Optional[Callable[[str], None]] = None):
        self.port: Optional[str] = None
        self.baudrate: int = 115200
        self.ser: Optional[serial.Serial] = None
        self.running: bool = False
        self.thread: Optional[threading.Thread] = None
        self.current_mode: str = "desktop"
        self.on_event_callback = on_event_callback
        self.config: Dict[str, Any] = self.load_config()
        self.last_connected_port: Optional[str] = None
        self.device_name: Optional[str] = None

    # ------------------------------------------------------------------
    def load_config(self) -> Dict[str, Any]:
        if os.path.exists(CONFIG_FILE):
            try:
                with open(CONFIG_FILE, "r", encoding="utf-8") as handle:
                    return json.load(handle)
            except Exception as exc:  # noqa: BLE001
                print(f"[DAEMON] Error reading config file: {exc}")
        return self._default_config()

    def save_config(self, new_config: Dict[str, Any]) -> bool:
        os.makedirs(CONFIG_DIR, exist_ok=True)
        try:
            with open(CONFIG_FILE, "w", encoding="utf-8") as handle:
                json.dump(new_config, handle, indent=2)
            self.config = new_config
            print("[DAEMON] Configuration saved to disk.")
            return True
        except Exception as exc:  # noqa: BLE001
            print(f"[DAEMON] Failed to save config: {exc}")
            return False

    def _default_config(self) -> Dict[str, Any]:
        return config_module.default_config()

    # ------------------------------------------------------------------
    @staticmethod
    def list_available_ports() -> List[str]:
        """Return COM ports, preferring ones that look like a macropad first."""
        try:
            ports = list(serial.tools.list_ports.comports())
        except Exception:  # noqa: BLE001
            return []

        def _score(port) -> int:
            text = f"{port.description or ''} {port.hwid or ''}".lower()
            if any(hint in text for hint in _DESCRIPTION_HINTS):
                return 0
            if any(vid in text for vid in _VID_PID_HINTS):
                return 1
            return 2

        ports.sort(key=_score)
        return [p.device for p in ports]

    def start(self, preferred_port: Optional[str] = None) -> None:
        if self.running:
            return
        self.running = True
        self.port = preferred_port
        self.thread = threading.Thread(target=self._run_loop, daemon=True)
        self.thread.start()
        print("[DAEMON] Serial listener started (scanning ports)...")

    def stop(self) -> None:
        self.running = False
        if self.ser and self.ser.is_open:
            try:
                self.ser.close()
            except Exception:  # noqa: BLE001
                pass
        self.ser = None

    def send_command(self, command: str, log: bool = True) -> bool:
        """Send a single text command to the hardware (used by the console)."""
        if not self.ser or not self.ser.is_open:
            print("[DAEMON] Serial not open — command not sent.")
            return False
        try:
            self.ser.write((command.strip() + "\n").encode("utf-8"))
            if log and self.on_event_callback:
                self.on_event_callback(f"[TX] {command.strip()}")
            return True
        except Exception as exc:  # noqa: BLE001
            print(f"[DAEMON] Failed to send command: {exc}")
            return False

    def sync_timings(self) -> bool:
        """Push the configured timing parameters to the firmware.

        The firmware treats these as authoritative once received, so the USB/BLE
        behaviour matches what the Settings page shows. Best effort: returns
        False when the port is not open or the values are invalid.
        """
        try:
            debounce = int(self.config.get("debounceMs", 25))
            click = int(self.config.get("clickTimeoutMs", 250))
            hold = int(self.config.get("holdTimeoutMs", 450))
        except (TypeError, ValueError):
            return False
        return self.send_command(f"TIMING:{debounce},{click},{hold}", log=False)

    def rename_device(self, name: str) -> bool:
        name = name.strip()
        if not name or len(name) > 24:
            return False
        if self.send_command(f"NAME:{name}"):
            self.device_name = name
            return True
        return False

    # ------------------------------------------------------------------
    def _probe_port(self, port: str) -> Optional[str]:
        """Open `port`, ask WHO, and return the device name if an OctoPad replies."""
        try:
            probe = serial.Serial(port, self.baudrate, timeout=0.3)
        except Exception:  # noqa: BLE001
            return None
        try:
            time.sleep(0.1)
            probe.reset_input_buffer()
            probe.write(b"WHO\n")
            deadline = time.time() + PROBE_TIMEOUT_S
            while time.time() < deadline:
                raw = probe.readline()
                if not raw:
                    continue
                line = raw.decode("utf-8", errors="ignore").strip()
                name = parse_hello_line(line)
                if name is not None:
                    return name
        except Exception:  # noqa: BLE001
            return None
        finally:
            try:
                probe.close()
            except Exception:  # noqa: BLE001
                pass
        return None

    def _pick_port(self, candidates: List[str]) -> Optional[str]:
        if not candidates:
            return None
        if self.last_connected_port in candidates and self.device_name:
            return self.last_connected_port
        for port in candidates:
            name = self._probe_port(port)
            if name is not None:
                self.device_name = name
                message = f'[IDENTIFIED] OctoPad "{name}" on {port}'
                print(f"[DAEMON] {message}")
                if self.on_event_callback:
                    self.on_event_callback(message)
                return port
        if len(candidates) == 1:
            return candidates[0]
        return None

    # ------------------------------------------------------------------
    def _run_loop(self) -> None:
        while self.running:
            if not self.ser or not self.ser.is_open:
                target_port = self.port
                if not target_port:
                    target_port = self._pick_port(self.list_available_ports())
                if not target_port:
                    time.sleep(2)
                    continue
                try:
                    self.ser = serial.Serial(target_port, self.baudrate, timeout=1)
                    self.last_connected_port = target_port
                    message = f"[CONNECTED] Connected to hardware port: {target_port}"
                    print(f"[DAEMON] {message}")
                    if self.on_event_callback:
                        self.on_event_callback(message)
                    try:
                        self.ser.write(b"WHO\n")
                    except Exception:  # noqa: BLE001
                        pass
                    self.sync_timings()
                except Exception:  # noqa: BLE001
                    time.sleep(2)
                    continue

            try:
                raw_line = self.ser.readline()
                if not raw_line:
                    continue
                line = raw_line.decode("utf-8", errors="ignore").strip()
                if not line:
                    continue

                if self.on_event_callback:
                    self.on_event_callback(line)

                if "MODE: HOME ASSISTANT" in line:
                    self.current_mode = "ha"
                elif "MODE: DESKTOP" in line:
                    self.current_mode = "desktop"

                hello = parse_hello_line(line)
                if hello is not None:
                    self.device_name = hello
                    continue

                event = parse_event_line(line)
                if event:
                    self._handle_hardware_trigger(
                        event["mode"], event["index"], event["trigger"])

            except Exception as exc:  # noqa: BLE001
                print(f"[DAEMON] Serial read exception: {exc}")
                if self.ser:
                    try:
                        self.ser.close()
                    except Exception:  # noqa: BLE001
                        pass
                self.ser = None
                time.sleep(1)

    def _handle_hardware_trigger(self, mode: str, button_index: int,
                                trigger_event: str) -> None:
        execute_button_trigger(self.config, mode, button_index, trigger_event,
                               on_log=self.on_event_callback)
