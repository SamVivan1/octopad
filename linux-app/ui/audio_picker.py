#!/usr/bin/env python3
"""OctoPad - Audio output picker dialog.

Lists every available output sink (scanned via pactl) and lets the user tick
the ones the action should cycle through.
"""

from typing import Any, Callable, Dict, List

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Pango  # noqa: E402

from ui import util


class AudioOutputPickerDialog(Adw.Window):
    def __init__(self, window, current: List[Dict[str, Any]],
                 on_pick: Callable[[List[Dict[str, str]]], None]) -> None:
        super().__init__(transient_for=window, modal=True,
                         title="Choose Audio Outputs",
                         default_width=560, default_height=560)
        self.on_pick = on_pick
        self.selected_names = {
            s.get("name") for s in current if isinstance(s, dict) and s.get("name")
        }
        self.rows: List[Any] = []

        toolbar = Adw.ToolbarView()
        header = Adw.HeaderBar()
        header.set_title_widget(Adw.WindowTitle.new(
            "Choose Audio Outputs",
            "Tick the devices to include in the cycle"))
        apply_button = Gtk.Button(label="Apply")
        apply_button.add_css_class("suggested-action")
        apply_button.connect("clicked", self._on_apply)
        header.pack_end(apply_button)
        toolbar.add_top_bar(header)

        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        for edge in ("top", "bottom"):
            getattr(content, f"set_margin_{edge}")(8)
        for edge in ("start", "end"):
            getattr(content, f"set_margin_{edge}")(12)

        self.status = util.hint("Scanning audio outputs…")
        content.append(self.status)

        self.listbox = Gtk.ListBox()
        self.listbox.set_selection_mode(Gtk.SelectionMode.NONE)
        self.listbox.set_css_classes(["boxed-list"])
        scroll = Gtk.ScrolledWindow(vexpand=True)
        scroll.set_child(self.listbox)
        content.append(scroll)

        toolbar.set_content(content)
        self.set_content(toolbar)

        util.run_async(list_audio_sinks, self._on_scanned)

    # ------------------------------------------------------------------
    def _on_scanned(self, result: Any) -> None:
        sinks = result if isinstance(result, list) else []
        self.status.set_visible(False)
        if not sinks:
            self.status.set_text(
                "No audio outputs found — `pactl` (pulseaudio-utils) is required.")
            self.status.set_visible(True)
            return
        for sink in sinks:
            self._add_row(sink)

    def _add_row(self, sink: Dict[str, str]) -> None:
        row = Gtk.ListBoxRow()
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        box.set_margin_top(8)
        box.set_margin_bottom(8)
        box.set_margin_start(8)
        box.set_margin_end(8)

        switch = Gtk.Switch()
        switch.set_valign(Gtk.Align.CENTER)
        switch.set_active(sink.get("name") in self.selected_names)
        box.append(switch)

        box.append(Gtk.Image.new_from_icon_name("audio-speakers-symbolic"))

        text = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        text.set_hexpand(True)
        description = sink.get("description") or sink.get("name", "")
        label = util.label(description, css=["mp-entity-name"])
        label.set_ellipsize(Pango.EllipsizeMode.END)
        text.append(label)
        name = util.label(sink.get("name", ""), css=["mp-hint"])
        name.set_ellipsize(Pango.EllipsizeMode.MIDDLE)
        text.append(name)
        box.append(text)

        row.set_child(box)
        click = Gtk.GestureClick()
        click.connect("released", lambda *_: switch.set_active(not switch.get_active()))
        text.add_controller(click)
        self.rows.append((switch, sink))
        self.listbox.append(row)

    def _on_apply(self, _button) -> None:
        picked = [sink for switch, sink in self.rows if switch.get_active()]
        try:
            self.on_pick(picked)
        finally:
            self.close()


def list_audio_sinks() -> List[Dict[str, str]]:
    """Delegate to the system helper (kept here for a local import)."""
    from system_actions import list_audio_sinks as _list
    return _list()
