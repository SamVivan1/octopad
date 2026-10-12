#!/usr/bin/env python3
"""UI helpers: background work, toasts, domain icons, dialogs."""

from typing import Any, Callable, List, Optional

# Adwaita symbolic icons per Home Assistant domain (fallback if the theme lacks one)
DOMAIN_ICONS = {
    "light": "display-brightness-symbolic",
    "switch": "media-playback-start-symbolic",
    "group": "view-grid-symbolic",
    "input_boolean": "media-playback-start-symbolic",
    "fan": "weather-windy-symbolic",
    "cover": "view-grid-symbolic",
    "climate": "weather-few-clouds-symbolic",
    "humidifier": "weather-showers-symbolic",
    "water_heater": "weather-showers-symbolic",
    "lock": "changes-prevent-symbolic",
    "vacuum": "edit-clear-all-symbolic",
    "media_player": "multimedia-player-symbolic",
    "remote": "input-gaming-symbolic",
    "scene": "preferences-desktop-weather-symbolic",
    "script": "text-x-generic-symbolic",
    "automation": "applications-engineering-symbolic",
    "button": "media-skip-forward-symbolic",
    "input_button": "media-skip-forward-symbolic",
    "timer": "alarm-symbolic",
    "number": "view-list-symbolic",
    "input_number": "view-list-symbolic",
    "select": "view-list-symbolic",
    "input_select": "view-list-symbolic",
    "text": "text-x-generic-symbolic",
    "siren": "alarm-symbolic",
    "alarm_control_panel": "changes-prevent-symbolic",
    "update": "software-update-available-symbolic",
    "tts": "audio-volume-high-symbolic",
    "default": "applications-other-symbolic",
}

ACTION_ICONS = {
    "none": "action-unavailable-symbolic",
    "shortcut": "input-keyboard-symbolic",
    "text": "text-x-generic-symbolic",
    "media": "audio-volume-high-symbolic",
    "audio_output": "audio-speakers-symbolic",
    "launch_app": "system-run-symbolic",
    "system_action": "applications-system-symbolic",
    "bash_script": "utilities-terminal-symbolic",
    "home_assistant": "go-home-symbolic",
    "ha_webhook": "mail-send-symbolic",
    "delay": "alarm-symbolic",
    "mode_toggle": "view-grid-symbolic",
}

# Compact names used in the icon-driven action-type selector.
ACTION_SHORT = {
    "none": "None",
    "shortcut": "Shortcut",
    "text": "Text",
    "media": "Media",
    "audio_output": "Audio Output",
    "launch_app": "App",
    "system_action": "System",
    "bash_script": "Script",
    "home_assistant": "Home Assistant",
    "ha_webhook": "Webhook",
    "delay": "Delay",
    "mode_toggle": "Mode",
}

# One-line explanations, surfaced through the card's ⓘ tooltip instead of
# printed inline (keeps the editor compact when many actions are stacked).
ACTION_HINTS = {
    "none": "Empty slot. Pick a type from the menu, or use the trash button "
            "to remove it.",
    "shortcut": "Sends a keyboard shortcut to the active window.",
    "text": "Types this text as keystrokes into the active window.",
    "media": "Controls volume or media playback (wpctl/pactl/playerctl).",
    "audio_output": "Switches the default audio output to the next selected "
                    "device each time (cycles through your chosen outputs).",
    "launch_app": "Launches an installed desktop application.",
    "system_action": "Runs a built-in system action preset.",
    "bash_script": "Runs the script with /bin/bash in the background.",
    "home_assistant": "Calls a Home Assistant service on the chosen entity.",
    "ha_webhook": "POSTs an empty JSON body to a Home Assistant webhook URL.",
    "delay": "Pauses the action sequence before the next action.",
    "mode_toggle": "Switches the layer between Desktop and Home Assistant mode "
                   "(handled by the macropad firmware / status LED).",
}

MEDIA_ICONS = {
    "MUTE": "audio-volume-muted-symbolic",
    "VOL_UP": "audio-volume-high-symbolic",
    "VOL_DOWN": "audio-volume-low-symbolic",
    "PLAY_PAUSE": "media-playback-start-symbolic",
    "NEXT_TRACK": "media-skip-forward-symbolic",
    "PREV_TRACK": "media-skip-backward-symbolic",
}


def system_preset_icon(preset) -> str:
    """Best-effort symbolic icon for a system-action preset."""
    pid = str(preset.get("id", "")).lower()
    if "volume" in pid or "mute" in pid:
        return "audio-volume-high-symbolic"
    if "lock" in pid:
        return "changes-prevent-symbolic"
    if "terminal" in pid:
        return "utilities-terminal-symbolic"
    if "screenshot" in pid:
        return "applets-screenshooter-symbolic"
    if "media" in pid or "play" in pid:
        return "media-playback-start-symbolic"
    return "applications-system-symbolic"


def domain_icon(domain: str, theme=None) -> str:
    name = DOMAIN_ICONS.get(domain, DOMAIN_ICONS["default"])
    if theme is not None and not theme.has_icon(name):
        return DOMAIN_ICONS["default"]
    return name


def action_icon(action_type: str, theme=None) -> str:
    name = ACTION_ICONS.get(action_type, DOMAIN_ICONS["default"])
    if theme is not None and not theme.has_icon(name):
        return DOMAIN_ICONS["default"]
    return name


def run_async(work: Callable[[], Any],
              on_done: Optional[Callable[[Any], None]] = None) -> None:
    """Run `work` on a separate thread; the result is posted back to the main loop."""
    import threading
    from gi.repository import GLib

    def target() -> None:
        try:
            result = work()
        except Exception as exc:  # noqa: BLE001 - all errors are forwarded to the UI
            result = (False, str(exc))
        GLib.idle_add(_finish, result)

    def _finish(result: Any) -> bool:
        if on_done is not None:
            try:
                on_done(result)
            except Exception as exc:  # noqa: BLE001
                print(f"[UI] callback failed: {exc}")
        return False

    threading.Thread(target=target, daemon=True).start()


def toast_result(window, result: Any, prefix: str = "") -> None:
    """Show a toast from an (ok, message) tuple result."""
    if isinstance(result, tuple) and len(result) == 2:
        ok, message = result
    else:
        ok, message = bool(result), str(result)
    kind = "success" if ok else "error"
    window.show_toast(f"{prefix}{message}", kind)


def vertical(spacing: int = 8, **kwargs):
    from gi.repository import Gtk
    return Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=spacing, **kwargs)


def horizontal(spacing: int = 8, **kwargs):
    from gi.repository import Gtk
    return Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=spacing, **kwargs)


def label(text: str, css: Optional[List[str]] = None, xalign: float = 0.0, wrap: bool = False):
    from gi.repository import Gtk
    widget = Gtk.Label(label=text, xalign=xalign, wrap=wrap, selectable=False)
    if css:
        widget.set_css_classes(css)
    return widget


def hint(text: str):
    from gi.repository import Gtk
    widget = Gtk.Label(label=text, xalign=0.0, wrap=True)
    widget.set_css_classes(["mp-hint"])
    widget.set_selectable(False)
    return widget


def button(text: str, icon_name: Optional[str] = None, css: Optional[List[str]] = None,
           on_clicked: Optional[Callable] = None, *args):
    from gi.repository import Gtk
    if icon_name:
        widget = Gtk.Button()
        content = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        content.append(Gtk.Image.new_from_icon_name(icon_name))
        content.append(Gtk.Label(label=text))
        widget.set_child(content)
    else:
        widget = Gtk.Button(label=text)
    if css:
        widget.set_css_classes(css)
    if on_clicked is not None:
        widget.connect("clicked", on_clicked, *args)
    return widget


def icon_button(icon_name: str, tooltip: str,
                on_clicked: Optional[Callable] = None,
                css: Optional[List[str]] = None, *args):
    """Compact icon-only button with a tooltip (keeps the UI text-light)."""
    from gi.repository import Gtk
    widget = Gtk.Button(icon_name=icon_name)
    widget.set_tooltip_text(tooltip)
    if css:
        widget.set_css_classes(css)
    if on_clicked is not None:
        widget.connect("clicked", on_clicked, *args)
    return widget


def info_icon(text: str):
    """Small ⓘ glyph whose tooltip carries an explanation (no inline text)."""
    from gi.repository import Gtk
    widget = Gtk.Image.new_from_icon_name("help-about-symbolic")
    widget.set_css_classes(["mp-info"])
    widget.set_tooltip_text(text)
    widget.set_valign(Gtk.Align.CENTER)
    return widget


def icon_dropdown(items, css: Optional[List[str]] = None):
    """A DropDown whose rows show an icon + label.

    `items` is a list of (key, label, icon_name) tuples, in display order.
    Use `set_selected(index)` / `get_selected()` / `notify::selected` as usual.
    """
    from gi.repository import Gtk

    dropdown = Gtk.DropDown.new_from_strings([label for _key, label, _icon in items])
    factory = Gtk.SignalListItemFactory()

    def setup(_factory, list_item):
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        row.append(Gtk.Image())
        row.append(Gtk.Label(xalign=0.0))
        list_item.set_child(row)

    def bind(_factory, list_item):
        position = list_item.get_position()
        if not 0 <= position < len(items):
            return
        _key, label, icon_name = items[position]
        row = list_item.get_child()
        image = row.get_first_child()
        text = image.get_next_sibling()
        image.set_from_icon_name(icon_name)
        text.set_text(label)

    factory.connect("setup", setup)
    factory.connect("bind", bind)
    dropdown.set_factory(factory)
    if css:
        dropdown.set_css_classes(css)
    return dropdown
