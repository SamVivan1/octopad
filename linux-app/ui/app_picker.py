#!/usr/bin/env python3
"""OctoPad - Installed application picker dialog (.desktop)."""

from typing import Callable, Dict, List

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw  # noqa: E402

import state
from ui import util


class AppPickerDialog(Adw.Window):
    def __init__(self, window, on_pick: Callable[[Dict[str, str]], None]) -> None:
        super().__init__(transient_for=window, modal=True,
                         title="Choose Application", default_width=600, default_height=560)
        self.on_pick = on_pick
        self.apps: List[Dict[str, str]] = list(state.installed_apps)
        self._filter = ""

        toolbar = Adw.ToolbarView()
        header = Adw.HeaderBar()
        header.set_title_widget(Adw.WindowTitle.new(
            "Choose Application", f"{len(self.apps)} applications installed"))
        toolbar.add_top_bar(header)

        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        content.set_margin_top(8)
        content.set_margin_bottom(8)
        content.set_margin_start(12)
        content.set_margin_end(12)

        self.search = Gtk.SearchEntry()
        self.search.set_placeholder_text("Search application name…")
        self.search.connect("search-changed", self._on_search)
        content.append(self.search)

        self.listbox = Gtk.ListBox()
        self.listbox.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.listbox.set_activate_on_single_click(True)
        self.listbox.connect("row-activated", self._on_row_activated)
        self.listbox.set_filter_func(self._filter_row)

        scroll = Gtk.ScrolledWindow(vexpand=True)
        scroll.set_child(self.listbox)
        content.append(scroll)

        if not self.apps:
            content.append(util.hint(
                "No applications yet — run a rescan on the Settings page."))
        else:
            self._populate()

        toolbar.set_content(content)
        self.set_content(toolbar)
        self.search.grab_focus()

    # ------------------------------------------------------------------
    def _populate(self) -> None:
        theme = Gtk.IconTheme.get_for_display(self.get_display())
        for app in self.apps:
            row = Gtk.ListBoxRow()
            row.app_data = app  # type: ignore[attr-defined]

            box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
            box.set_margin_top(6)
            box.set_margin_bottom(6)
            box.set_margin_start(6)
            box.set_margin_end(6)

            icon_name = app.get("icon", "applications-other-symbolic")
            if not theme.has_icon(icon_name):
                icon_name = "applications-other-symbolic"
            image = Gtk.Image.new_from_icon_name(icon_name)
            image.set_pixel_size(32)
            box.append(image)

            text_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
            name = util.label(app.get("name", ""), css=["mp-entity-name"])
            text_box.append(name)
            comment = app.get("comment") or app.get("id", "")
            if comment:
                text_box.append(util.hint(comment))
            box.append(text_box)

            row.set_child(box)
            self.listbox.append(row)

    def _on_search(self, entry: Gtk.SearchEntry) -> None:
        self._filter = (entry.get_text() or "").strip().lower()
        self.listbox.invalidate_filter()

    def _filter_row(self, row: Gtk.ListBoxRow) -> bool:
        if not self._filter:
            return True
        app = getattr(row, "app_data", {}) or {}
        haystack = " ".join([
            app.get("name", ""), app.get("id", ""), app.get("comment", ""),
            app.get("exec", ""),
        ]).lower()
        return self._filter in haystack

    def _on_row_activated(self, _listbox, row: Gtk.ListBoxRow) -> None:
        app = getattr(row, "app_data", None)
        if not app:
            return
        try:
            self.on_pick(app)
        finally:
            self.close()
