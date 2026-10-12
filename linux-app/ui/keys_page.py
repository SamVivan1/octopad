#!/usr/bin/env python3
"""
OctoPad - Button configuration page.

Visual grid of 8 keycaps + mode switch + per-trigger action inspector.
"""

from typing import Any, Dict, List

import gi
gi.require_version("Gtk", "4.0")
from gi.repository import Gtk, GLib, Pango  # noqa: E402

import config as config_module
import state
from ui.action_editor import ActionEditor
from ui import util


_TRIGGER_SHORT = {"single": "Single", "double": "Double", "hold": "Hold"}


class KeysPage(Gtk.Box):
    def __init__(self, window) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self.add_css_class("mp-page")
        self.window = window
        self._syncing = False
        self._active_trigger = "single"
        self.cards: List[Dict[str, Any]] = []

        self.append(self._build_header())

        panes = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=16)
        panes.set_vexpand(True)

        grid_host = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        grid_host.set_hexpand(True)
        grid_host.set_valign(Gtk.Align.CENTER)
        grid_host.set_halign(Gtk.Align.CENTER)
        grid_host.add_css_class("mp-chassis")

        self.grid = Gtk.Grid(column_spacing=12, row_spacing=12)
        self.grid.set_halign(Gtk.Align.CENTER)
        for index in range(8):
            card = self._build_card(index)
            self.cards.append(card)
            self.grid.attach(card["widget"], index % 4, index // 4, 1, 1)
        grid_host.append(self.grid)
        panes.append(grid_host)

        self.inspector = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.inspector.set_size_request(392, -1)
        self.inspector.add_css_class("mp-inspector")
        inspector_scroll = Gtk.ScrolledWindow(hexpand=False, vexpand=True)
        inspector_scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        inspector_scroll.set_child(self.inspector)
        inspector_scroll.add_css_class("mp-panel")
        inspector_scroll.set_size_request(400, -1)
        panes.append(inspector_scroll)

        self.append(panes)

        state.subscribe(self._on_state_event)
        self._sync_mode_switch()
        self.refresh_grid()
        self.rebuild_inspector()

    # ------------------------------------------------------------------
    # Header: mode switch + timing
    # ------------------------------------------------------------------
    def _build_header(self) -> Gtk.Widget:
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)

        self.mode_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        self.mode_box.set_css_classes(["linked", "mp-mode-switch"])
        self.mode_buttons: List[Gtk.ToggleButton] = []
        for index, name in enumerate(["Desktop", "Home Assistant"]):
            button = Gtk.ToggleButton(label=name)
            button.set_active(index == state.mode_index)
            if index > 0:
                button.set_group(self.mode_buttons[0])
            button.connect("toggled", self._on_mode_toggled, index)
            self.mode_buttons.append(button)
            self.mode_box.append(button)
        row.append(self.mode_box)

        spacer = Gtk.Box()
        spacer.set_hexpand(True)
        row.append(spacer)
        self.timing_info = util.info_icon(self._timing_text())
        row.append(self.timing_info)
        return row

    @staticmethod
    def _timing_text() -> str:
        cfg = state.cfg
        return (f"Debounce {cfg['debounceMs']} ms  ·  "
                f"Click {cfg['clickTimeoutMs']} ms  ·  "
                f"Hold {cfg['holdTimeoutMs']} ms")

    def _on_mode_toggled(self, button: Gtk.ToggleButton, index: int) -> None:
        if self._syncing or not button.get_active():
            return
        state.set_mode(index)

    def _sync_mode_switch(self) -> None:
        self._syncing = True
        self.mode_buttons[state.mode_index].set_active(True)
        self._syncing = False

    # ------------------------------------------------------------------
    # Button cards
    # ------------------------------------------------------------------
    def _build_card(self, index: int) -> Dict[str, Any]:
        button = Gtk.Button()
        button.set_css_classes(["mp-key"])
        button.set_has_frame(False)
        button.set_size_request(160, 120)

        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        body.set_vexpand(True)
        body.set_valign(Gtk.Align.CENTER)

        head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        number = util.label(str(index + 1), css=["mp-key-number"])
        number.set_xalign(0.5)
        number.set_valign(Gtk.Align.START)
        head.append(number)

        spacer = Gtk.Box()
        spacer.set_hexpand(True)
        head.append(spacer)

        edit_badge = Gtk.Image.new_from_icon_name("document-edit-symbolic")
        edit_badge.set_css_classes(["mp-key-edit"])
        edit_badge.set_valign(Gtk.Align.START)
        edit_badge.set_visible(False)
        head.append(edit_badge)
        body.append(head)

        name_label = util.label("—", css=["mp-key-label"])
        name_label.set_ellipsize(Pango.EllipsizeMode.END)
        name_label.set_xalign(0.5)
        name_label.set_hexpand(True)
        body.append(name_label)

        # One icon slot per trigger (Single / Double / Hold). The slot shows the
        # type icon of the first action, colour-coded to the trigger; an empty
        # slot is a faint placeholder. Details live in the tooltip.
        actions_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        actions_row.set_css_classes(["mp-key-actions"])
        actions_row.set_halign(Gtk.Align.CENTER)
        slots: List[Gtk.Image] = []
        for _trigger, _label in config_module.TRIGGERS:
            icon = Gtk.Image.new_from_icon_name("action-unavailable-symbolic")
            icon.set_pixel_size(18)
            icon.set_css_classes(["mp-key-action", "mp-key-action-empty"])
            slots.append(icon)
            actions_row.append(icon)
        body.append(actions_row)

        button.set_child(body)
        button.connect("clicked", self._on_card_clicked, index)

        return {
            "widget": button, "name": name_label, "slots": slots,
            "edit": edit_badge, "number": number, "index": index,
        }

    def _on_card_clicked(self, _button, index: int) -> None:
        state.select_button(index)

    def refresh_grid(self) -> None:
        for position, card in enumerate(self.cards):
            button_cfg = config_module.get_button(state.cfg, state.mode_index, position)
            card["name"].set_text(button_cfg.get("label", f"Button {position + 1}"))

            active = position == state.selected_index
            card["widget"].set_css_classes(
                ["mp-key"] + (["mp-key-active"] if active else []))
            card["edit"].set_visible(active)

            for slot, trigger in zip(card["slots"], ("single", "double", "hold")):
                actions = config_module.actions_list(button_cfg.get(trigger))
                armed = [a for a in actions
                         if isinstance(a, dict)
                         and a.get("type", "none") != "none"]
                label = _TRIGGER_SHORT.get(trigger, trigger)
                if armed:
                    slot.set_from_icon_name(
                        util.action_icon(armed[0].get("type", "none")))
                    slot.set_css_classes(
                        ["mp-key-action", "mp-key-action-armed",
                         f"mp-trig-{trigger}"])
                    detail = "\n".join(
                        config_module.action_summary(a) for a in armed)
                    slot.set_tooltip_text(f"{label} · {detail}")
                else:
                    slot.set_from_icon_name("action-unavailable-symbolic")
                    slot.set_css_classes(["mp-key-action", "mp-key-action-empty"])
                    slot.set_tooltip_text(f"{label} · empty")

            card["widget"].set_tooltip_text(
                f"Button {position + 1} · GPIO {config_module.PINOUT[position]}")

    # ------------------------------------------------------------------
    # Inspector
    # ------------------------------------------------------------------
    def rebuild_inspector(self) -> None:
        child = self.inspector.get_first_child()
        while child is not None:
            following = child.get_next_sibling()
            self.inspector.remove(child)
            child = following

        button_cfg = state.current_button()
        mode_name = state.cfg["modes"][state.mode_index].get("name", "")

        head = util.horizontal(spacing=8)
        edit_icon = Gtk.Image.new_from_icon_name("document-edit-symbolic")
        edit_icon.set_css_classes(["mp-accent-icon"])
        head.append(edit_icon)
        badge = util.label(f"Button {state.selected_index + 1}",
                           css=["mp-pill", "mp-key-active"])
        head.append(badge)
        head.append(util.label(mode_name, css=["mp-hint"]))
        self.inspector.append(head)

        label_entry = Gtk.Entry()
        label_entry.set_text(button_cfg.get("label", ""))
        label_entry.set_placeholder_text("Button name…")
        label_entry.set_hexpand(True)
        label_entry.connect("changed", self._on_label_changed)
        self.inspector.append(label_entry)

        self.editors: List[ActionEditor] = []
        self.editors_by_trigger: Dict[str, ActionEditor] = {}

        switcher = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        switcher.set_css_classes(["linked", "mp-trig-switch"])
        self.trig_buttons: Dict[str, Gtk.ToggleButton] = {}

        self.stack = Gtk.Stack()
        self.stack.set_transition_type(Gtk.StackTransitionType.CROSSFADE)
        self.stack.set_vexpand(True)

        group_button = None
        for trigger, trigger_label in config_module.TRIGGERS:
            editor = ActionEditor(self.window, trigger, trigger_label)
            self.editors.append(editor)
            self.editors_by_trigger[trigger] = editor
            self.stack.add_named(editor, trigger)

            button = Gtk.ToggleButton(label=_TRIGGER_SHORT.get(trigger, trigger))
            button.set_hexpand(True)
            button.set_css_classes([f"mp-trig-{trigger}"])
            if group_button is None:
                group_button = button
            else:
                button.set_group(group_button)
            button.connect("toggled", self._on_trigger_toggled, trigger)
            self.trig_buttons[trigger] = button
            switcher.append(button)

        self._syncing = True
        active_button = self.trig_buttons.get(self._active_trigger)
        if active_button is not None:
            active_button.set_active(True)
        self._syncing = False
        self.stack.set_visible_child_name(self._active_trigger)

        self.inspector.append(switcher)
        self.inspector.append(self.stack)
        self._update_trigger_tabs()

    # ------------------------------------------------------------------
    def _on_trigger_toggled(self, button: Gtk.ToggleButton, trigger: str) -> None:
        if self._syncing or not button.get_active():
            return
        self._active_trigger = trigger
        self.stack.set_visible_child_name(trigger)

    def _update_trigger_tabs(self) -> None:
        """Refresh the tab labels with the number of active actions per trigger."""
        for trigger, button in self.trig_buttons.items():
            actions = config_module.actions_list(
                state.current_button().get(trigger))
            count = sum(1 for a in actions
                        if isinstance(a, dict) and a.get("type") not in (None, "none"))
            text = _TRIGGER_SHORT.get(trigger, trigger)
            if count:
                text = f"{text} · {count}"
            if button.get_label() != text:
                button.set_label(text)

    def _on_label_changed(self, entry: Gtk.Entry) -> None:
        if state.current_button().get("label") == entry.get_text():
            return
        state.set_label(entry.get_text())

    # ------------------------------------------------------------------
    # Event bus
    # ------------------------------------------------------------------
    def _on_state_event(self, event: str, **data: Any) -> None:
        if event in ("mode-changed", "config-replaced"):
            self._sync_mode_switch()
            self.refresh_grid()
            self.rebuild_inspector()
            self.timing_info.set_tooltip_text(self._timing_text())
        elif event == "selection-changed":
            self.refresh_grid()
            self.rebuild_inspector()
        elif event == "button-changed":
            self.refresh_grid()
            self._update_trigger_tabs()
        elif event == "timing-changed":
            self.timing_info.set_tooltip_text(self._timing_text())
        elif event == "ha-entities-changed":
            self.rebuild_inspector()
        elif event == "hardware-event":
            self.flash(int(data.get("index", -1)))

    # ------------------------------------------------------------------
    def flash(self, index: int) -> None:
        """“Pressed” animation when the physical key is pressed."""
        if not 0 <= index < len(self.cards):
            return
        widget = self.cards[index]["widget"]
        widget.add_css_class("mp-key-pressed")

        def release() -> bool:
            widget.remove_css_class("mp-key-pressed")
            return False

        GLib.timeout_add(180, release)
