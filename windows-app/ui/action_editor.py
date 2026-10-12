#!/usr/bin/env python3
"""
OctoPad - Action editor per trigger (Single / Double / Hold).

Each trigger can hold several actions. `ActionEditor` hosts one `_ActionCard`
per action, with buttons to add, reorder and remove. Every change writes
straight to state and emits an event so the grid stays in sync.
"""

import copy
from typing import Any, Dict, List

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDoubleSpinBox, QHBoxLayout, QLabel, QLineEdit,
    QPlainTextEdit, QPushButton, QVBoxLayout, QWidget,
)

import config as config_module
import state
from platform_actions import SYSTEM_ACTION_PRESETS, execute_action
from ui import util


def _clear_layout(layout: QVBoxLayout) -> None:
    while layout.count():
        item = layout.takeAt(0)
        widget = item.widget()
        if widget is not None:
            widget.deleteLater()


class ActionEditor(QWidget):
    """Editor for a single trigger: action list + add button."""

    def __init__(self, window, trigger: str, trigger_label: str) -> None:
        super().__init__()
        self.window = window
        self.trigger = trigger
        self._cards: List["_ActionCard"] = []

        layout = util.vbox(6, self)

        toolbar = QHBoxLayout()
        toolbar.addStretch(1)
        test_all = QPushButton("Test all")
        test_all.setToolTip("Run every action on this trigger")
        test_all.clicked.connect(self._test_all)
        toolbar.addWidget(test_all)
        add_button = QPushButton("Add action")
        add_button.setToolTip("Add another action to this trigger")
        add_button.clicked.connect(self._add_action)
        toolbar.addWidget(add_button)
        layout.addLayout(toolbar)

        self.cards_layout = QVBoxLayout()
        self.cards_layout.setSpacing(8)
        layout.addLayout(self.cards_layout)
        layout.addStretch(1)

        self.refresh()

    # ------------------------------------------------------------------
    def actions(self) -> List[Dict[str, Any]]:
        return state.get_actions(self.trigger)

    def refresh(self) -> None:
        _clear_layout(self.cards_layout)
        self._cards = []
        actions = self.actions() or [config_module.default_action("none")]
        for index, action in enumerate(actions):
            card = _ActionCard(self, index, action)
            self._cards.append(card)
            self.cards_layout.addWidget(card)

    def replace_action(self, index: int, action: Dict[str, Any]) -> None:
        state.set_action(self.trigger, index, action)
        self.refresh()

    def _add_action(self) -> None:
        state.add_action(self.trigger)
        self.refresh()

    def remove_action(self, index: int) -> None:
        state.remove_action(self.trigger, index)
        self.refresh()

    def move_action(self, index: int, delta: int) -> None:
        state.move_action(self.trigger, index, delta)
        self.refresh()

    # ------------------------------------------------------------------
    def _test_all(self) -> None:
        actions = [copy.deepcopy(a) for a in self.actions()
                   if a.get("type", "none") != "none"]
        if not actions:
            self.window.show_toast("No actions on this trigger", "info")
            return

        def work():
            return [execute_action(a.get("type", "none"), a) for a in actions]

        def done(result):
            for res in (result if isinstance(result, list) else []):
                util.toast_result(self.window, res, "Test: ")

        state.run_async(work, done)


class _ActionCard(QWidget):
    def __init__(self, editor: ActionEditor, index: int,
                 action: Dict[str, Any]) -> None:
        super().__init__()
        self.editor = editor
        self.index = index
        self.action = action
        self._syncing = False

        layout = util.vbox(6, self)

        head = QHBoxLayout()
        step = QLabel(str(index + 1))
        step.setFixedWidth(22)
        step.setAlignment(Qt.AlignCenter)
        head.addWidget(step)

        self.type_combo = QComboBox()
        for type_id, _label in config_module.ACTION_TYPES:
            self.type_combo.addItem(util.ACTION_SHORT.get(type_id, type_id), type_id)
        current = action.get("type", "none")
        ids = [t for t, _ in config_module.ACTION_TYPES]
        if current in ids:
            self._syncing = True
            self.type_combo.setCurrentIndex(ids.index(current))
            self._syncing = False
        self.type_combo.currentIndexChanged.connect(self._on_type_changed)
        head.addWidget(self.type_combo, 1)

        self.info = QLabel("?")
        self.info.setToolTip(util.ACTION_HINTS.get(current, ""))
        head.addWidget(self.info)

        up = QPushButton("Up")
        up.setFixedWidth(34)
        up.setEnabled(index > 0)
        up.clicked.connect(lambda: editor.move_action(self.index, -1))
        head.addWidget(up)

        down = QPushButton("Dn")
        down.setFixedWidth(34)
        down.setEnabled(index < len(state.get_actions(editor.trigger)) - 1)
        down.clicked.connect(lambda: editor.move_action(self.index, 1))
        head.addWidget(down)

        remove = QPushButton("Del")
        remove.setFixedWidth(38)
        remove.setToolTip("Remove this action")
        remove.clicked.connect(lambda: editor.remove_action(self.index))
        head.addWidget(remove)

        layout.addLayout(head)

        self.content = QVBoxLayout()
        layout.addLayout(self.content)
        self._rebuild()

    # ------------------------------------------------------------------
    def _on_type_changed(self, _index: int) -> None:
        if self._syncing:
            return
        type_id = self.type_combo.currentData()
        if type_id == self.action.get("type"):
            return
        self.editor.replace_action(self.index, config_module.default_action(type_id))

    def _rebuild(self) -> None:
        _clear_layout(self.content)
        kind = self.action.get("type", "none")
        self.info.setToolTip(util.ACTION_HINTS.get(kind, ""))
        builder = getattr(self, f"_build_{kind}", self._build_none)
        widget = builder(self.action)
        if widget is not None:
            self.content.addWidget(widget)

    # ------------------------------------------------------------------
    def _test_widget(self) -> QPushButton:
        button = QPushButton("Test")
        button.clicked.connect(self._test_action)
        return button

    def _test_action(self) -> None:
        action = copy.deepcopy(self.action)

        def work():
            return execute_action(action.get("type", "none"), action)

        state.run_async(work, lambda result: util.toast_result(
            self.editor.window, result, "Test: "))

    def _hint(self, text: str) -> QLabel:
        widget = QLabel(text)
        widget.setWordWrap(True)
        widget.setProperty("css", ["hint"])
        return widget

    # ------------------------------------------------------------------
    def _build_hint(self, text: str) -> QLabel:
        widget = QLabel(text)
        widget.setWordWrap(True)
        widget.setProperty("css", ["hint"])
        return widget

    def _build_none(self, _action) -> QWidget:
        return self._build_hint("Empty slot")

    def _build_mode_toggle(self, _action) -> QWidget:
        return self._build_hint(
            "The layer is toggled by the macropad firmware (status LED).")

    def _build_delay(self, action: Dict[str, Any]) -> QWidget:
        row = QWidget()
        layout = QHBoxLayout(row)
        layout.setContentsMargins(0, 0, 0, 0)
        spin = QDoubleSpinBox()
        spin.setRange(0.0, 600.0)
        spin.setSingleStep(0.1)
        spin.setDecimals(1)
        try:
            spin.setValue(float(action.get("seconds", 0.5)))
        except (TypeError, ValueError):
            spin.setValue(0.5)
        spin.valueChanged.connect(
            lambda value: self._set_field(action, "seconds", round(value, 1)))
        layout.addWidget(spin)
        layout.addWidget(QLabel("seconds"))
        layout.addStretch(1)
        return row

    def _build_shortcut(self, action: Dict[str, Any]) -> QWidget:
        widget = QWidget()
        layout = util.vbox(6, widget)

        mods = action.setdefault("modifiers", [])
        mod_row = QHBoxLayout()
        for modifier in config_module.MODIFIERS:
            box = QCheckBox(modifier.upper())
            box.setChecked(modifier in mods)
            box.toggled.connect(
                lambda checked, m=modifier: self._on_modifier(m, checked, action))
            mod_row.addWidget(box)
        mod_row.addStretch(1)
        layout.addLayout(mod_row)

        keys: List[str] = []
        for _group, group_keys in config_module.KEY_GROUPS:
            keys.extend(group_keys)
        combo = QComboBox()
        combo.addItems(keys)
        if action.get("key", "A") in keys:
            combo.setCurrentIndex(keys.index(action.get("key", "A")))
        combo.currentIndexChanged.connect(
            lambda index: self._set_field(action, "key", keys[index]))
        layout.addWidget(combo)
        layout.addWidget(self._test_widget(), alignment=Qt.AlignLeft)
        return widget

    def _on_modifier(self, modifier: str, checked: bool,
                     action: Dict[str, Any]) -> None:
        modifiers = list(action.get("modifiers", []))
        if checked and modifier not in modifiers:
            modifiers.append(modifier)
        elif not checked and modifier in modifiers:
            modifiers.remove(modifier)
        action["modifiers"] = [m for m in config_module.MODIFIERS if m in modifiers]
        self._touch()

    def _build_text(self, action: Dict[str, Any]) -> QWidget:
        widget = QWidget()
        layout = util.vbox(6, widget)
        view = QPlainTextEdit()
        view.setPlainText(action.get("text", ""))
        view.setFixedHeight(70)
        view.textChanged.connect(
            lambda: self._set_field(action, "text", view.toPlainText()))
        layout.addWidget(view)
        layout.addWidget(self._test_widget(), alignment=Qt.AlignLeft)
        return widget

    def _build_media(self, action: Dict[str, Any]) -> QWidget:
        widget = QWidget()
        layout = util.vbox(6, widget)
        ids = [media_id for media_id, _ in config_module.MEDIA_ACTIONS]
        combo = QComboBox()
        for media_id, label in config_module.MEDIA_ACTIONS:
            combo.addItem(label, media_id)
        if action.get("mediaKey") in ids:
            combo.setCurrentIndex(ids.index(action.get("mediaKey")))
        combo.currentIndexChanged.connect(
            lambda index: self._set_field(action, "mediaKey", ids[index]))
        layout.addWidget(combo)
        layout.addWidget(self._test_widget(), alignment=Qt.AlignLeft)
        return widget

    def _build_audio_output(self, action: Dict[str, Any]) -> QWidget:
        widget = QWidget()
        layout = util.vbox(6, widget)
        sinks = [s for s in (action.get("sinks") or []) if isinstance(s, dict)]
        if sinks:
            for position, sink in enumerate(sinks, start=1):
                layout.addWidget(QLabel(
                    f"{position}. {sink.get('description') or sink.get('name', '')}"))
        else:
            layout.addWidget(util.hint("No output selected"))
        row = QHBoxLayout()
        choose = QPushButton("Choose outputs...")
        choose.clicked.connect(lambda: self._pick_outputs(action))
        row.addWidget(choose)
        row.addWidget(self._test_widget())
        row.addStretch(1)
        layout.addLayout(row)
        return widget

    def _pick_outputs(self, action: Dict[str, Any]) -> None:
        from ui.audio_picker import AudioOutputPickerDialog

        def on_pick(sinks: List[Dict[str, str]]) -> None:
            action["sinks"] = sinks
            self._touch()
            self.editor.refresh()

        AudioOutputPickerDialog(self.editor.window, action.get("sinks") or [],
                                on_pick).exec()

    def _build_launch_app(self, action: Dict[str, Any]) -> QWidget:
        widget = QWidget()
        layout = util.vbox(6, widget)
        row = QHBoxLayout()
        name = action.get("appName") or action.get("desktopId") or "Not selected"
        row.addWidget(QLabel(name), 1)
        choose = QPushButton("Choose...")
        choose.clicked.connect(lambda: self._pick_app(action))
        row.addWidget(choose)
        row.addWidget(self._test_widget())
        layout.addLayout(row)
        return widget

    def _pick_app(self, action: Dict[str, Any]) -> None:
        from ui.app_picker import AppPickerDialog

        def on_pick(app: Dict[str, str]) -> None:
            action["desktopId"] = app.get("id") or app.get("path", "")
            action["appName"] = app.get("name", "")
            self._touch()
            self.editor.refresh()

        AppPickerDialog(self.editor.window, on_pick).exec()

    def _build_system_action(self, action: Dict[str, Any]) -> QWidget:
        widget = QWidget()
        layout = util.vbox(6, widget)
        combo = QComboBox()
        for preset in SYSTEM_ACTION_PRESETS:
            combo.addItem(preset["name"], preset["id"])
        ids = [preset["id"] for preset in SYSTEM_ACTION_PRESETS]
        if action.get("presetId") in ids:
            combo.setCurrentIndex(ids.index(action["presetId"]))

        def on_selected(index: int) -> None:
            preset = SYSTEM_ACTION_PRESETS[index]
            action["presetId"] = preset["id"]
            action["presetName"] = preset["name"]
            self.info.setToolTip(preset.get("description", ""))
            self._touch()

        combo.currentIndexChanged.connect(on_selected)
        layout.addWidget(combo)
        layout.addWidget(self._test_widget(), alignment=Qt.AlignLeft)
        return widget

    def _build_bash_script(self, action: Dict[str, Any]) -> QWidget:
        widget = QWidget()
        layout = util.vbox(6, widget)
        view = QPlainTextEdit()
        view.setPlainText(action.get("script", ""))
        view.setFixedHeight(80)
        view.textChanged.connect(
            lambda: self._set_field(action, "script", view.toPlainText()))
        layout.addWidget(view)
        layout.addWidget(self._test_widget(), alignment=Qt.AlignLeft)
        return widget

    def _build_ha_webhook(self, action: Dict[str, Any]) -> QWidget:
        widget = QWidget()
        layout = util.vbox(6, widget)
        entry = QLineEdit(action.get("url", ""))
        entry.setPlaceholderText(
            "http://homeassistant.local:8123/api/webhook/my_macro")
        entry.textChanged.connect(lambda text: self._set_field(action, "url", text))
        layout.addWidget(entry)
        layout.addWidget(self._test_widget(), alignment=Qt.AlignLeft)
        return widget

    def _build_home_assistant(self, action: Dict[str, Any]) -> QWidget:
        from home_assistant import ha_client
        widget = QWidget()
        layout = util.vbox(6, widget)

        entity_id = action.get("entityId", "")
        if not ha_client.is_configured():
            warning = QLabel("Home Assistant not configured")
            warning.setProperty("css", ["warn"])
            layout.addWidget(warning)
        elif entity_id:
            layout.addWidget(QLabel(action.get("friendlyName") or entity_id))
        else:
            layout.addWidget(QLabel("No entity selected"))

        buttons = QHBoxLayout()
        pick = QPushButton("Entity")
        pick.clicked.connect(self._pick_entity)
        buttons.addWidget(pick)
        conn = QPushButton("Connection...")
        conn.clicked.connect(self._open_ha_connection)
        buttons.addWidget(conn)
        buttons.addStretch(1)
        layout.addLayout(buttons)

        selected = next((e for e in state.ha_entities
                         if e.get("entity_id") == action.get("entityId")), None)
        services = list((selected or {}).get("services")
                        or ["toggle", "turn_on", "turn_off"])
        if action.get("service") not in services and self.action.get("entityId"):
            self.action["service"] = services[0]

        row = QHBoxLayout()
        row.addWidget(QLabel("Service:"))
        service_combo = QComboBox()
        service_combo.addItems(services)
        if action.get("service") in services:
            service_combo.setCurrentIndex(services.index(action["service"]))
        service_combo.currentIndexChanged.connect(
            lambda index: self._set_field(action, "service", services[index]))
        row.addWidget(service_combo)
        row.addStretch(1)
        test = self._test_widget()
        test.setEnabled(bool(action.get("entityId")))
        row.addWidget(test)
        layout.addLayout(row)
        return widget

    def _pick_entity(self) -> None:
        from ui.ha_dialogs import EntityPickerDialog

        def on_pick(entity: Dict[str, Any]) -> None:
            action = self.action
            action["entityId"] = entity.get("entity_id", "")
            action["friendlyName"] = entity.get("friendly_name", "")
            action["domain"] = entity.get("domain", "")
            services = entity.get("services") or ["toggle", "turn_on", "turn_off"]
            action["service"] = services[0]
            self._touch()
            self.editor.refresh()

        EntityPickerDialog(self.editor.window, on_pick).exec()

    def _open_ha_connection(self) -> None:
        from ui.ha_dialogs import HaConnectionDialog
        HaConnectionDialog(self.editor.window).exec()

    # ------------------------------------------------------------------
    def _set_field(self, action: Dict[str, Any], field: str, value: Any) -> None:
        if action.get(field) == value:
            return
        action[field] = value
        self._touch()

    def _touch(self) -> None:
        state.mark_dirty()
        state.emit("button-changed", index=state.selected_index)
