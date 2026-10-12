#!/usr/bin/env python3
"""
OctoPad - Hardware Serial Event Listener & Action Dispatcher
Monitors USB Serial port, parses button clicks, and executes native Ubuntu actions.
"""

import time
import threading
import glob
import re
import os
import json
import serial
from typing import Optional, Callable, Dict, Any

from system_actions import execute_action
import config as config_module

CONFIG_DIR = os.path.expanduser('~/.config/octopad')
CONFIG_FILE = os.path.join(CONFIG_DIR, 'config.json')

EVENT_PATTERN = re.compile(
    r'\[(DESKTOP|HA)\]\s+Button\s+(\d+)\s+->\s+(SINGLE|DOUBLE|HOLD)',
    re.IGNORECASE)

# Identity line printed by the firmware: HELLO:OCTOPAD:<device-name>:v<version>
BRAND_TAG = "OCTOPAD"
HELLO_PATTERN = re.compile(
    r'HELLO:([A-Za-z0-9_-]+):([^:\n]*)(?::[^\n]*)?', re.IGNORECASE)

# How long to wait for a HELLO reply when probing a serial port.
PROBE_TIMEOUT_S = 2.0


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


def parse_hello_line(line: str) -> Optional[str]:
    """Return the device name from a firmware identity line, else None.

    The firmware prints/broadcasts ``HELLO:OCTOPAD:<device-name>:v<version>``;
    the brand tag confirms the sender is a genuine OctoPad regardless of the
    user-configured name.
    """
    match = HELLO_PATTERN.search(line)
    if not match or match.group(1).upper() != BRAND_TAG:
        return None
    return match.group(2).strip() or "OctoPad"


def execute_button_trigger(config: Dict[str, Any],
                           mode: str,
                           button_index: int,
                           trigger_event: str,
                           on_log: Optional[Callable[[str], None]] = None):
    """Run the actions for (mode, button, trigger) from the config.

    Shared by the USB serial listener and the BLE bridge so that the
    cable / Bluetooth always execute actions in the same way.
    `on_log` is called for each action log line (if any).
    """
    mode_key = "desktop" if mode == "desktop" else "ha"
    modes_list = config.get("modes", [])
    active_mode_config = next((m for m in modes_list if m.get("id") == mode_key), None)
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
            print(f"[DAEMON] Executing action for Button {button_index + 1} "
                  f"({trigger_event}) -> Type: {action_type}")
            ok, reason = execute_action(action_type, action)
            log_msg = (f"[ACTION] B{button_index + 1} ({trigger_event.upper()}): "
                       f"{reason} [{'OK' if ok else 'FAIL'}]")
            print(f"[DAEMON] {log_msg}")
            if on_log is not None:
                on_log(log_msg)

    # Run the sequence off the GTK main loop: BLE events arrive on the main
    # thread and a `delay` action must not freeze the UI.
    threading.Thread(target=_run_actions, daemon=True,
                     name="macropad-actions").start()


class SerialDaemonListener:
    def __init__(self, on_event_callback: Optional[Callable[[str], None]] = None):
        self.port: Optional[str] = None
        self.baudrate: int = 115200
        self.ser: Optional[serial.Serial] = None
        self.running: bool = False
        self.thread: Optional[threading.Thread] = None
        self.current_mode: str = "desktop"  # "desktop" or "ha"
        self.on_event_callback = on_event_callback
        self.config: Dict[str, Any] = self.load_config()
        self.last_connected_port: Optional[str] = None
        self.device_name: Optional[str] = None

    def load_config(self) -> Dict[str, Any]:
        """Load button-to-action configuration from user's config directory."""
        if os.path.exists(CONFIG_FILE):
            try:
                with open(CONFIG_FILE, 'r', encoding='utf-8') as f:
                    return json.load(f)
            except Exception as e:
                print(f"[DAEMON] Error reading config file: {e}")
        return self._default_config()

    def save_config(self, new_config: Dict[str, Any]) -> bool:
        """Save updated action configuration."""
        os.makedirs(CONFIG_DIR, exist_ok=True)
        try:
            with open(CONFIG_FILE, 'w', encoding='utf-8') as f:
                json.dump(new_config, f, indent=2)
            self.config = new_config
            print("[DAEMON] Configuration successfully saved to disk.")
            return True
        except Exception as e:
            print(f"[DAEMON] Failed to save config: {e}")
            return False

    def _default_config(self) -> Dict[str, Any]:
        """Default fallback configuration."""
        return {
            "version": 2.5,
            "modes": [
                {
                    "id": "desktop",
                    "buttons": [
                        {"index": 0, "single": {"type": "mode_toggle"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 1, "single": {"type": "system_action", "presetId": "open_terminal"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 2, "single": {"type": "system_action", "presetId": "screenshot_interactive"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 3, "single": {"type": "system_action", "presetId": "mic_toggle_mute"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 4, "single": {"type": "system_action", "presetId": "volume_up_5"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 5, "single": {"type": "system_action", "presetId": "volume_down_5"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 6, "single": {"type": "system_action", "presetId": "lock_screen"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 7, "single": {"type": "none"}, "double": {"type": "none"}, "hold": {"type": "none"}}
                    ]
                },
                {
                    "id": "ha",
                    "buttons": [
                        {"index": 0, "single": {"type": "mode_toggle"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 1, "single": {"type": "none"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 2, "single": {"type": "none"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 3, "single": {"type": "none"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 4, "single": {"type": "none"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 5, "single": {"type": "none"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 6, "single": {"type": "none"}, "double": {"type": "none"}, "hold": {"type": "none"}},
                        {"index": 7, "single": {"type": "none"}, "double": {"type": "none"}, "hold": {"type": "none"}}
                    ]
                }
            ]
        }

    @staticmethod
    def list_available_ports():
        """Find active serial devices (USB or ACM)."""
        ports = glob.glob('/dev/ttyUSB*') + glob.glob('/dev/ttyACM*')
        return sorted(ports)

    def start(self, preferred_port: Optional[str] = None):
        """Start background worker thread."""
        if self.running:
            return
        self.running = True
        self.port = preferred_port
        self.thread = threading.Thread(target=self._run_loop, daemon=True)
        self.thread.start()
        print("[DAEMON] Serial background listener started (searching ports)...")

    def stop(self):
        """Stop background worker thread."""
        self.running = False
        if self.ser and self.ser.is_open:
            try:
                self.ser.close()
            except Exception:
                pass
        self.ser = None

    def send_command(self, command: str, log: bool = True) -> bool:
        """Send a single line of text command to the hardware (used by the UI console)."""
        if not self.ser or not self.ser.is_open:
            print("[DAEMON] [ERROR] Serial not open — command not sent.")
            return False
        try:
            payload = (command.strip() + "\n").encode("utf-8")
            self.ser.write(payload)
            if log and self.on_event_callback:
                self.on_event_callback(f"[TX] {command.strip()}")
            return True
        except Exception as e:
            print(f"[DAEMON] [ERROR] Failed to send command: {e}")
            return False

    def rename_device(self, name: str) -> bool:
        """Ask the firmware to adopt `name` as its device name (over serial)."""
        name = name.strip()
        if not name or len(name) > 24:
            return False
        if self.send_command(f"NAME:{name}"):
            self.device_name = name
            return True
        return False

    def _probe_port(self, port: str) -> Optional[str]:
        """Open `port`, ask the pad to identify itself, and return its name.

        Returns the device name if an OctoPad answered, else None. Uses a short
        timeout so scanning several ports stays quick.
        """
        try:
            probe = serial.Serial(port, self.baudrate, timeout=0.3)
        except Exception:
            return None
        try:
            time.sleep(0.1)
            probe.reset_input_buffer()
            probe.write(b"WHO\n")
            deadline = time.time() + 2.0
            while time.time() < deadline:
                raw = probe.readline()
                if not raw:
                    continue
                line = raw.decode('utf-8', errors='ignore').strip()
                name = parse_hello_line(line)
                if name is not None:
                    return name
        except Exception:
            return None
        finally:
            try:
                probe.close()
            except Exception:
                pass
        return None

    def _pick_port(self, candidates: list) -> Optional[str]:
        """Choose the serial port belonging to an OctoPad.

        Prefers the port that answers our identity handshake. If none answer
        but there is exactly one candidate, it is used anyway (the firmware may
        be an older build without the handshake), so single-device setups keep
        working. With several devices and no reply, nothing is picked.
        """
        if not candidates:
            return None
        if self.last_connected_port in candidates and self.device_name:
            return self.last_connected_port
        for port in candidates:
            name = self._probe_port(port)
            if name is not None:
                self.device_name = name
                msg = f"[IDENTIFIED] OctoPad “{name}” on {port}"
                print(f"[DAEMON] {msg}")
                if self.on_event_callback:
                    self.on_event_callback(msg)
                return port
        if len(candidates) == 1:
            return candidates[0]
        return None

    def _run_loop(self):
        """Continuously listen to serial input and auto-reconnect."""
        while self.running:
            # 1. Connect if not open
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
                    msg = f"[CONNECTED] Connected to hardware port: {target_port}"
                    print(f"[DAEMON] {msg}")
                    if self.on_event_callback:
                        self.on_event_callback(msg)
                    # Learn/confirm the device name (harmless if already known).
                    try:
                        self.ser.write(b"WHO\n")
                    except Exception:
                        pass
                except Exception:
                    time.sleep(2)
                    continue

            # 2. Read incoming line
            try:
                raw_line = self.ser.readline()
                if not raw_line:
                    continue
                line = raw_line.decode('utf-8', errors='ignore').strip()
                if not line:
                    continue

                if self.on_event_callback:
                    self.on_event_callback(line)

                # Track mode transitions
                if "MODE: HOME ASSISTANT" in line:
                    self.current_mode = "ha"
                elif "MODE: DESKTOP" in line:
                    self.current_mode = "desktop"

                # Learn/refresh the device name from its identity line.
                hello = parse_hello_line(line)
                if hello is not None:
                    self.device_name = hello
                    continue

                # Parse button click event
                event = parse_event_line(line)
                if event:
                    self._handle_hardware_trigger(event["mode"],
                                                  event["index"],
                                                  event["trigger"])

            except Exception as e:
                print(f"[DAEMON] Serial read exception: {e}")
                if self.ser:
                    try:
                        self.ser.close()
                    except Exception:
                        pass
                self.ser = None
                time.sleep(1)

    def _handle_hardware_trigger(self, mode: str, button_index: int, trigger_event: str):
        """Dispatch action based on active configuration."""
        execute_button_trigger(self.config, mode, button_index, trigger_event,
                               on_log=self.on_event_callback)
