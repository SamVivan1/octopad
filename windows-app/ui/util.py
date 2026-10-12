#!/usr/bin/env python3
"""
OctoPad - shared Qt UI helpers.

Icons from an icon theme are avoided on purpose (Windows ships with none), so
action types are shown with short text badges instead of symbolic icons.
"""

from typing import Any, Callable, Dict, List, Optional

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QFrame, QHBoxLayout, QLabel, QPushButton, QScrollArea, QToolButton,
    QVBoxLayout, QWidget,
)

import state

# Re-exported so pages only import from one place.
run_async = state.run_async

# Short text badge shown for each action type (no icon fonts required).
ACTION_BADGE: Dict[str, str] = {
    "none": "-",
    "shortcut": "KEY",
    "text": "TXT",
    "media": "MED",
    "audio_output": "AUD",
    "launch_app": "APP",
    "system_action": "SYS",
    "bash_script": "CMD",
    "home_assistant": "HA",
    "ha_webhook": "WH",
    "delay": "DLY",
    "mode_toggle": "MODE",
}

ACTION_SHORT: Dict[str, str] = {
    "none": "None",
    "shortcut": "Shortcut",
    "text": "Text",
    "media": "Media",
    "audio_output": "Audio Output",
    "launch_app": "App",
    "system_action": "System",
    "bash_script": "Script",
    "home_assistant": "Home Assistant",
    "ha_webhook": "Webhook",
    "delay": "Delay",
    "mode_toggle": "Mode",
}

ACTION_HINTS: Dict[str, str] = {
    "none": "Empty slot. Pick a type from the menu, or use the trash button to remove it.",
    "shortcut": "Sends a keyboard shortcut to the active window.",
    "text": "Types this text as keystrokes into the active window.",
    "media": "Controls volume or media playback (SendInput media keys / pycaw).",
    "audio_output": "Switches the default audio output to the next selected device each "
                    "time (cycles through your chosen outputs).",
    "launch_app": "Launches an installed application (Start Menu shortcut).",
    "system_action": "Runs a built-in system action preset.",
    "bash_script": "Runs the command in the background via cmd.exe.",
    "home_assistant": "Calls a Home Assistant service on the chosen entity.",
    "ha_webhook": "POSTs an empty JSON body to a Home Assistant webhook URL.",
    "delay": "Pauses the action sequence before the next action.",
    "mode_toggle": "Switches the layer between Desktop and Home Assistant mode "
                   "(handled by the macropad firmware / status LED).",
}

DOMAIN_BADGE: Dict[str, str] = {
    "light": "LGT",
    "switch": "SW",
    "media_player": "MED",
    "cover": "CVR",
    "lock": "LCK",
    "scene": "SCN",
    "script": "SCR",
    "automation": "AUT",
    "button": "BTN",
    "number": "NUM",
    "select": "SEL",
    "sensor": "SNS",
}
DEFAULT_BADGE = "HA"


def _badge_text(value: str, table: Dict[str, str], default: str) -> str:
    return table.get(value, default)


def action_badge(action_type: str) -> str:
    return _badge_text(action_type, ACTION_BADGE, "?")


def domain_badge(domain: str) -> str:
    shown = (domain or "").upper()[:3]
    return _badge_text(domain, DOMAIN_BADGE, shown or DEFAULT_BADGE)


# ---------------------------------------------------------------------------
# Small widget builders
# ---------------------------------------------------------------------------
def vbox(spacing: int = 8, parent: Optional[QWidget] = None) -> QVBoxLayout:
    layout = QVBoxLayout(parent) if parent is not None else QVBoxLayout()
    layout.setSpacing(spacing)
    return layout


def hbox(spacing: int = 8, parent: Optional[QWidget] = None) -> QHBoxLayout:
    layout = QHBoxLayout(parent) if parent is not None else QHBoxLayout()
    layout.setSpacing(spacing)
    return layout


def label(text: str, css: Optional[List[str]] = None, wrap: bool = False) -> QLabel:
    widget = QLabel(text)
    widget.setWordWrap(wrap)
    if css:
        widget.setProperty("css", css)
    return widget


def hint(text: str) -> QLabel:
    widget = QLabel(text)
    widget.setWordWrap(True)
    widget.setProperty("css", ["hint"])
    return widget


def badge(text: str, kind: str = "") -> QLabel:
    widget = QLabel(text)
    widget.setAlignment(Qt.AlignCenter)
    widget.setProperty("css", ["badge"] + ([kind] if kind else []))
    return widget


def button(text: str, css: Optional[List[str]] = None,
           on_clicked: Optional[Callable] = None) -> QPushButton:
    widget = QPushButton(text)
    if css:
        widget.setProperty("css", css)
    if on_clicked is not None:
        widget.clicked.connect(lambda *_: on_clicked())
    return widget


def tool_button(text: str, tooltip: str,
                on_clicked: Optional[Callable] = None,
                css: Optional[List[str]] = None) -> QToolButton:
    widget = QToolButton()
    widget.setText(text)
    widget.setToolTip(tooltip)
    if css:
        widget.setProperty("css", css)
    if on_clicked is not None:
        widget.clicked.connect(lambda *_: on_clicked())
    return widget


def separator() -> QFrame:
    line = QFrame()
    line.setFrameShape(QFrame.HLine)
    line.setFrameShadow(QFrame.Sunken)
    return line


def scroll_area(content: QWidget) -> QScrollArea:
    area = QScrollArea()
    area.setWidgetResizable(True)
    area.setWidget(content)
    area.setFrameShape(QFrame.NoFrame)
    return area


def toast_result(window: Any, result: Any, prefix: str = "") -> None:
    if isinstance(result, tuple) and len(result) == 2:
        ok, message = result
    else:
        ok, message = bool(result), str(result)
    kind = "success" if ok else "error"
    window.show_toast(f"{prefix}{message}", kind)
