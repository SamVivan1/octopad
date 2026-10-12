#!/usr/bin/env python3
"""OctoPad - installed application picker dialog."""

from typing import Callable, Dict

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog, QLineEdit, QListWidget, QListWidgetItem,
)

import state
from ui import util


class AppPickerDialog(QDialog):
    def __init__(self, window, on_pick: Callable[[Dict[str, str]], None]) -> None:
        super().__init__(window)
        self.setWindowTitle("Choose Application")
        self.resize(600, 560)
        self.on_pick = on_pick
        self.apps = list(state.installed_apps)

        layout = util.vbox(10, self)

        search = QLineEdit()
        search.setPlaceholderText("Search application name...")
        search.textChanged.connect(self._on_search)
        layout.addWidget(search)

        self.list = QListWidget()
        self.list.itemActivated.connect(self._on_activated)
        self.list.itemClicked.connect(self._on_activated)
        layout.addWidget(self.list, 1)

        if not self.apps:
            layout.addWidget(util.hint(
                "No applications yet - run a rescan on the Settings page."))

        self._populate()

    def _populate(self) -> None:
        self.list.clear()
        for app in self.apps:
            item = QListWidgetItem(app.get("name", ""))
            comment = app.get("comment") or app.get("id", "")
            item.setToolTip(comment)
            item.setData(Qt.UserRole, app)
            self.list.addItem(item)

    def _on_search(self, text: str) -> None:
        needle = (text or "").strip().lower()
        for row in range(self.list.count()):
            item = self.list.item(row)
            app = item.data(Qt.UserRole) or {}
            haystack = " ".join([
                app.get("name", ""), app.get("id", ""),
                app.get("comment", ""), app.get("exec", ""),
            ]).lower()
            item.setHidden(bool(needle) and needle not in haystack)

    def _on_activated(self, item: QListWidgetItem) -> None:
        app = item.data(Qt.UserRole)
        if app:
            self.on_pick(app)
            self.accept()
