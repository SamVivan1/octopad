#!/usr/bin/env python3
"""OctoPad - audio output picker dialog (multi-select)."""

from typing import Callable

from PySide6.QtWidgets import (
    QCheckBox, QDialog, QDialogButtonBox, QLabel, QListWidget, QListWidgetItem,
)

from platform_actions import list_audio_outputs
from ui import util


class AudioOutputPickerDialog(QDialog):
    def __init__(self, window, current, on_pick: Callable[[list], None]) -> None:
        super().__init__(window)
        self.setWindowTitle("Choose Audio Outputs")
        self.resize(560, 480)
        self.on_pick = on_pick
        self.selected = {
            s.get("name") for s in (current or []) if isinstance(s, dict) and s.get("name")}

        layout = util.vbox(10, self)
        self.status = QLabel("Scanning audio outputs...")
        layout.addWidget(self.status)

        self.list = QListWidget()
        layout.addWidget(self.list, 1)

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self._on_accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

        self.rows = []
        util.run_async(list_audio_outputs, self._on_scanned)

    def _on_scanned(self, result) -> None:
        sinks = result if isinstance(result, list) else []
        if not sinks:
            self.status.setText("No audio outputs found.")
            return
        self.status.hide()
        for sink in sinks:
            item = QListWidgetItem()
            check = QCheckBox(sink.get("description") or sink.get("name", ""))
            check.setChecked(sink.get("name") in self.selected)
            check.setToolTip(sink.get("name", ""))
            self.list.addItem(item)
            self.list.setItemWidget(item, check)
            self.rows.append((check, sink))

    def _on_accept(self) -> None:
        picked = [sink for check, sink in self.rows if check.isChecked()]
        self.on_pick(picked)
        self.accept()
