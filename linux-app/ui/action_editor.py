#!/usr/bin/env python3
"""
OctoPad - Action editor per trigger (Single / Double / Hold).

Each trigger can hold multiple actions. `ActionEditor` hosts a list of
`_ActionCard` widgets (one per action) with a "+" button to add and an "×"
button to remove. Every change writes straight to state and emits an event.

The editor is deliberately compact: the action type is chosen through an
icon-driven menu, per-type explanations live in a ⓘ tooltip, and every card
carries a colour bar + step number so a long stack stays readable.
"""

from typing import Any, Dict, List

import gi
gi.require_version("Gtk", "4.0")
from gi.repository import Gtk, Pango  # noqa: E402

import config as config_module
import state
from system_actions import SYSTEM_ACTION_PRESETS, execute_action
from ui import util


def _copy_action(action: Dict[str, Any]) -> Dict[str, Any]:
    import copy
    return copy.deepcopy(action)


class ActionEditor(Gtk.Box):
    """Editor for a single trigger: action list + add button."""

    def __init__(self, window, trigger: str, trigger_label: str) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self.window = window
        self.trigger = trigger
        self._cards: List["_ActionCard"] = []

        toolbar = util.horizontal(spacing=4)
        toolbar.set_css_classes(["mp-ae-toolbar"])
        spacer = Gtk.Box()
        spacer.set_hexpand(True)
        toolbar.append(spacer)

        test_all = util.icon_button(
            "media-playback-start-symbolic",
            "Test every action on this trigger", self._test_all,
            ["flat", "mp-move"])
        toolbar.append(test_all)

        add_button = util.icon_button(
            "list-add-symbolic", "Add an action", self._add_action,
            ["flat", "suggested-action", "mp-move"])
        toolbar.append(add_button)
        self.append(toolbar)

        self.content = util.vertical(spacing=8)
        self.append(self.content)

        self.refresh()

    # ------------------------------------------------------------------
    def actions(self) -> List[Dict[str, Any]]:
        return state.get_actions(self.trigger)

    def refresh(self) -> None:
        child = self.content.get_first_child()
        while child is not None:
            following = child.get_next_sibling()
            self.content.remove(child)
            child = following

        self._cards = []
        actions = self.actions() or [config_module.default_action("none")]
        for index, action in enumerate(actions):
            card = _ActionCard(self, index, action)
            self._cards.append(card)
            self.content.append(card)

    def replace_action(self, index: int, action: Dict[str, Any]) -> None:
        state.set_action(self.trigger, index, action)
        self.refresh()

    def _add_action(self, _button) -> None:
        state.add_action(self.trigger)
        self.refresh()

    def remove_action(self, index: int) -> None:
        state.remove_action(self.trigger, index)
        self.refresh()

    def move_action(self, index: int, delta: int) -> None:
        state.move_action(self.trigger, index, delta)
        self.refresh()

    # ------------------------------------------------------------------
    # Test every action on the trigger (threaded, result -> toast)
    # ------------------------------------------------------------------
    def _test_all(self, _button) -> None:
        actions = [a for a in self.actions()
                   if a.get("type", "none") != "none"]
        if not actions:
            self.window.show_toast("No actions on this trigger", "info")
            return

        def work():
            results: List[Any] = []
            for action in actions:
                results.append(execute_action(action.get("type", "none"), action))
            return results

        def done(results: List[Any]) -> None:
            ok = all(isinstance(r, tuple) and len(r) == 2 and r[0] for r in results)
            messages = [r[1] if isinstance(r, tuple) and len(r) == 2 else str(r)
                        for r in results]
            self.window.show_toast("Test: " + " | ".join(messages),
                                   "success" if ok else "error")

        util.run_async(work, done)


class _ActionCard(Gtk.Box):
    """A single card: one action on one trigger."""

    def __init__(self, editor: ActionEditor, index: int,
                 action: Dict[str, Any]) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self.set_css_classes(["mp-ae-card", f"mp-ae-{editor.trigger}"])
        self.editor = editor
        self.index = index
        self.action = action
        self._syncing = False

        head = util.horizontal(spacing=6)

        step = util.label(str(index + 1),
                          css=["mp-ae-step", f"mp-trig-{editor.trigger}"])
        step.set_xalign(0.5)
        step.set_valign(Gtk.Align.CENTER)
        head.append(step)

        self.type_combo = util.icon_dropdown(self._type_items())
        self.type_combo.set_hexpand(True)
        self.type_combo.set_valign(Gtk.Align.CENTER)
        self._syncing = True
        self.type_combo.set_selected(self._current_index())
        self._syncing = False
        self.type_combo.connect("notify::selected", self._on_type_changed)
        head.append(self.type_combo)

        self.info = util.info_icon(
            util.ACTION_HINTS.get(action.get("type", "none"), ""))
        head.append(self.info)

        total = len(editor.actions())
        up_button = util.icon_button(
            "go-up-symbolic", "Move up",
            lambda *_: self.editor.move_action(self.index, -1),
            ["flat", "mp-move"])
        up_button.set_sensitive(index > 0)
        head.append(up_button)

        down_button = util.icon_button(
            "go-down-symbolic", "Move down",
            lambda *_: self.editor.move_action(self.index, 1),
            ["flat", "mp-move"])
        down_button.set_sensitive(index < total - 1)
        head.append(down_button)

        remove_button = util.icon_button(
            "user-trash-symbolic", "Remove this action",
            lambda *_: self.editor.remove_action(self.index),
            ["flat", "mp-move"])
        head.append(remove_button)

        self.append(head)

        self.content = util.vertical(spacing=6)
        self.append(self.content)
        self._rebuild()

    # ------------------------------------------------------------------
    def _type_items(self) -> List[Any]:
        return [
            (type_id, util.ACTION_SHORT.get(type_id, label),
             util.action_icon(type_id))
            for type_id, label in config_module.ACTION_TYPES
        ]

    def _type_ids(self) -> List[str]:
        return [type_id for type_id, _ in config_module.ACTION_TYPES]

    def _current_index(self) -> int:
        try:
            return self._type_ids().index(self.action.get("type", "none"))
        except ValueError:
            return 0

    def _on_type_changed(self, *_args) -> None:
        if self._syncing:
            return
        type_id = self._type_ids()[self.type_combo.get_selected()]
        if self.action.get("type") == type_id:
            return
        self.editor.replace_action(self.index,
                                   config_module.default_action(type_id))

    def _rebuild(self) -> None:
        child = self.content.get_first_child()
        while child is not None:
            following = child.get_next_sibling()
            self.content.remove(child)
            child = following

        self.info.set_tooltip_text(
            util.ACTION_HINTS.get(self.action.get("type", "none"), ""))

        builder = getattr(self, f"_build_{self.action.get('type', 'none')}",
                          None)
        if builder is None:
            builder = self._build_none
        widget = builder(self.action)
        if widget is not None:
            self.content.append(widget)

    # ------------------------------------------------------------------
    def _test_button(self) -> Gtk.Widget:
        return util.icon_button(
            "media-playback-start-symbolic", "Test this action",
            self._test_action, ["flat", "suggested-action", "mp-move"])

    def _test_row(self) -> Gtk.Box:
        row = util.horizontal(spacing=6)
        row.append(self._test_button())
        return row

    # ------------------------------------------------------------------
    # Test this action on its own (threaded, result -> toast)
    # ------------------------------------------------------------------
    def _test_action(self, _button) -> None:
        action = _copy_action(self.action)

        def work():
            return execute_action(action.get("type", "none"), action)

        util.run_async(work, lambda result: util.toast_result(
            self.editor.window, result, "Test: "))

    # ------------------------------------------------------------------
    # Per-type builders (controls only — explanations live in the ⓘ tooltip)
    # ------------------------------------------------------------------
    def _build_none(self, _action) -> Gtk.Widget:
        placeholder = util.horizontal(spacing=8)
        placeholder.set_css_classes(["mp-ae-empty"])
        placeholder.append(Gtk.Image.new_from_icon_name("list-add-symbolic"))
        placeholder.append(util.label("Empty slot", css=["mp-hint"]))
        return placeholder

    def _build_mode_toggle(self, _action) -> Gtk.Widget:
        return util.vertical(spacing=0)

    def _build_delay(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=6)
        row = util.horizontal(spacing=8)
        row.append(Gtk.Image.new_from_icon_name("alarm-symbolic"))
        spin = Gtk.SpinButton.new_with_range(0.0, 600.0, 0.1)
        spin.set_digits(1)
        spin.set_numeric(True)
        try:
            spin.set_value(float(action.get("seconds", 0.5)))
        except (TypeError, ValueError):
            spin.set_value(0.5)
        spin.connect("value-changed", lambda w: self._set_field(
            action, "seconds", round(w.get_value(), 1)))
        row.append(spin)
        row.append(util.label("seconds", css=["mp-hint"]))
        box.append(row)
        return box

    def _build_shortcut(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=8)

        mods = action.setdefault("modifiers", [])
        mod_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        mod_box.set_css_classes(["linked"])
        for modifier in config_module.MODIFIERS:
            toggle = Gtk.ToggleButton(label=modifier.upper())
            toggle.set_active(modifier in mods)
            toggle.connect("toggled", self._on_modifier_toggle, modifier, action)
            mod_box.append(toggle)
        box.append(mod_box)

        keys: List[str] = []
        for _group_name, group_keys in config_module.KEY_GROUPS:
            keys.extend(group_keys)
        combo = Gtk.DropDown.new_from_strings(keys)
        try:
            combo.set_selected(keys.index(action.get("key", "A")))
        except ValueError:
            combo.set_selected(0)
        combo.connect("notify::selected",
                      lambda w, *_: self._set_field(action, "key", keys[w.get_selected()]))

        row = util.horizontal(spacing=8)
        row.append(Gtk.Image.new_from_icon_name("input-keyboard-symbolic"))
        row.append(combo)
        box.append(row)

        box.append(self._test_row())
        return box

    def _on_modifier_toggle(self, button: Gtk.ToggleButton, modifier: str,
                            action: Dict[str, Any]) -> None:
        modifiers = list(action.get("modifiers", []))
        if button.get_active():
            if modifier not in modifiers:
                modifiers.append(modifier)
        elif modifier in modifiers:
            modifiers.remove(modifier)
        action["modifiers"] = [m for m in config_module.MODIFIERS if m in modifiers]
        state.mark_dirty()
        state.emit("button-changed", index=state.selected_index)

    def _build_text(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=6)
        view = Gtk.TextView(editable=True, wrap_mode=Gtk.WrapMode.CHAR, monospace=True)
        view.set_css_classes(["mp-console"])
        view.get_buffer().set_text(action.get("text", ""))
        view.get_buffer().connect("changed", self._on_text_changed, action)
        scroll = Gtk.ScrolledWindow(vexpand=True, hexpand=True)
        scroll.set_min_content_height(64)
        scroll.set_child(view)
        box.append(scroll)
        box.append(self._test_row())
        return box

    def _on_text_changed(self, buffer, action: Dict[str, Any]) -> None:
        start, end = buffer.get_bounds()
        action["text"] = buffer.get_text(start, end, True)
        state.mark_dirty()
        state.emit("button-changed", index=state.selected_index)

    def _build_media(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=8)
        media_ids = [media_id for media_id, _ in config_module.MEDIA_ACTIONS]
        items = [
            (media_id, label,
             util.MEDIA_ICONS.get(media_id, "audio-volume-high-symbolic"))
            for media_id, label in config_module.MEDIA_ACTIONS
        ]
        combo = util.icon_dropdown(items)
        try:
            combo.set_selected(media_ids.index(action.get("mediaKey", "VOL_UP")))
        except ValueError:
            combo.set_selected(0)
        combo.connect("notify::selected", lambda w, *_: self._set_field(
            action, "mediaKey", media_ids[w.get_selected()]))
        box.append(combo)
        box.append(self._test_row())
        return box

    def _build_audio_output(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=6)
        sinks = [s for s in (action.get("sinks") or []) if isinstance(s, dict)]

        if sinks:
            for position, sink in enumerate(sinks, start=1):
                row = util.horizontal(spacing=8)
                row.append(Gtk.Image.new_from_icon_name("audio-speakers-symbolic"))
                name = util.label(sink.get("description") or sink.get("name", ""))
                name.set_hexpand(True)
                name.set_ellipsize(Pango.EllipsizeMode.END)
                name.set_tooltip_text(sink.get("name", ""))
                row.append(name)
                row.append(util.label(str(position), css=["mp-hint"]))
                box.append(row)
        else:
            box.append(util.label("No output selected", css=["mp-hint"]))

        row = util.horizontal(spacing=8)
        row.append(util.button("Choose outputs…", "view-list-symbolic",
                               None, self._pick_outputs, action))
        row.append(self._test_button())
        box.append(row)
        return box

    def _pick_outputs(self, _btn, action: Dict[str, Any]) -> None:
        from ui.audio_picker import AudioOutputPickerDialog

        def on_pick(sinks: List[Dict[str, str]]) -> None:
            action["sinks"] = sinks
            state.mark_dirty()
            state.emit("button-changed", index=state.selected_index)
            self.editor.refresh()

        AudioOutputPickerDialog(self.editor.window, action.get("sinks") or [],
                                on_pick).present()

    def _build_launch_app(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=8)

        name = action.get("appName") or action.get("desktopId") or "Not selected"
        row = util.horizontal(spacing=8)
        row.append(Gtk.Image.new_from_icon_name("system-run-symbolic"))
        label_widget = util.label(name)
        label_widget.set_hexpand(True)
        label_widget.set_ellipsize(Pango.EllipsizeMode.END)
        row.append(label_widget)

        choose = util.icon_button("view-more-symbolic", "Choose an application",
                                  self._pick_app, ["flat", "mp-move"], action)
        row.append(choose)
        row.append(self._test_button())
        box.append(row)

        if action.get("desktopId"):
            self.info.set_tooltip_text(
                f"{util.ACTION_HINTS['launch_app']}\nDesktop entry: "
                f"{action['desktopId']}")
        return box

    def _pick_app(self, _btn, action: Dict[str, Any]) -> None:
        from ui.app_picker import AppPickerDialog

        def on_pick(app: Dict[str, str]) -> None:
            action["desktopId"] = app.get("id") or app.get("path", "")
            action["appName"] = app.get("name", "")
            state.mark_dirty()
            state.emit("button-changed", index=state.selected_index)
            self.editor.refresh()

        AppPickerDialog(self.editor.window, on_pick).present()

    def _build_system_action(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=8)
        presets = SYSTEM_ACTION_PRESETS
        preset_ids = [preset["id"] for preset in presets]
        items = [
            (preset["id"], preset["name"], util.system_preset_icon(preset))
            for preset in presets
        ]
        combo = util.icon_dropdown(items)
        try:
            combo.set_selected(preset_ids.index(action.get("presetId", "")))
        except ValueError:
            combo.set_selected(0)

        def on_selected(widget, *_args) -> None:
            preset = presets[widget.get_selected()]
            action["presetId"] = preset["id"]
            action["presetName"] = preset["name"]
            description = preset.get("description", "")
            if description:
                self.info.set_tooltip_text(description)
            state.mark_dirty()
            state.emit("button-changed", index=state.selected_index)

        combo.connect("notify::selected", on_selected)
        selected = combo.get_selected()
        if 0 <= selected < len(presets):
            description = presets[selected].get("description", "")
            if description:
                self.info.set_tooltip_text(description)

        box.append(combo)
        box.append(self._test_row())
        return box

    def _build_bash_script(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=6)
        view = Gtk.TextView(editable=True, wrap_mode=Gtk.WrapMode.CHAR, monospace=True)
        view.set_css_classes(["mp-console"])
        view.get_buffer().set_text(action.get("script", ""))
        view.get_buffer().connect("changed", self._on_script_changed, action)
        scroll = Gtk.ScrolledWindow(hexpand=True)
        scroll.set_min_content_height(72)
        scroll.set_child(view)
        box.append(scroll)
        box.append(self._test_row())
        return box

    def _on_script_changed(self, buffer, action: Dict[str, Any]) -> None:
        start, end = buffer.get_bounds()
        action["script"] = buffer.get_text(start, end, True)
        state.mark_dirty()
        state.emit("button-changed", index=state.selected_index)

    def _build_ha_webhook(self, action: Dict[str, Any]) -> Gtk.Widget:
        box = util.vertical(spacing=6)
        entry = Gtk.Entry(placeholder_text="http://homeassistant.local:8123/api/webhook/my_macro")
        entry.set_text(action.get("url", ""))
        entry.set_hexpand(True)
        entry.connect("changed", lambda w: self._set_field(action, "url", w.get_text()))
        box.append(entry)
        box.append(self._test_row())
        return box

    def _build_home_assistant(self, action: Dict[str, Any]) -> Gtk.Widget:
        from home_assistant import ha_client

        box = util.vertical(spacing=8)
        theme = Gtk.IconTheme.get_for_display(self.editor.window.get_display())

        entity_id = action.get("entityId", "")
        if not ha_client.is_configured():
            warning = util.horizontal(spacing=6)
            warning.set_css_classes(["mp-pill", "mp-warn"])
            warning.append(Gtk.Image.new_from_icon_name("dialog-warning-symbolic"))
            warning.append(util.label("Home Assistant not configured"))
            box.append(warning)
        elif entity_id:
            head = util.horizontal(spacing=8)
            domain = entity_id.split(".")[0] if "." in entity_id else ""
            head.append(Gtk.Image.new_from_icon_name(util.domain_icon(domain, theme)))
            name_label = util.label(action.get("friendlyName") or entity_id,
                                    css=["mp-entity-name"])
            name_label.set_hexpand(True)
            name_label.set_ellipsize(Pango.EllipsizeMode.END)
            name_label.set_tooltip_text(entity_id)
            head.append(name_label)
            box.append(head)
            self.info.set_tooltip_text(f"{util.ACTION_HINTS['home_assistant']}\n"
                                       f"{entity_id}")
        else:
            box.append(util.label("No entity selected", css=["mp-hint"]))

        buttons = util.horizontal(spacing=8)
        buttons.append(util.button("Entity", "view-list-symbolic",
                                   on_clicked=self._pick_entity))
        buttons.append(util.button("Connection…", "network-server-symbolic",
                                   on_clicked=self._open_ha_connection))
        box.append(buttons)

        selected = next((e for e in state.ha_entities
                         if e.get("entity_id") == entity_id), None)
        services = list((selected or {}).get("services") or ["toggle", "turn_on", "turn_off"])
        if action.get("service") not in services and action.get("entityId"):
            action["service"] = services[0]

        row = util.horizontal(spacing=8)
        row.append(util.label("Service:", css=["mp-hint"]))
        service_combo = Gtk.DropDown.new_from_strings(services)
        if action.get("service") in services:
            service_combo.set_selected(services.index(action["service"]))
        service_combo.connect(
            "notify::selected",
            lambda w, *_: self._set_field(action, "service", services[w.get_selected()]),
        )
        row.append(service_combo)

        can_test = bool(entity_id) and bool(action.get("service"))
        test_btn = self._test_button()
        test_btn.set_sensitive(can_test)
        row.append(test_btn)
        box.append(row)
        return box

    def _pick_entity(self, _btn) -> None:
        from ui.ha_dialogs import EntityPickerDialog

        def on_pick(entity: Dict[str, Any]) -> None:
            action = self.action
            action["entityId"] = entity.get("entity_id", "")
            action["friendlyName"] = entity.get("friendly_name", "")
            action["domain"] = entity.get("domain", "")
            services = entity.get("services") or ["toggle", "turn_on", "turn_off"]
            action["service"] = services[0]
            state.mark_dirty()
            state.emit("button-changed", index=state.selected_index)
            self.editor.refresh()

        EntityPickerDialog(self.editor.window, on_pick).present()

    def _open_ha_connection(self, _btn) -> None:
        from ui.ha_dialogs import HaConnectionDialog
        HaConnectionDialog(self.editor.window).present()

    # ------------------------------------------------------------------
    @staticmethod
    def _set_field(action: Dict[str, Any], field: str, value: Any) -> None:
        if action.get(field) == value:
            return
        action[field] = value
        state.mark_dirty()
        state.emit("button-changed", index=state.selected_index)