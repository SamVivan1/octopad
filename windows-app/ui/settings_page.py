#!/usr/bin/env python3
"""OctoPad - Settings page."""

from typing import Any

from PySide6.QtWidgets import (
    QGroupBox, QHBoxLayout, QInputDialog, QLabel, QPushButton, QSpinBox,
    QWidget,
)

import config as config_module
import state
from ui import util


class SettingsPage(QWidget):
    def __init__(self, window) -> None:
        super().__init__()
        self.window = window

        outer = util.vbox(0, self)
        content = QWidget()
        layout = util.vbox(12, content)

        # --- Timing ---------------------------------------------------
        timing = QGroupBox("Timing")
        timing_layout = util.vbox(8, timing)
        self.timing_spins = {}
        for field, label, minimum, maximum, step in (
            ("debounceMs", "Debounce (ms)", 0, 1000, 5),
            ("clickTimeoutMs", "Click timeout (ms)", 50, 2000, 10),
            ("holdTimeoutMs", "Hold timeout (ms)", 50, 3000, 10),
        ):
            row = QHBoxLayout()
            row.addWidget(QLabel(label), 1)
            spin = QSpinBox()
            spin.setRange(minimum, maximum)
            spin.setSingleStep(step)
            spin.setValue(int(state.cfg.get(field, 0)))
            spin.valueChanged.connect(
                lambda value, f=field: self._on_timing(f, value))
            row.addWidget(spin)
            timing_layout.addLayout(row)
            self.timing_spins[field] = spin
        layout.addWidget(timing)

        # --- Home Assistant -------------------------------------------
        ha_group = QGroupBox("Home Assistant")
        ha_layout = util.vbox(8, ha_group)
        ha_row = QHBoxLayout()
        self.ha_row = QLabel("Checking...")
        ha_row.addWidget(self.ha_row, 1)
        ha_button = QPushButton("Connection...")
        ha_button.clicked.connect(self._on_ha_connect)
        ha_row.addWidget(ha_button)
        ha_layout.addLayout(ha_row)

        entity_row = QHBoxLayout()
        self.ha_entity_row = QLabel(f"{len(state.ha_entities)} entities")
        entity_row.addWidget(self.ha_entity_row, 1)
        reload_button = QPushButton("Reload")
        reload_button.clicked.connect(self._on_ha_reload)
        entity_row.addWidget(reload_button)
        ha_layout.addLayout(entity_row)
        layout.addWidget(ha_group)

        # --- Applications ---------------------------------------------
        apps_group = QGroupBox("Installed Applications")
        apps_layout = util.vbox(8, apps_group)
        apps_row = QHBoxLayout()
        self.apps_row = QLabel(f"{len(state.installed_apps)} applications")
        apps_row.addWidget(self.apps_row, 1)
        rescan = QPushButton("Rescan")
        rescan.clicked.connect(self._on_rescan_apps)
        apps_row.addWidget(rescan)
        apps_layout.addLayout(apps_row)
        layout.addWidget(apps_group)

        # --- Device ---------------------------------------------------
        device_group = QGroupBox("Device")
        device_layout = util.vbox(8, device_group)
        device_layout.addWidget(util.hint(
            "The name your pad advertises over Bluetooth/USB. Stored on the pad."))
        device_row = QHBoxLayout()
        self.device_row = QLabel("Not connected")
        device_row.addWidget(self.device_row, 1)
        rename = QPushButton("Rename...")
        rename.clicked.connect(self._on_rename_device)
        device_row.addWidget(rename)
        device_layout.addLayout(device_row)
        layout.addWidget(device_group)

        # --- Info -----------------------------------------------------
        info = QGroupBox("Information")
        info_layout = util.vbox(4, info)
        info_layout.addWidget(QLabel(f"Configuration: {config_module.CONFIG_FILE}"))
        info_layout.addWidget(util.hint(
            "Runtime: PySide6 (Qt6), pyserial + bleak, direct serial listener "
            "(no HTTP server)."))
        layout.addWidget(info)

        layout.addStretch(1)
        outer.addWidget(util.scroll_area(content))
        outer.addStretch(1)

        state.subscribe(self._on_state_event)
        self._sync_state()

    # ------------------------------------------------------------------
    def _on_timing(self, field: str, value: int) -> None:
        if state.cfg.get(field) != value:
            state.set_timing(field, value)

    def _sync_state(self) -> None:
        status = state.ha_message or "Not configured"
        self.ha_row.setText(("HA: Connected - " if state.ha_connected else "") + status)
        self.ha_entity_row.setText(f"{len(state.ha_entities)} entities")
        self.apps_row.setText(f"{len(state.installed_apps)} applications")
        if state.device_name:
            self.device_row.setText(f"Name: {state.device_name}")
        elif state.serial_connected:
            self.device_row.setText("Connected (name unknown)")
        else:
            self.device_row.setText("Not connected")

    def _on_state_event(self, event: str, **data: Any) -> None:
        if event == "ha-status-changed":
            self.ha_row.setText(state.ha_message or "Home Assistant status")
        elif event == "ha-entities-changed":
            self.ha_entity_row.setText(f"{len(state.ha_entities)} entities")
        elif event == "apps-changed":
            self.apps_row.setText(f"{len(state.installed_apps)} applications")
        elif event == "device-name-changed":
            self.device_row.setText(state.device_name or "Not connected")

    # ------------------------------------------------------------------
    def _on_ha_connect(self) -> None:
        from ui.ha_dialogs import HaConnectionDialog
        HaConnectionDialog(self.window).exec()

    def _on_ha_reload(self) -> None:
        from home_assistant import ha_client
        self.window.show_toast("Reloading entities...", "info")

        def done(result):
            entities = result if isinstance(result, list) else []
            source = getattr(ha_client, "_last_scan_live", False)
            state.set_ha_entities(entities, "live" if source else "cache")
            self.window.show_toast(f"{len(entities)} entities loaded.", "success")

        util.run_async(lambda: ha_client.get_entities(force_refresh=True), done)

    def _on_rescan_apps(self) -> None:
        from app_scanner import get_installed_apps
        self.window.show_toast("Scanning applications...", "info")

        def done(result):
            apps = result if isinstance(result, list) else []
            state.set_apps(apps)
            self.window.show_toast(f"{len(apps)} applications found.", "success")

        util.run_async(lambda: get_installed_apps(force_refresh=True), done)

    def _on_rename_device(self) -> None:
        if not (state.serial_connected or state.ble_connected):
            self.window.show_toast(
                "Connect the macropad (USB or Bluetooth) to rename it.", "error")
            return
        name, ok = QInputDialog.getText(
            self, "Rename Device", "Name (1-24 characters):",
            text=state.device_name or "OctoPad")
        if not ok:
            return
        name = name.strip()
        if not name or len(name) > 24:
            self.window.show_toast("Name must be 1-24 characters.", "error")
            return
        if state.rename_device(name):
            self.window.show_toast(f'Device renamed to "{name}".', "success")
        else:
            self.window.show_toast("Failed to rename the device.", "error")
