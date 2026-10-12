#!/usr/bin/env python3
"""OctoPad - Home Assistant connection dialog & entity picker."""

from typing import Any, Callable, Dict

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QComboBox, QDialog, QHBoxLayout, QLabel, QLineEdit, QListWidget,
    QListWidgetItem, QPushButton,
)

import state
from ui import util


class HaConnectionDialog(QDialog):
    def __init__(self, window) -> None:
        super().__init__(window)
        self.setWindowTitle("Home Assistant Connection")
        self.resize(560, 420)
        self.window = window
        from home_assistant import ha_client
        self.client = ha_client

        layout = util.vbox(10, self)

        layout.addWidget(QLabel("Home Assistant URL"))
        self.url_entry = QLineEdit(self.client.url or "")
        self.url_entry.setPlaceholderText("http://homeassistant.local:8123")
        layout.addWidget(self.url_entry)

        layout.addWidget(QLabel("Long-Lived Access Token"))
        self.token_entry = QLineEdit()
        self.token_entry.setEchoMode(QLineEdit.Password)
        self.token_entry.setPlaceholderText(
            "Stored token (leave blank to keep it)"
            if self.client.is_configured() else "eyJhbGciOi...")
        layout.addWidget(self.token_entry)

        layout.addWidget(util.hint(
            "Credentials are stored in %APPDATA%\\OctoPad\\ha_config.json"))

        self.status = QLabel("")
        self.status.setWordWrap(True)
        layout.addWidget(self.status)

        buttons = QHBoxLayout()
        self.test_button = QPushButton("Test")
        self.test_button.clicked.connect(self._on_test)
        buttons.addWidget(self.test_button)
        self.reload_button = QPushButton("Reload Entities")
        self.reload_button.clicked.connect(self._on_reload)
        buttons.addWidget(self.reload_button)
        save_button = QPushButton("Save")
        save_button.clicked.connect(self._on_save)
        buttons.addWidget(save_button)
        buttons.addStretch(1)
        layout.addLayout(buttons)

        layout.addStretch(1)
        self._set_status("info", "Read the current settings, then Test or Save.")

    # ------------------------------------------------------------------
    def _set_status(self, kind: str, message: str) -> None:
        self.status.setText(message)
        self.status.setProperty("css", [kind])

    def _credentials(self):
        url = self.url_entry.text().strip().rstrip("/")
        token = self.token_entry.text().strip() or (self.client.token or "")
        return url, token

    def _on_test(self) -> None:
        url, token = self._credentials()
        if not url or not token:
            self._set_status("error", "URL and token are required.")
            return
        self._set_status("info", "Testing connection...")

        def done(result):
            ok, message = result if isinstance(result, tuple) else (False, str(result))
            self._set_status("ok" if ok else "error", message)
            state.set_ha_status(bool(ok), message)

        util.run_async(lambda: self.client.check_connection(), done)

    def _on_reload(self) -> None:
        from home_assistant import ha_client
        self._set_status("info", "Scanning entities...")

        def done(result):
            entities = result if isinstance(result, list) else []
            source = "live" if getattr(ha_client, "_last_scan_live", False) else "cache"
            state.set_ha_entities(entities, source)
            self._set_status("ok", f"{len(entities)} entities loaded.")

        util.run_async(lambda: ha_client.get_entities(force_refresh=True), done)

    def _on_save(self) -> None:
        url, token = self._credentials()
        if not url or not token:
            self._set_status("error", "URL and token are required.")
            return
        self._set_status("info", "Saving and scanning entities...")

        def done(result):
            ok, message = result if isinstance(result, tuple) else (False, str(result))
            if not ok:
                self._set_status("error", message)
                return
            entities = self.client.get_entities()
            state.set_ha_entities(entities, "live" if self.client._last_scan_live else "cache")
            state.set_ha_status(True, "Connected to Home Assistant")
            self._set_status("ok", f"Saved. {len(entities)} entities loaded.")
            self.window.show_toast(f"{len(entities)} entities loaded.", "success")

        def work():
            saved = self.client.save_config(url, token)
            if not saved:
                return False, "Failed to save credentials"
            return self.client.check_connection()

        util.run_async(work, done)


class EntityPickerDialog(QDialog):
    def __init__(self, window, on_pick: Callable[[Dict[str, Any]], None]) -> None:
        super().__init__(window)
        self.setWindowTitle("Choose Entity")
        self.resize(640, 560)
        self.on_pick = on_pick

        layout = util.vbox(10, self)

        self.search = QLineEdit()
        self.search.setPlaceholderText("Search entities...")
        self.search.textChanged.connect(self._apply_filter)
        layout.addWidget(self.search)

        self.domain_combo = QComboBox()
        self.domain_combo.currentIndexChanged.connect(self._apply_filter)
        layout.addWidget(self.domain_combo)

        self.list = QListWidget()
        self.list.itemActivated.connect(self._on_activated)
        self.list.itemClicked.connect(self._on_activated)
        layout.addWidget(self.list, 1)

        if not state.ha_entities:
            layout.addWidget(util.hint(
                "No entities yet. Configure the Home Assistant connection, then reload."))

        self._populate()

    def _populate(self) -> None:
        domains = sorted({e.get("domain", "") for e in state.ha_entities if e.get("domain")})
        self.domain_combo.addItem("All", "all")
        for domain in domains:
            self.domain_combo.addItem(domain, domain)

        for entity in state.ha_entities:
            label = entity.get("friendly_name") or entity.get("entity_id", "")
            item = QListWidgetItem(f"{util.domain_badge(entity.get('domain', ''))}  {label}")
            item.setToolTip(entity.get("entity_id", ""))
            item.setData(Qt.UserRole, entity)
            self.list.addItem(item)

    def _apply_filter(self) -> None:
        needle = self.search.text().strip().lower()
        domain = self.domain_combo.currentData()
        for row in range(self.list.count()):
            item = self.list.item(row)
            entity = item.data(Qt.UserRole) or {}
            if domain != "all" and entity.get("domain") != domain:
                item.setHidden(True)
                continue
            haystack = " ".join([
                entity.get("friendly_name", ""), entity.get("entity_id", ""),
                entity.get("domain", ""),
            ]).lower()
            item.setHidden(bool(needle) and needle not in haystack)

    def _on_activated(self, item: QListWidgetItem) -> None:
        entity = item.data(Qt.UserRole)
        if entity:
            self.on_pick(entity)
            self.accept()
