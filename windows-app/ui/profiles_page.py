#!/usr/bin/env python3
"""OctoPad - Profiles page (import/export/preset)."""

import json
from typing import Any, Dict

from PySide6.QtWidgets import (
    QFileDialog, QGroupBox, QHBoxLayout, QLabel, QPushButton, QWidget,
)

import config as config_module
import state
from ui import util


class ProfilesPage(QWidget):
    def __init__(self, window) -> None:
        super().__init__()
        self.window = window

        outer = util.vbox(0, self)
        content = QWidget()
        layout = util.vbox(12, content)

        # --- Current profile ------------------------------------------
        current = QGroupBox("Current Profile")
        current_layout = util.vbox(8, current)

        path_row = QHBoxLayout()
        path_row.addWidget(QLabel("Configuration file:"))
        path_label = QLabel(config_module.CONFIG_FILE)
        path_label.setProperty("css", ["hint"])
        path_row.addWidget(path_label, 1)
        current_layout.addLayout(path_row)

        state_row = QHBoxLayout()
        self.state_label = QLabel("Saved")
        state_row.addWidget(self.state_label, 1)
        self.preset_button = QPushButton("Save as Preset...")
        self.preset_button.clicked.connect(self._on_save_preset)
        state_row.addWidget(self.preset_button)
        self.save_button = QPushButton("Save")
        self.save_button.clicked.connect(self._on_save)
        state_row.addWidget(self.save_button)
        current_layout.addLayout(state_row)

        io_row = QHBoxLayout()
        io_row.addWidget(QLabel("Import / Export the profile as JSON."), 1)
        import_button = QPushButton("Import...")
        import_button.clicked.connect(self._on_import)
        io_row.addWidget(import_button)
        export_button = QPushButton("Export...")
        export_button.clicked.connect(self._on_export)
        io_row.addWidget(export_button)
        current_layout.addLayout(io_row)
        layout.addWidget(current)

        # --- Built-in presets -----------------------------------------
        presets_group = QGroupBox("Built-in Presets")
        presets_layout = util.vbox(6, presets_group)
        for key, title, subtitle in config_module.PRESET_ITEMS:
            row = QHBoxLayout()
            text = util.vbox(0)
            text.addWidget(QLabel(title))
            text.addWidget(util.hint(subtitle))
            row.addLayout(text, 1)
            apply_button = QPushButton("Apply")
            apply_button.clicked.connect(
                lambda _c, k=key: self._on_apply_preset(None, k))
            row.addWidget(apply_button)
            presets_layout.addLayout(row)
        layout.addWidget(presets_group)

        # --- My presets -----------------------------------------------
        self.user_group = QGroupBox("My Presets")
        self.user_layout = util.vbox(6, self.user_group)
        layout.addWidget(self.user_group)

        about = QGroupBox("About")
        about_layout = util.vbox(4, about)
        about_layout.addWidget(QLabel("OctoPad v2.5"))
        about_layout.addWidget(util.hint(
            "Native Windows application (PySide6) - 8 keys, Home Assistant REST, "
            "no scripts."))
        layout.addWidget(about)

        layout.addStretch(1)
        outer.addWidget(util.scroll_area(content))
        outer.addStretch(1)

        state.subscribe(self._on_state_event)
        self._sync_state()

    # ------------------------------------------------------------------
    def _sync_state(self) -> None:
        if state.dirty:
            self.state_label.setText("Unsaved (use Save in the header)")
            self.save_button.setEnabled(True)
        else:
            self.state_label.setText("Saved to disk")
            self.save_button.setEnabled(False)
        self._rebuild_user_presets()

    def _rebuild_user_presets(self) -> None:
        while self.user_layout.count():
            item = self.user_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
            elif item.layout() is not None:
                self._clear_layout(item.layout())

        names = config_module.list_presets()
        if not names:
            self.user_layout.addWidget(util.hint(
                "No presets yet - use Save as Preset."))
            return
        for name in names:
            row = QHBoxLayout()
            marker = " (active)" if name == state.active_preset else ""
            row.addWidget(QLabel(f"{name}{marker}"), 1)
            load = QPushButton("Load")
            load.clicked.connect(lambda _c, n=name: self._on_load_user_preset(None, n))
            row.addWidget(load)
            delete = QPushButton("Delete")
            delete.clicked.connect(lambda _c, n=name: self._on_delete_user_preset(None, n))
            row.addWidget(delete)
            self.user_layout.addLayout(row)

    @staticmethod
    def _clear_layout(layout) -> None:
        while layout.count():
            item = layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()

    # ------------------------------------------------------------------
    def _on_load_user_preset(self, _widget, name: str) -> None:
        if state.apply_user_preset(name):
            self.window.show_toast(
                f'Preset "{name}" loaded - choose Save to activate it.', "success")
        else:
            self.window.show_toast(f'Failed to load preset "{name}".', "error")

    def _on_delete_user_preset(self, _widget, name: str) -> None:
        if state.delete_user_preset(name):
            self.window.show_toast(f'Preset "{name}" deleted.', "success")
        else:
            self.window.show_toast(f'Failed to delete preset "{name}".', "error")

    def _on_save(self, _btn=None) -> None:
        state.save()

    def _on_save_preset(self, _btn=None) -> None:
        self.window.show_save_preset_dialog()

    def _on_import(self, _btn=None) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self, "Import OctoPad Profile", "", "JSON Profile (*.json)")
        if not path:
            return
        try:
            with open(path, "r", encoding="utf-8") as handle:
                data: Dict[str, Any] = json.load(handle)
        except Exception as exc:  # noqa: BLE001
            self.window.show_toast(f"Invalid JSON: {exc}", "error")
            return
        ok, message = config_module.normalize_config(data)
        if not ok:
            self.window.show_toast(f"Invalid profile format: {message}", "error")
            return
        state.set_config(data, source="import")
        state.save()
        self.window.show_toast(f"Profile from {path} loaded.", "success")

    def _on_export(self, _btn=None) -> None:
        import time
        default = f"octopad-profile-{time.strftime('%Y-%m-%d')}.json"
        path, _ = QFileDialog.getSaveFileName(
            self, "Export OctoPad Profile", default, "JSON Profile (*.json)")
        if not path:
            return
        try:
            with open(path, "w", encoding="utf-8") as handle:
                json.dump(state.cfg, handle, indent=2, ensure_ascii=False)
            self.window.show_toast("Profile exported successfully.", "success")
        except Exception as exc:  # noqa: BLE001
            self.window.show_toast(f"Failed to write file: {exc}", "error")

    def _on_apply_preset(self, _widget, key: str) -> None:
        state.set_config(config_module.apply_preset(key), source="preset")
        state.save()
        title = next((t for k, t, _ in config_module.PRESET_ITEMS if k == key), key)
        self.window.show_toast(f'Preset "{title}" applied.', "success")

    # ------------------------------------------------------------------
    def _on_state_event(self, event: str, **data: Any) -> None:
        if event in ("dirty-changed", "config-saved", "config-replaced", "presets-changed"):
            self._sync_state()
