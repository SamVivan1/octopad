#!/usr/bin/env python3
"""OctoPad - Profiles page (import/export/preset) and Settings page."""

from typing import Any, Dict, Optional

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, GLib  # noqa: E402

import config as config_module
import state
from ui import util


def _json_filter() -> Gtk.FileFilter:
    file_filter = Gtk.FileFilter()
    file_filter.set_name("JSON Profile (*.json)")
    file_filter.add_pattern("*.json")
    return file_filter


def _finish_open(dialog, result, on_text) -> None:
    try:
        file = dialog.open_finish(result)
    except GLib.Error:
        return
    if file is None:
        return
    path = file.get_path()
    if not path:
        return
    try:
        with open(path, "r", encoding="utf-8") as handle:
            on_text(handle.read(), path)
    except Exception as exc:  # noqa: BLE001
        on_text(None, str(exc))


class ProfilesPage(Gtk.Box):
    def __init__(self, window) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.window = window

        page = Adw.PreferencesPage()
        page.set_title("Profiles")
        page.set_icon_name("document-open-symbolic")

        # --- Current profile -------------------------------------------
        current = Adw.PreferencesGroup(title="Current Profile")
        self.path_row = Adw.ActionRow(
            title="Configuration file",
            subtitle=config_module.CONFIG_FILE,
        )
        current.add(self.path_row)

        self.state_row = Adw.ActionRow(title="Changes", subtitle="Saved")
        self.preset_button = util.button("Save as Preset…", "document-save-as-symbolic",
                                         on_clicked=self._on_save_preset)
        self.preset_button.set_valign(Gtk.Align.CENTER)
        self.preset_button.set_tooltip_text(
            "Snapshot the current configuration as a preset")
        self.state_row.add_suffix(self.preset_button)
        self.save_button = util.button("Save", "document-save-symbolic",
                                       css=["suggested-action"],
                                       on_clicked=self._on_save)
        self.save_button.set_valign(Gtk.Align.CENTER)
        self.state_row.add_suffix(self.save_button)
        current.add(self.state_row)

        io_row = Adw.ActionRow(
            title="Import / Export",
            subtitle="Save the profile as a JSON file, or load an existing one.",
        )
        import_button = util.button("Import…", "document-open-symbolic",
                                    on_clicked=self._on_import)
        export_button = util.button("Export…", "document-save-as-symbolic",
                                    on_clicked=self._on_export)
        for widget in (import_button, export_button):
            widget.set_valign(Gtk.Align.CENTER)
            io_row.add_suffix(widget)
        current.add(io_row)
        page.add(current)

        # --- Built-in presets ----------------------------------------------
        presets_group = Adw.PreferencesGroup(
            title="Built-in Presets",
            description="Replaces the entire key mapping — save once it fits.",
        )
        for key, title, subtitle in config_module.PRESET_ITEMS:
            row = Adw.ActionRow(title=title, subtitle=subtitle, activatable=True)
            apply_button = util.button(
                "Apply", "emblem-default-symbolic", None,
                self._on_apply_preset, key)
            apply_button.set_valign(Gtk.Align.CENTER)
            row.add_suffix(apply_button)
            row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
            row.connect("activated", self._on_apply_preset, key)
            presets_group.add(row)
        page.add(presets_group)

        # --- My presets ----------------------------------------------------
        self.user_presets_group = Adw.PreferencesGroup(
            title="My Presets",
            description="Snapshots created with “Save as Preset”. Load one into "
                        "the editor, or overwrite it with the button above.",
        )
        self._user_preset_rows: list = []
        page.add(self.user_presets_group)

        info = Adw.PreferencesGroup(title="About")
        info.add(Adw.ActionRow(
            title="OctoPad v2.6",
            subtitle="Native GTK4 application for Ubuntu / GNOME — 8 keys, "
                     "Home Assistant REST, no scripts.",
        ))
        page.add(info)

        scroll = Gtk.ScrolledWindow(vexpand=True)
        scroll.set_child(page)
        self.append(scroll)

        state.subscribe(self._on_state_event)
        self._sync_state()

    # ------------------------------------------------------------------
    def _sync_state(self) -> None:
        if state.dirty:
            self.state_row.set_subtitle("Unsaved (use Save in the header)")
            self.save_button.set_sensitive(True)
        else:
            self.state_row.set_subtitle("Saved to disk")
            self.save_button.set_sensitive(False)
        self._rebuild_user_presets()

    def _rebuild_user_presets(self) -> None:
        for row in self._user_preset_rows:
            self.user_presets_group.remove(row)
        self._user_preset_rows = []

        names = config_module.list_presets()
        if not names:
            row = Adw.ActionRow(title="No presets yet",
                                subtitle="Use “Save as Preset” to create your "
                                         "first snapshot.")
            self._user_preset_rows.append(row)
            self.user_presets_group.add(row)
            return
        for name in names:
            active = name == state.active_preset
            row = Adw.ActionRow(
                title=name,
                subtitle="Active preset — use “Save as Preset” to update it"
                if active else "Click to load into the editor",
                activatable=not active)
            if active:
                row.add_prefix(Gtk.Image.new_from_icon_name("emblem-ok-symbolic"))
            activate = util.button("Load", "document-open-symbolic", None,
                                    self._on_load_user_preset, name)
            activate.set_valign(Gtk.Align.CENTER)
            row.add_suffix(activate)
            delete = util.button("Delete", "user-trash-symbolic", None,
                                 self._on_delete_user_preset, name)
            delete.set_valign(Gtk.Align.CENTER)
            row.add_suffix(delete)
            self._user_preset_rows.append(row)
            self.user_presets_group.add(row)

    def _on_load_user_preset(self, _widget, name: str) -> None:
        if state.apply_user_preset(name):
            self.window.show_toast(
                f"Preset “{name}” loaded — choose Save to activate it.",
                "success")
        else:
            self.window.show_toast(f"Failed to load preset “{name}”.", "error")

    def _on_delete_user_preset(self, widget, name: str) -> None:
        # Guard against double-clicks / repeated presses on a stale row.
        if widget is not None:
            widget.set_sensitive(False)
        if state.delete_user_preset(name):
            self.window.show_toast(f"Preset “{name}” deleted.", "success")
        else:
            if widget is not None:
                widget.set_sensitive(True)
            self.window.show_toast(f"Preset “{name}” was already removed.", "info")

    def _on_state_event(self, event: str, **_data: Any) -> None:
        if event in ("dirty-changed", "config-saved", "config-replaced",
                     "presets-changed"):
            self._sync_state()
            if event == "config-replaced":
                self.window.show_toast("Profile loaded.", "success")

    def _on_save(self, _btn) -> None:
        if not state.save():
            self.window.show_toast("Failed to save configuration.", "error")

    def _on_save_preset(self, _btn) -> None:
        self.window.show_save_preset_dialog()

    def _on_import(self, _btn) -> None:
        def on_text(text: Optional[str], info: str) -> None:
            if text is None:
                self.window.show_toast(f"Failed to read file: {info}", "error")
                return
            import json
            try:
                data: Dict[str, Any] = json.loads(text)
            except Exception as exc:  # noqa: BLE001
                self.window.show_toast(f"Invalid JSON: {exc}", "error")
                return
            ok, message = config_module.normalize_config(data)
            if not ok:
                self.window.show_toast(f"Invalid profile format: {message}", "error")
                return
            state.set_config(data, source="import")
            self.window.show_toast(f"Profile from {info} loaded — remember to Save.", "success")

        dialog = Gtk.FileDialog(title="Import OctoPad Profile")
        dialog.set_default_filter(_json_filter())
        dialog.open(self.window, None, lambda d, res: _finish_open(d, res, on_text))

    def _on_export(self, _btn) -> None:
        dialog = Gtk.FileDialog(title="Export OctoPad Profile")
        dialog.set_default_filter(_json_filter())
        dialog.set_initial_name(
            f"octopad-profile-{GLib.DateTime.new_now_local().format('%Y-%m-%d')}.json")

        def on_save(dlg, result) -> None:
            try:
                file = dlg.save_finish(result)
            except GLib.Error:
                return
            if file is None or not file.get_path():
                return
            try:
                import json
                with open(file.get_path(), "w", encoding="utf-8") as handle:
                    json.dump(state.cfg, handle, indent=2, ensure_ascii=False)
                self.window.show_toast("Profile exported successfully.", "success")
            except Exception as exc:  # noqa: BLE001
                self.window.show_toast(f"Failed to write file: {exc}", "error")

        dialog.save(self.window, None, on_save)

    def _on_apply_preset(self, _widget, key: str) -> None:
        state.set_config(config_module.apply_preset(key), source="preset")
        state.save()
        title = next((t for k, t, _ in config_module.PRESET_ITEMS if k == key), key)
        self.window.show_toast(f"Preset “{title}” applied.", "success")


# ---------------------------------------------------------------------------
class SettingsPage(Gtk.Box):
    def __init__(self, window) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.window = window

        page = Adw.PreferencesPage()
        page.set_title("Settings")
        page.set_icon_name("preferences-system-symbolic")

        # --- Timing -----------------------------------------------------
        timing = Adw.PreferencesGroup(
            title="Timing Parameters",
            description="Firmware defaults: debounce 25 ms, click 250 ms, "
                        "hold 450 ms.",
        )
        for field, title, minimum, maximum in (
            ("debounceMs", "Debounce Filter (ms)", 0, 200),
            ("clickTimeoutMs", "Click Timeout (ms)", 50, 2000),
            ("holdTimeoutMs", "Hold Timeout (ms)", 100, 3000),
        ):
            row = Adw.SpinRow.new_with_range(minimum, maximum, 5)
            row.set_title(title)
            row.set_value(float(state.cfg.get(field, 0)))
            row.connect("notify::value", self._on_timing, field)
            timing.add(row)
        page.add(timing)

        # --- Home Assistant --------------------------------------------
        ha_group = Adw.PreferencesGroup(title="Home Assistant")
        self.ha_row = Adw.ActionRow(title="Connection", subtitle="Checking…")
        ha_button = util.button("Connection…", "network-server-symbolic",
                                on_clicked=self._on_ha_connect)
        ha_button.set_valign(Gtk.Align.CENTER)
        self.ha_row.add_suffix(ha_button)
        ha_group.add(self.ha_row)

        self.ha_entity_row = Adw.ActionRow(
            title="Available entities", subtitle=f"{len(state.ha_entities)} entities")
        reload_button = util.button("Reload", "view-refresh-symbolic",
                                    on_clicked=self._on_ha_reload)
        reload_button.set_valign(Gtk.Align.CENTER)
        self.ha_entity_row.add_suffix(reload_button)
        ha_group.add(self.ha_entity_row)
        page.add(ha_group)

        # --- Aplikasi ---------------------------------------------------
        apps_group = Adw.PreferencesGroup(title="Installed Applications")
        self.apps_row = Adw.ActionRow(
            title="Application scanner (.desktop)",
            subtitle=f"{len(state.installed_apps)} applications")
        rescan_button = util.button("Rescan", "view-refresh-symbolic",
                                    on_clicked=self._on_rescan_apps)
        rescan_button.set_valign(Gtk.Align.CENTER)
        self.apps_row.add_suffix(rescan_button)
        apps_group.add(self.apps_row)
        page.add(apps_group)

        # --- Device -----------------------------------------------------
        device_group = Adw.PreferencesGroup(
            title="Device",
            description="The name your pad advertises over Bluetooth/USB. It is "
                        "stored on the pad and can be anything — the app finds "
                        "it by its OctoPad signature, not by name.",
        )
        self.device_row = Adw.ActionRow(title="Device name", subtitle="Not connected")
        rename_button = util.button("Rename…", "document-edit-symbolic",
                                    on_clicked=self._on_rename_device)
        rename_button.set_valign(Gtk.Align.CENTER)
        self.device_row.add_suffix(rename_button)
        device_group.add(self.device_row)
        page.add(device_group)

        # --- Info -------------------------------------------------------
        info = Adw.PreferencesGroup(title="Information")
        info.add(Adw.ActionRow(title="Configuration", subtitle=config_module.CONFIG_FILE))
        info.add(Adw.ActionRow(
            title="HA credentials",
            subtitle="~/.config/octopad/ha_config.json",
        ))
        info.add(Adw.ActionRow(
            title="Runtime",
            subtitle="GTK4 + Libadwaita, direct serial listener (no HTTP server)",
        ))
        page.add(info)

        scroll = Gtk.ScrolledWindow(vexpand=True)
        scroll.set_child(page)
        self.append(scroll)

        state.subscribe(self._on_state_event)
        self._sync_ha()
        self._sync_device()

    # ------------------------------------------------------------------
    def _sync_device(self) -> None:
        if state.device_name:
            text = f"“{state.device_name}”"
            if state.firmware_version:
                text += f" — firmware v{state.firmware_version}"
            self.device_row.set_subtitle(text)
        elif state.serial_connected or state.ble_connected:
            self.device_row.set_subtitle("Connected — unknown firmware")
        else:
            self.device_row.set_subtitle("Not connected")

    def _on_rename_device(self, _btn) -> None:
        if not (state.serial_connected or state.ble_connected):
            self.window.show_toast(
                "Connect the pad (USB or Bluetooth) to rename it.", "error")
            return

        dialog = Adw.MessageDialog(
            transient_for=self.window,
            heading="Rename device",
            body="Choose a name for your pad (1–24 characters). It is stored on "
                 "the device; the Bluetooth link restarts after renaming.",
        )
        entry = Gtk.Entry()
        entry.set_text(state.device_name or "OctoPad")
        entry.set_max_length(24)
        entry.set_activates_default(True)
        dialog.set_extra_child(entry)
        dialog.add_response("cancel", "Cancel")
        dialog.add_response("rename", "Rename")
        dialog.set_response_appearance("rename", Adw.ResponseAppearance.SUGGESTED)
        dialog.set_default_response("rename")
        dialog.set_close_response("cancel")

        def on_response(_dialog, response: str) -> None:
            if response != "rename":
                return
            name = entry.get_text().strip()
            if not name or len(name) > 24:
                self.window.show_toast("Name must be 1–24 characters.", "error")
                return
            if state.rename_device(name):
                self.window.show_toast(f"Device renamed to “{name}”.", "success")
            else:
                self.window.show_toast("Failed to rename the device.", "error")

        dialog.connect("response", on_response)
        dialog.present()

    # ------------------------------------------------------------------
    def _on_timing(self, row, *_args, field: str) -> None:
        value = int(row.get_value())
        if state.cfg.get(field) != value:
            state.set_timing(field, value)

    def _sync_ha(self) -> None:
        if state.ha_connected:
            self.ha_row.set_subtitle(f"Connected — {state.ha_message}")
        elif state.ha_message:
            self.ha_row.set_subtitle(state.ha_message)
        else:
            self.ha_row.set_subtitle("Not configured")
        self.ha_entity_row.set_subtitle(f"{len(state.ha_entities)} entities "
                                        f"({state.ha_entity_source})")

    def _on_state_event(self, event: str, **_data: Any) -> None:
        if event in ("device-name-changed", "device-info-changed",
                     "serial-status-changed", "ble-status-changed"):
            self._sync_device()
        elif event in ("ha-status-changed", "ha-entities-changed"):
            self._sync_ha()
        elif event == "apps-changed":
            self.apps_row.set_subtitle(f"{len(state.installed_apps)} applications")

    def _on_ha_connect(self, _btn) -> None:
        from ui.ha_dialogs import HaConnectionDialog
        HaConnectionDialog(self.window).present()

    def _on_ha_reload(self, _btn) -> None:
        from home_assistant import ha_client
        self.window.show_toast("Scanning entities…", "info")

        def done(result) -> None:
            entities = result if isinstance(result, list) else []
            source = "live" if ha_client._last_scan_live else "cache"
            state.set_ha_entities(entities, source)
            self.window.show_toast(f"{len(entities)} entities loaded.", "success")

        util.run_async(lambda: ha_client.get_entities(force_refresh=True), done)

    def _on_rescan_apps(self, _btn) -> None:
        from app_scanner import get_installed_apps
        self.window.show_toast("Scanning .desktop applications…", "info")

        def done(result) -> None:
            apps = result if isinstance(result, list) else []
            state.set_apps(apps)
            self.window.show_toast(f"{len(apps)} applications found.", "success")

        util.run_async(lambda: get_installed_apps(force_refresh=True), done)
