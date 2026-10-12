#!/usr/bin/env python3
"""OctoPad - main window (PySide6 / Qt6)."""

from typing import Any, Dict

from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QAction
from PySide6.QtWidgets import (
    QComboBox, QDialog, QDialogButtonBox, QHBoxLayout, QLabel, QLineEdit,
    QListWidget, QListWidgetItem, QMainWindow, QMessageBox, QPushButton,
    QRadioButton, QSizePolicy, QStackedWidget, QWidget,
)

import config as config_module
import state
from ui import util

_PAGES = (
    ("keys", "Buttons"),
    ("console", "Serial Console"),
    ("profiles", "Profiles"),
    ("settings", "Settings"),
)


class OctoPadWindow(QMainWindow):
    def __init__(self, app) -> None:
        super().__init__()
        self.app = app
        self.setWindowTitle("OctoPad - Desktop Mode")
        self.resize(1180, 780)
        self._pages: Dict[str, QWidget] = {}

        central = QWidget()
        root = util.hbox(0, central)
        root.setContentsMargins(0, 0, 0, 0)

        self.sidebar = QListWidget()
        self.sidebar.setFixedWidth(190)
        for name, title in _PAGES:
            item = QListWidgetItem(title)
            item.setData(Qt.UserRole, name)
            self.sidebar.addItem(item)
        self.sidebar.currentRowChanged.connect(self._on_sidebar_changed)
        root.addWidget(self.sidebar)

        self.stack = QStackedWidget()
        root.addWidget(self.stack, 1)
        self.setCentralWidget(central)

        self._get_page("keys")
        self.sidebar.setCurrentRow(0)

        self._build_menu()
        self._build_toolbar()

        state.subscribe(self._on_state_event)
        self._sync_pills()
        self._sync_save_button()

        self._poll_timer = QTimer(self)
        self._poll_timer.setInterval(3000)
        self._poll_timer.timeout.connect(self._poll_serial)
        self._poll_timer.start()
        self._poll_serial()

    # ------------------------------------------------------------------
    # Menu & toolbar
    # ------------------------------------------------------------------
    def _build_menu(self) -> None:
        menu = self.menuBar().addMenu("Menu")

        save_action = QAction("Save", self)
        save_action.setShortcut("Ctrl+S")
        save_action.triggered.connect(lambda: state.save())
        menu.addAction(save_action)

        save_preset = QAction("Save as Preset...", self)
        save_preset.triggered.connect(self.show_save_preset_dialog)
        menu.addAction(save_preset)

        profiles = menu.addMenu("Profiles")
        profiles.addAction("Import Profile...",
                           lambda: self._get_page("profiles")._on_import(None))
        profiles.addAction("Export Profile...",
                           lambda: self._get_page("profiles")._on_export(None))

        presets = menu.addMenu("Built-in Presets")
        for key, title, _subtitle in config_module.PRESET_ITEMS:
            action = QAction(title, self)
            action.triggered.connect(
                lambda _checked=False, k=key: self._get_page("profiles")._on_apply_preset(None, k))
            presets.addAction(action)

        menu.addSeparator()
        about = QAction("About OctoPad", self)
        about.triggered.connect(self._show_about)
        menu.addAction(about)

    def _build_toolbar(self) -> None:
        bar = self.addToolBar("Main")
        bar.setMovable(False)

        self.save_button = QPushButton("Save")
        self.save_button.clicked.connect(lambda: state.save())
        bar.addWidget(self.save_button)

        spacer = QWidget()
        spacer.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
        bar.addWidget(spacer)

        self.conn_button = QPushButton("Disconnected")
        self.conn_button.clicked.connect(lambda: self._show_page("console"))
        bar.addWidget(self.conn_button)

        self.ha_button = QPushButton("HA: checking...")
        self.ha_button.clicked.connect(self._on_ha_clicked)
        bar.addWidget(self.ha_button)

    def _show_about(self) -> None:
        QMessageBox.about(
            self, "About OctoPad",
            "OctoPad v2.5\n\n"
            "Native Windows application for an 8-key serial macropad.\n"
            "Home Assistant REST integration and local action mapping,\n"
            "with no web server required.\n\n"
            "MIT License - SamVivan and the OctoPad contributors.")

    def _on_ha_clicked(self) -> None:
        from ui.ha_dialogs import HaConnectionDialog
        HaConnectionDialog(self).exec()

    # ------------------------------------------------------------------
    # Navigation
    # ------------------------------------------------------------------
    def _get_page(self, name: str) -> QWidget:
        page = self._pages.get(name)
        if page is None:
            if name == "keys":
                from ui.keys_page import KeysPage
                page = KeysPage(self)
            elif name == "console":
                from ui.console_page import ConsolePage
                page = ConsolePage(self)
            elif name == "profiles":
                from ui.profiles_page import ProfilesPage
                page = ProfilesPage(self)
            elif name == "settings":
                from ui.settings_page import SettingsPage
                page = SettingsPage(self)
            else:
                raise ValueError(f"Unknown page: {name}")
            self._pages[name] = page
            self.stack.addWidget(page)
        return page

    def _show_page(self, name: str) -> QWidget:
        page = self._get_page(name)
        self.stack.setCurrentWidget(page)
        row = [p[0] for p in _PAGES].index(name)
        if self.sidebar.currentRow() != row:
            self.sidebar.setCurrentRow(row)
        return page

    def _on_sidebar_changed(self, row: int) -> None:
        if 0 <= row < len(_PAGES):
            self._get_page(_PAGES[row][0])
            self.stack.setCurrentIndex(row)

    # ------------------------------------------------------------------
    # Toasts (status bar messages)
    # ------------------------------------------------------------------
    def show_toast(self, message: str, kind: str = "info") -> None:
        self.statusBar().showMessage(message, 6000 if kind == "error" else 4000)

    # ------------------------------------------------------------------
    # Save as preset dialog
    # ------------------------------------------------------------------
    def show_save_preset_dialog(self) -> None:
        presets = config_module.list_presets()
        dialog = QDialog(self)
        dialog.setWindowTitle("Save as Preset")
        layout = util.vbox(10, dialog)

        overwrite = QRadioButton("Overwrite existing preset")
        new = QRadioButton("Create new preset")
        overwrite.setChecked(bool(presets))
        new.setChecked(not presets)
        if not presets:
            overwrite.setEnabled(False)
        layout.addWidget(overwrite)

        row = QHBoxLayout()
        row.addWidget(QLabel("Preset:"))
        combo = QComboBox()
        combo.addItems(presets or ["(no presets yet)"])
        if state.active_preset in presets:
            combo.setCurrentIndex(presets.index(state.active_preset))
        row.addWidget(combo, 1)
        layout.addLayout(row)

        layout.addWidget(new)
        name_edit = QLineEdit()
        name_edit.setPlaceholderText("New preset name")
        name_edit.setText(f"Preset {len(presets) + 1}")
        layout.addWidget(name_edit)

        def sync_mode() -> None:
            overwriting = overwrite.isChecked()
            combo.setEnabled(overwriting)
            name_edit.setEnabled(not overwriting)

        overwrite.toggled.connect(sync_mode)
        sync_mode()

        buttons = QDialogButtonBox(QDialogButtonBox.Save | QDialogButtonBox.Cancel)
        buttons.accepted.connect(dialog.accept)
        buttons.rejected.connect(dialog.reject)
        layout.addWidget(buttons)

        if dialog.exec() != QDialog.Accepted:
            return
        if overwrite.isChecked():
            if not presets:
                self.show_toast("No preset to overwrite - create a new one.", "error")
                return
            name = presets[combo.currentIndex()]
            ok = state.save_preset(name, overwrite=True)
            action = "updated"
        else:
            name = name_edit.text().strip()
            if not name:
                self.show_toast("Preset name cannot be empty.", "error")
                return
            ok = state.save_preset(name, overwrite=False)
            action = "created"

        if not ok:
            self.show_toast("Failed to save preset.", "error")
            return
        self.show_toast(f'Preset "{state.active_preset or name}" {action}.', "success")

    # ------------------------------------------------------------------
    # Status sync
    # ------------------------------------------------------------------
    def _sync_pills(self) -> None:
        if state.serial_connected or state.ble_connected:
            if state.serial_connected and state.ble_connected:
                tip = f"Connected via USB ({state.serial_port or '?'}) and Bluetooth"
            elif state.serial_connected:
                tip = f"Connected via USB ({state.serial_port or '?'})"
            else:
                tip = "Connected via Bluetooth"
            self.conn_button.setText("Connected")
            self.conn_button.setToolTip(tip + " - click to open the console")
        elif "searching" in (state.ble_message or "").lower():
            self.conn_button.setText("Disconnected")
            self.conn_button.setToolTip(state.ble_message or "Searching for the macropad...")
        else:
            self.conn_button.setText("Disconnected")
            self.conn_button.setToolTip(
                state.ble_message or "OctoPad not connected - connect via USB or Bluetooth")

        if state.ha_connected:
            self.ha_button.setText("HA: Connected")
        elif not state.ha_message:
            self.ha_button.setText("HA: checking...")
        else:
            self.ha_button.setText("HA: disconnected")
        self.ha_button.setToolTip(state.ha_message or "Home Assistant status")

    def _sync_save_button(self) -> None:
        self.save_button.setEnabled(state.dirty)
        self.save_button.setToolTip(
            "Save changes (Ctrl+S)" if state.dirty else "Configuration already saved")

    def _poll_serial(self) -> None:
        listener = state.listener
        if listener is not None:
            open_port = getattr(listener, "ser", None)
            connected = bool(open_port is not None and open_port.is_open)
            port = listener.last_connected_port if connected else None
            state.set_serial_status(connected, port)
            state.set_device_name(getattr(listener, "device_name", None))

    # ------------------------------------------------------------------
    # Event bus
    # ------------------------------------------------------------------
    def _on_state_event(self, event: str, **data: Any) -> None:
        if event == "mode-changed":
            self.setWindowTitle(
                "OctoPad - Home Assistant Mode" if state.mode_index == 1
                else "OctoPad - Desktop Mode")
        elif event in ("serial-status-changed", "ha-status-changed", "ble-status-changed"):
            self._sync_pills()
        elif event in ("dirty-changed", "config-saved"):
            self._sync_save_button()
            if event == "config-saved" and data.get("ok") is False:
                self.show_toast("Failed to save configuration.", "error")

    # ------------------------------------------------------------------
    def closeEvent(self, event) -> None:  # noqa: N802 - Qt override
        if getattr(self.app, "_quitting", False) or not getattr(self.app, "tray_ok", False):
            event.accept()
            return
        event.ignore()
        self.hide()
