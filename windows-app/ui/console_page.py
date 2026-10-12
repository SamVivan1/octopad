#!/usr/bin/env python3
"""OctoPad - Serial console page (hardware log + command input)."""

from typing import Any, Optional

from PySide6.QtGui import QColor, QTextCharFormat, QTextCursor
from PySide6.QtWidgets import (
    QHBoxLayout, QLabel, QLineEdit, QPlainTextEdit, QPushButton, QWidget,
)

import state
from ui import util

_COLORS = {
    "system": "#8ff0a4",
    "action": "#99c1f1",
    "error": "#f66151",
    "tx": "#f9f06b",
    "time": "#9a9996",
}


class ConsolePage(QWidget):
    def __init__(self, window) -> None:
        super().__init__()
        self.window = window
        self._line_count = 0

        layout = util.vbox(10, self)

        top = QHBoxLayout()
        self.status_label = QLabel("Serial: checking...")
        top.addWidget(self.status_label, 1)
        clear = QPushButton("Clear")
        clear.clicked.connect(self._on_clear)
        top.addWidget(clear)
        layout.addLayout(top)

        quick = QHBoxLayout()
        quick.addWidget(QLabel("Quick:"))
        for label, command in (("Who", "WHO"), ("Info", "INFO"),
                               ("Save", "SAVE"), ("Help", "HELP")):
            btn = QPushButton(label)
            btn.clicked.connect(lambda _checked=False, c=command: self._send_command(c))
            quick.addWidget(btn)
        quick.addStretch(1)
        layout.addLayout(quick)

        self.view = QPlainTextEdit()
        self.view.setReadOnly(True)
        layout.addWidget(self.view, 1)

        bottom = QHBoxLayout()
        self.entry = QLineEdit()
        self.entry.setPlaceholderText("Send a command to the macropad (e.g. CMD:PING)...")
        self.entry.returnPressed.connect(self._on_send)
        bottom.addWidget(self.entry, 1)
        send = QPushButton("Send")
        send.clicked.connect(self._on_send)
        bottom.addWidget(send)
        layout.addLayout(bottom)

        state.subscribe(self._on_state_event)
        self._sync_status()
        self._append_line("[SYSTEM] Serial console ready - hardware logs appear here.",
                          "system")

    # ------------------------------------------------------------------
    def _classify(self, line: str) -> str:
        upper = line.upper()
        if "[ERROR" in upper or " FAIL]" in upper:
            return "error"
        if "[ACTION]" in upper:
            return "action"
        if upper.startswith("[TX]") or line.startswith("> "):
            return "tx"
        return "system"

    def _append_line(self, text: str, kind: Optional[str] = None) -> None:
        import time
        kind = kind or self._classify(text)
        cursor = self.view.textCursor()
        cursor.movePosition(QTextCursor.End)

        stamp = time.strftime("%H:%M:%S")
        time_format = QTextCharFormat()
        time_format.setForeground(QColor(_COLORS["time"]))
        cursor.insertText(f"[{stamp}] ", time_format)

        body_format = QTextCharFormat()
        body_format.setForeground(QColor(_COLORS.get(kind, _COLORS["system"])))
        cursor.insertText(text + "\n", body_format)

        self._line_count += 1
        if self._line_count > 1500:
            self._trim()
        self.view.verticalScrollBar().setValue(
            self.view.verticalScrollBar().maximum())

    def _trim(self) -> None:
        cursor = self.view.textCursor()
        cursor.movePosition(QTextCursor.Start)
        cursor.movePosition(QTextCursor.Down, QTextCursor.KeepAnchor, 200)
        cursor.removeSelectedText()

    # ------------------------------------------------------------------
    def _on_clear(self) -> None:
        self.view.setPlainText("")
        self._line_count = 0

    def _on_send(self) -> None:
        self._send_command(self.entry.text().strip())

    def _send_command(self, text: str) -> None:
        if not text:
            return
        try:
            ok = state.send_device_command(text)
        except Exception as exc:  # noqa: BLE001
            self.window.show_toast(f"Failed to send: {exc}", "error")
            return
        if ok:
            self.entry.clear()
            self._append_line(f"> {text}", "tx")
        else:
            self.window.show_toast(
                "Serial port not open - connect the macropad first.", "error")

    # ------------------------------------------------------------------
    def _sync_status(self) -> None:
        if state.serial_connected:
            self.status_label.setText(
                f"Serial: connected at {state.serial_port or '?'} (115200 baud)")
        elif state.ble_connected:
            self.status_label.setText("Bluetooth: connected")
        else:
            self.status_label.setText("Waiting for macropad (USB or Bluetooth)...")

    def _on_state_event(self, event: str, **data: Any) -> None:
        if event == "serial-log":
            self._append_line(str(data.get("line", "")))
        elif event == "serial-status-changed":
            self._sync_status()
            if data.get("connected"):
                self._append_line(
                    f"[HARDWARE] Connected at {data.get('port') or '?'}", "system")
        elif event == "ble-status-changed":
            self._sync_status()
