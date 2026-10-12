#!/usr/bin/env python3
"""OctoPad - Buttons page: key grid + per-trigger action inspector."""

from typing import Any, Dict, List

from PySide6.QtWidgets import (
    QButtonGroup, QGridLayout, QHBoxLayout, QLabel, QLineEdit, QPushButton,
    QScrollArea, QTabWidget, QWidget,
)

import config as config_module
import state
from ui import util
from ui.action_editor import ActionEditor

_TRIGGER_SHORT = {"single": "Single", "double": "Double", "hold": "Hold"}


class KeysPage(QWidget):
    def __init__(self, window) -> None:
        super().__init__()
        self.window = window
        self._syncing = False
        self._active_trigger = "single"
        self.cards: List[Dict[str, Any]] = []

        root = util.vbox(10, self)
        root.addLayout(self._build_header())

        panes = QHBoxLayout()
        panes.setSpacing(16)
        panes.addWidget(self._build_grid_host(), 1)
        panes.addWidget(self._build_inspector_host())
        root.addLayout(panes, 1)

        state.subscribe(self._on_state_event)
        self._sync_mode_switch()
        self.refresh_grid()
        self.rebuild_inspector()

    # ------------------------------------------------------------------
    def _build_header(self) -> QHBoxLayout:
        row = QHBoxLayout()
        self.mode_group = QButtonGroup(self)
        self.mode_group.setExclusive(True)
        self.mode_buttons: List[QPushButton] = []
        for index, name in enumerate(["Desktop", "Home Assistant"]):
            button = QPushButton(name)
            button.setCheckable(True)
            button.setChecked(index == state.mode_index)
            button.clicked.connect(lambda _c, i=index: self._on_mode_clicked(i))
            self.mode_group.addButton(button, index)
            self.mode_buttons.append(button)
            row.addWidget(button)
        row.addStretch(1)

        self.timing_label = QLabel()
        self.timing_label.setToolTip(self._timing_text())
        row.addWidget(self.timing_label)
        return row

    @staticmethod
    def _timing_text() -> str:
        cfg = state.cfg
        return (f"Debounce {cfg['debounceMs']} ms | Click {cfg['clickTimeoutMs']} ms | "
                f"Hold {cfg['holdTimeoutMs']} ms")

    def _on_mode_clicked(self, index: int) -> None:
        if self._syncing:
            return
        state.set_mode(index)

    def _sync_mode_switch(self) -> None:
        self._syncing = True
        self.mode_buttons[state.mode_index].setChecked(True)
        self._syncing = False

    # ------------------------------------------------------------------
    def _build_grid_host(self) -> QWidget:
        host = QWidget()
        layout = QGridLayout(host)
        layout.setSpacing(12)
        for index in range(8):
            card = self._build_card(index)
            self.cards.append(card)
            layout.addWidget(card["widget"], index // 4, index % 4)
        return host

    def _build_card(self, index: int) -> Dict[str, Any]:
        widget = QPushButton()
        widget.setMinimumSize(160, 118)
        widget.setCheckable(False)
        widget.clicked.connect(lambda _c, i=index: state.select_button(i))
        return {"widget": widget, "index": index}

    def refresh_grid(self) -> None:
        for position, card in enumerate(self.cards):
            button_cfg = config_module.get_button(state.cfg, state.mode_index, position)
            name = button_cfg.get("label", f"Button {position + 1}")
            lines = [f"B{position + 1}", name]
            for trigger in ("single", "double", "hold"):
                actions = config_module.actions_list(button_cfg.get(trigger))
                armed = [a for a in actions
                         if isinstance(a, dict) and a.get("type", "none") != "none"]
                tag = _TRIGGER_SHORT[trigger][0].upper()
                badge = util.action_badge(armed[0].get("type", "none")) if armed else "-"
                lines.append(f"{tag}:{badge}")
            card["widget"].setText("\n".join(lines))
            active = position == state.selected_index
            card["widget"].setProperty("active", active)
            card["widget"].setToolTip(
                f"Button {position + 1} | GPIO {config_module.PINOUT[position]}")
            card["widget"].style().unpolish(card["widget"])
            card["widget"].style().polish(card["widget"])

    # ------------------------------------------------------------------
    def _build_inspector_host(self) -> QWidget:
        host = QWidget()
        host.setFixedWidth(420)
        layout = util.vbox(8, host)

        self.inspector_title = QLabel("Button 1")
        layout.addWidget(self.inspector_title)

        self.label_edit = QLineEdit()
        self.label_edit.setPlaceholderText("Button label")
        self.label_edit.textEdited.connect(self._on_label_edited)
        layout.addWidget(self.label_edit)

        self.tabs = QTabWidget()
        self.editors: Dict[str, ActionEditor] = {}
        for trigger, label in config_module.TRIGGERS:
            editor = ActionEditor(self.window, trigger, label)
            self.editors[trigger] = editor
            self.tabs.addTab(editor, _TRIGGER_SHORT[trigger])
        self.tabs.currentChanged.connect(self._on_tab_changed)
        layout.addWidget(self.tabs, 1)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(host)
        scroll.setFixedWidth(440)
        return scroll

    def rebuild_inspector(self) -> None:
        self.inspector_title.setText(
            f"Button {state.selected_index + 1} - "
            f"{state.cfg['modes'][state.mode_index].get('name', '')}")
        self.label_edit.setText(state.current_button().get("label", ""))
        for editor in self.editors.values():
            editor.refresh()

    def _on_label_edited(self, text: str) -> None:
        if text != state.current_button().get("label", ""):
            state.set_label(text)

    def _on_tab_changed(self, index: int) -> None:
        if 0 <= index < len(config_module.TRIGGERS):
            self._active_trigger = config_module.TRIGGERS[index][0]

    # ------------------------------------------------------------------
    def _on_state_event(self, event: str, **data: Any) -> None:
        if event in ("mode-changed", "config-replaced"):
            self._sync_mode_switch()
            self.refresh_grid()
            self.rebuild_inspector()
        elif event == "selection-changed":
            self.refresh_grid()
            self.rebuild_inspector()
        elif event == "button-changed":
            self.refresh_grid()
            for editor in self.editors.values():
                editor.refresh()
        elif event == "timing-changed":
            self.timing_label.setToolTip(self._timing_text())
        elif event == "ha-entities-changed":
            for editor in self.editors.values():
                editor.refresh()
        elif event == "hardware-event":
            self.flash(int(data.get("index", -1)))

    def flash(self, index: int) -> None:
        if not 0 <= index < len(self.cards):
            return
        widget = self.cards[index]["widget"]
        widget.setProperty("pressed", True)
        widget.style().unpolish(widget)
        widget.style().polish(widget)
        from PySide6.QtCore import QTimer
        QTimer.singleShot(180, lambda: self._clear_flash(widget))

    @staticmethod
    def _clear_flash(widget: QPushButton) -> None:
        widget.setProperty("pressed", False)
        widget.style().unpolish(widget)
        widget.style().polish(widget)
