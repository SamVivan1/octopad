#!/usr/bin/env python3
"""OctoPad - Main window (GTK4 + Libadwaita, GNOME HIG)."""

from typing import Any, Optional

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, Gio, GLib  # noqa: E402

import config as config_module
import state
from ui import util

_PAGES = (
    ("keys", "Buttons", "input-keyboard-symbolic"),
    ("console", "Serial Console", "utilities-terminal-symbolic"),
    ("profiles", "Profiles", "document-open-symbolic"),
    ("settings", "Settings", "preferences-system-symbolic"),
)


class OctoPadWindow(Adw.ApplicationWindow):
    def __init__(self, app) -> None:
        super().__init__(application=app, title="OctoPad")
        self.set_default_size(1180, 780)
        self._sidebar_syncing = False

        # --- pages -----------------------------------------------------
        # Only the default page is built up front; the other pages are created
        # on first visit to keep idle memory low.
        self._pages: dict = {}
        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE,
                               transition_duration=120)
        self._get_page("keys")
        self.stack.set_visible_child_name("keys")
        self.stack.connect("notify::visible-child", self._on_stack_changed)
        self.connect("close-request", self._on_close_request)

        # --- sidebar ---------------------------------------------------
        self._sidebar_expanded = True
        self._expanded_sidebar_width: Optional[int] = None
        self._sidebar_rows = []
        self.sidebar_list = Gtk.ListBox()
        self.sidebar_list.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.sidebar_list.add_css_class("navigation-sidebar")
        self.sidebar_list.connect("row-selected", self._on_sidebar_selected)
        for name, title, icon in _PAGES:
            row = Gtk.ListBoxRow()
            box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            image = Gtk.Image.new_from_icon_name(icon)
            label = Gtk.Label(label=title)
            label.set_xalign(0.0)
            box.append(image)
            box.append(label)
            row.set_child(box)
            row.set_activatable(True)
            row._page_name = name  # noqa: SLF001 - simple internal marker
            self.sidebar_list.append(row)
            self._sidebar_rows.append({
                "name": name, "title": title,
                "box": box, "image": image, "label": label, "row": row,
            })

        sidebar_page = Adw.NavigationPage.new(self.sidebar_list, "Navigation")
        content_page = Adw.NavigationPage.new(self.stack, "OctoPad")

        split = Adw.NavigationSplitView()
        split.set_sidebar(sidebar_page)
        split.set_content(content_page)
        split.set_collapsed(False)          # sidebar always visible (icons only when collapsed)
        split.set_min_sidebar_width(160)
        split.set_max_sidebar_width(220)
        self.split = split
        split.connect("notify::collapsed", self._on_collapsed_changed)

        self.connect("map", lambda *_: GLib.idle_add(self._apply_sidebar_mode))

        # --- header bar -------------------------------------------------
        self.title_widget = Adw.WindowTitle.new(
            "OctoPad", self._mode_subtitle())

        self.menu_button = Gtk.MenuButton()
        self.menu_button.set_icon_name("open-menu-symbolic")
        self.menu_button.set_menu_model(self._build_menu())
        self.menu_button.set_tooltip_text("Menu")

        # Sidebar button: expand / collapse (icons only vs icon + label)
        self.sidebar_toggle = Gtk.ToggleButton()
        self.sidebar_toggle.set_icon_name("pan-start-symbolic")
        self.sidebar_toggle.set_tooltip_text("Collapse sidebar (icons only)")
        self.sidebar_toggle.set_active(True)
        self.sidebar_toggle.connect("toggled", self._on_sidebar_toggled)

        self.conn_pill, self.conn_pill_dot, self.conn_pill_lbl, self.conn_pill_icon = \
            self._make_conn_pill()
        self.conn_pill.set_css_classes(["mp-pill", "mp-err"])
        self.conn_pill.connect("clicked", self._on_conn_pill_clicked)

        self.ha_pill, self.ha_pill_dot, self.ha_pill_lbl = self._make_pill()
        self.ha_pill.set_css_classes(["mp-pill", "mp-warn"])
        self.ha_pill.connect("clicked", self._on_ha_pill_clicked)

        self.save_button = util.button("Save", "document-save-symbolic",
                                       ["suggested-action"], self._on_save_clicked)
        self.save_button.set_tooltip_text("Save the current configuration (Ctrl+S)")

        header = Adw.HeaderBar()
        header.set_title_widget(self.title_widget)
        header.pack_start(self.sidebar_toggle)
        header.pack_start(self.menu_button)
        header.pack_end(self.save_button)
        header.pack_end(self.ha_pill)
        header.pack_end(self.conn_pill)

        toolbar = Adw.ToolbarView()
        toolbar.add_top_bar(header)
        toolbar.set_content(split)

        self.toast_overlay = Adw.ToastOverlay()
        self.toast_overlay.set_child(toolbar)
        self.set_content(self.toast_overlay)

        # --- actions ----------------------------------------------------
        self._install_actions()

        # --- event bus --------------------------------------------------
        state.subscribe(self._on_state_event)
        self._sync_pills()
        self._sync_save_button()
        GLib.timeout_add_seconds(3, self._poll_serial)

    # ------------------------------------------------------------------
    # Menu & actions
    # ------------------------------------------------------------------
    @staticmethod
    def _build_menu() -> Gio.Menu:
        menu = Gio.Menu()

        menu.append("Save as Preset…", "win.save-preset")

        profile = Gio.Menu()
        profile.append("Import Profile…", "win.import-profile")
        profile.append("Export Profile…", "win.export-profile")
        menu.append_submenu("Profiles", profile)

        presets = Gio.Menu()
        presets.append("Default (Desktop and Native HA)", "win.preset-default")
        presets.append("Productivity", "win.preset-productivity")
        presets.append("Media", "win.preset-media")
        menu.append_submenu("Built-in Presets", presets)

        menu.append("About OctoPad", "win.about")
        return menu

    def _install_actions(self) -> None:
        def add(name: str, handler) -> None:
            action = Gio.SimpleAction.new(name, None)
            action.connect("activate", handler)
            self.add_action(action)

        add("save", lambda *_: self._on_save_clicked(None))
        add("save-preset", lambda *_: self._on_save_preset_clicked(None))
        add("import-profile", lambda *_: self._get_page("profiles")._on_import(None))
        add("export-profile", lambda *_: self._get_page("profiles")._on_export(None))
        add("preset-default",
            lambda *_: self._get_page("profiles")._on_apply_preset(None, "default"))
        add("preset-productivity",
            lambda *_: self._get_page("profiles")._on_apply_preset(None, "productivity"))
        add("preset-media",
            lambda *_: self._get_page("profiles")._on_apply_preset(None, "media"))
        add("about", lambda *_: self._show_about())

    def _show_about(self) -> None:
        Adw.AboutWindow(
            transient_for=self,
            application_name="OctoPad",
            application_icon="octopad",
            version="2.6",
            developer_name="OctoPad Project",
            developers=["SamVivan", "OctoPad contributors"],
            license_type=Gtk.License.MIT_X11,
            comments="Native GTK4 application for an 8-key serial macropad — "
                     "Home Assistant REST integration and local action "
                     "mapping, with no web server required.",
        ).present()

    # ------------------------------------------------------------------
    # Toast
    # ------------------------------------------------------------------
    def show_toast(self, message: str, kind: str = "info") -> None:
        toast = Adw.Toast(title=message)
        toast.set_timeout(6 if kind == "error" else 4)
        self.toast_overlay.add_toast(toast)

    @staticmethod
    def _make_pill():
        """Header status pill: colored dot icon + label."""
        button = Gtk.Button()
        button.set_has_frame(False)
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        dot = Gtk.Label(label="●")
        dot.set_css_classes(["mp-pill-dot"])
        label = Gtk.Label(label="…")
        box.append(dot)
        box.append(label)
        button.set_child(box)
        return button, dot, label

    @staticmethod
    def _make_conn_pill():
        """Single connection pill: dot + label + transport icon (USB/BT)."""
        button = Gtk.Button()
        button.set_has_frame(False)
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        dot = Gtk.Label(label="●")
        dot.set_css_classes(["mp-pill-dot"])
        label = Gtk.Label(label="Disconnected")
        icon = Gtk.Image()
        icon.set_visible(False)
        box.append(dot)
        box.append(label)
        box.append(icon)
        button.set_child(box)
        return button, dot, label, icon

    # ------------------------------------------------------------------
    # Navigation & sidebar
    # ------------------------------------------------------------------
    @staticmethod
    def _mode_subtitle() -> str:
        return "Home Assistant Mode" if state.mode_index == 1 else "Desktop Mode"

    def _get_page(self, name: str):
        """Return a page widget, creating it (and its module) on first use."""
        page = self._pages.get(name)
        if page is None:
            if name == "keys":
                from ui.keys_page import KeysPage
                page = KeysPage(self)
            elif name == "console":
                from ui.console_page import ConsolePage
                page = ConsolePage(self)
            elif name == "profiles":
                from ui.settings_page import ProfilesPage
                page = ProfilesPage(self)
            elif name == "settings":
                from ui.settings_page import SettingsPage
                page = SettingsPage(self)
            else:
                raise ValueError(f"Unknown page: {name}")
            self._pages[name] = page
            self.stack.add_named(page, name)
        return page

    def _show_page(self, name: str):
        page = self._get_page(name)
        self.stack.set_visible_child_name(name)
        return page

    def _apply_sidebar_mode(self) -> None:
        """Apply the sidebar mode: expanded (icon + label, text-sized)
        or collapsed (icons only, narrow sidebar)."""
        expanded = self._sidebar_expanded

        for info in self._sidebar_rows:
            info["label"].set_visible(expanded)
            info["box"].set_halign(Gtk.Align.START if expanded else Gtk.Align.CENTER)
            info["row"].set_tooltip_text(None if expanded else info["title"])

        if expanded and self._expanded_sidebar_width is None:
            natural = self._measure_sidebar_width()
            self._expanded_sidebar_width = max(160, natural + 4)
        width = self._expanded_sidebar_width if expanded else 52

        self.split.set_min_sidebar_width(width)
        self.split.set_max_sidebar_width(width)
        self._sidebar_syncing = True
        self.sidebar_toggle.set_active(expanded)
        self._sidebar_syncing = False
        self.sidebar_toggle.set_icon_name(
            "pan-start-symbolic" if expanded else "pan-end-symbolic")
        self.sidebar_toggle.set_tooltip_text(
            "Collapse sidebar (icons only)" if expanded
            else "Expand sidebar (icon + text)")

    def _measure_sidebar_width(self) -> int:
        """Natural width of the navigation list (up to the end of the text)."""
        try:
            return self.sidebar_list.measure(Gtk.Orientation.HORIZONTAL, -1)[1]
        except Exception:  # noqa: BLE001 - size not ready yet
            return 180

    def _on_sidebar_selected(self, _listbox, row) -> None:
        if self._sidebar_syncing or row is None:
            return
        self._get_page(row._page_name)  # noqa: SLF001
        self.stack.set_visible_child_name(row._page_name)  # noqa: SLF001
        # if the content pane is hidden (narrow window mode), reveal it
        if self.split.get_collapsed():
            self.split.set_show_content(True)
            self._sidebar_syncing = True
            self.sidebar_toggle.set_active(True)
            self._sidebar_syncing = False

    def _on_collapsed_changed(self, split, *_args) -> None:
        collapsed = split.get_collapsed()
        self._sidebar_syncing = True
        if collapsed:
            self.sidebar_toggle.set_active(split.get_show_content())
        else:
            self.sidebar_toggle.set_active(self._sidebar_expanded)
        self._sidebar_syncing = False

    def _on_sidebar_toggled(self, button: Gtk.ToggleButton) -> None:
        if self._sidebar_syncing:
            return
        if self.split.get_collapsed():
            self.split.set_show_content(button.get_active())
        else:
            self._sidebar_expanded = button.get_active()
            self._apply_sidebar_mode()

    def _on_stack_changed(self, *_args) -> None:
        name = self.stack.get_visible_child_name()
        self._sidebar_syncing = True
        row = self.sidebar_list.get_row_at_index(
            [p[0] for p in _PAGES].index(name) if name else 0)
        if row is not None:
            self.sidebar_list.select_row(row)
        self._sidebar_syncing = False

    def _on_close_request(self, *_args) -> bool:
        """Closing the window hides to the tray when available; on quit, close."""
        app = self.get_application()
        quitting = getattr(app, "_quitting", False)
        if quitting or not getattr(app, "tray_ok", False):
            return False
        self.hide()
        return True

    def _on_conn_pill_clicked(self, _btn) -> None:
        self._show_page("console")

    def _on_ha_pill_clicked(self, _btn) -> None:
        from ui.ha_dialogs import HaConnectionDialog
        HaConnectionDialog(self).present()

    def _on_save_clicked(self, _btn) -> None:
        """Plain save: persist the working configuration to disk."""
        state.save()

    def _on_save_preset_clicked(self, _btn) -> None:
        self.show_save_preset_dialog()

    def show_save_preset_dialog(self) -> None:
        """Save the current configuration as a preset.

        By default an existing preset is overwritten (the active one is
        preselected). The user may pick any preset from the list or switch to
        "Create new preset".
        """
        presets = config_module.list_presets()
        dialog = Adw.MessageDialog(transient_for=self, heading="Save as Preset")
        dialog.set_body("Overwrite an existing preset, or create a new one.")

        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        body.set_margin_top(8)
        body.set_margin_bottom(4)
        body.set_margin_start(8)
        body.set_margin_end(8)

        overwrite_button = Gtk.ToggleButton()
        overwrite_button.set_label("Overwrite existing preset")
        new_button = Gtk.ToggleButton()
        new_button.set_label("Create new preset")
        new_button.set_group(overwrite_button)

        overwrite_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        overwrite_row.append(Gtk.Label(label="Preset:"))
        dropdown = Gtk.DropDown.new_from_strings(presets or ["(no presets yet)"])
        dropdown.set_hexpand(True)
        if state.active_preset in presets:
            dropdown.set_selected(presets.index(state.active_preset))
        overwrite_row.append(dropdown)

        name_entry = Gtk.Entry()
        name_entry.set_placeholder_text("New preset name")
        name_entry.set_text(self._suggest_preset_name())
        name_entry.connect("activate", lambda *_: dialog.emit("response", "save"))

        if presets:
            overwrite_button.set_active(True)
        else:
            new_button.set_active(True)
            overwrite_button.set_sensitive(False)

        def _sync_mode(*_args) -> None:
            overwriting = overwrite_button.get_active()
            overwrite_row.set_sensitive(overwriting)
            name_entry.set_sensitive(not overwriting)
            name_entry.set_visible(not overwriting)

        overwrite_button.connect("toggled", _sync_mode)

        body.append(overwrite_button)
        body.append(overwrite_row)
        body.append(new_button)
        body.append(name_entry)
        _sync_mode()

        dialog.set_extra_child(body)
        dialog.add_response("cancel", "Cancel")
        dialog.add_response("save", "Save")
        dialog.set_default_response("save")
        dialog.set_response_appearance("save", Adw.ResponseAppearance.SUGGESTED)
        dialog.connect("response", self._on_save_preset_response,
                       overwrite_button, dropdown, presets, name_entry)
        dialog.present()

    def _suggest_preset_name(self) -> str:
        count = len(config_module.list_presets()) + 1
        return f"Preset {count}"

    def _on_save_preset_response(
            self, _dialog, response: str, overwrite_button, dropdown,
            presets, name_entry) -> None:
        if response != "save":
            return
        if overwrite_button.get_active():
            if not presets:
                self.show_toast("No preset to overwrite — create a new one.", "error")
                return
            name = presets[dropdown.get_selected()]
            ok = state.save_preset(name, overwrite=True)
            action = "updated"
        else:
            name = name_entry.get_text().strip()
            if not name:
                self.show_toast("Preset name cannot be empty.", "error")
                return
            ok = state.save_preset(name, overwrite=False)
            action = "created"

        if not ok:
            self.show_toast("Failed to save preset.", "error")
            return
        self.show_toast(f"Preset “{state.active_preset or name}” {action}.", "success")

    # ------------------------------------------------------------------
    # Pill / button synchronization
    # ------------------------------------------------------------------
    def _sync_pills(self) -> None:
        if state.serial_connected or state.ble_connected:
            if state.serial_connected:
                icon = "network-wired-symbolic"
                tip = f"Connected via USB ({state.serial_port or '?'})"
                if state.ble_connected:
                    tip += " and Bluetooth"
            else:
                icon = "bluetooth-symbolic"
                tip = "Connected via Bluetooth"
            self.conn_pill_icon.set_from_icon_name(icon)
            self.conn_pill_icon.set_visible(True)
            self.conn_pill_lbl.set_text("Connected")
            self.conn_pill.set_css_classes(["mp-pill", "mp-ok"])
            self.conn_pill.set_tooltip_text(
                tip + " — click to open the console")
        elif "searching" in (state.ble_message or "").lower():
            self.conn_pill_icon.set_visible(False)
            self.conn_pill_lbl.set_text("Disconnected")
            self.conn_pill.set_css_classes(["mp-pill", "mp-warn"])
            self.conn_pill.set_tooltip_text(
                state.ble_message or "Searching for the macropad…")
        else:
            self.conn_pill_icon.set_visible(False)
            self.conn_pill_lbl.set_text("Disconnected")
            self.conn_pill.set_css_classes(["mp-pill", "mp-err"])
            self.conn_pill.set_tooltip_text(
                state.ble_message
                or "OctoPad not connected — connect via USB or Bluetooth")

        if state.ha_connected:
            self.ha_pill_lbl.set_text("HA: Connected")
            self.ha_pill.set_css_classes(["mp-pill", "mp-ok"])
        elif not state.ha_message:
            self.ha_pill_lbl.set_text("HA: checking…")
            self.ha_pill.set_css_classes(["mp-pill", "mp-warn"])
        else:
            self.ha_pill_lbl.set_text("HA: disconnected")
            self.ha_pill.set_css_classes(["mp-pill", "mp-err"])
        self.ha_pill.set_tooltip_text(state.ha_message or "Home Assistant status")

    def _sync_save_button(self) -> None:
        self.save_button.set_sensitive(state.dirty)
        if state.dirty:
            self.save_button.set_tooltip_text("Save changes (Ctrl+S)")
        else:
            self.save_button.set_tooltip_text("Configuration already saved")

    def _poll_serial(self) -> bool:
        listener = state.listener
        if listener is not None:
            open_port = getattr(listener, "ser", None)
            connected = bool(open_port is not None and open_port.is_open)
            port = listener.last_connected_port if connected else None
            state.set_serial_status(connected, port)
            state.set_device_name(getattr(listener, "device_name", None))
        return True

    # ------------------------------------------------------------------
    # Event bus
    # ------------------------------------------------------------------
    def _on_state_event(self, event: str, **data: Any) -> None:
        if event == "mode-changed":
            self.title_widget.set_subtitle(self._mode_subtitle())
        elif event in ("serial-status-changed", "ha-status-changed",
                       "ble-status-changed"):
            self._sync_pills()
        elif event in ("dirty-changed", "config-saved"):
            self._sync_save_button()
            if event == "config-saved" and data.get("ok") is False:
                self.show_toast("Failed to save configuration.", "error")
