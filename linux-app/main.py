#!/usr/bin/env python3
"""
OctoPad - Native Linux application (GTK4 + Libadwaita).

Every feature runs directly from Python: serial listener, action execution,
button mapping, and Home Assistant REST — with no HTTP server and no web UI.
"""

import re
import glob
import os
import sys


def _prefer_software_renderer() -> None:
    """Use the lightweight Cairo GSK renderer on integrated/virtual GPUs.

    Hardware OpenGL/Vulkan renderers keep large host buffers (tens of MB) alive
    even for a mostly-idle window. On integrated GPUs those buffers come out of
    system RAM; Cairo draws in software with a far smaller footprint, which is
    plenty for this UI. Discrete GPUs (dedicated VRAM) keep the default hardware
    renderer. Set ``GSK_RENDERER`` explicitly to override the choice.
    """
    if os.environ.get("GSK_RENDERER"):
        return
    if not glob.glob("/dev/dri/renderD*"):
        os.environ["GSK_RENDERER"] = "cairo"
        return

    virtual_drivers = {
        "virtio_gpu", "vmwgfx", "qxl", "vboxvideo", "bochs", "cirrus", "vkms",
    }
    discrete_vram_threshold = 2 * 1024 ** 3  # 2 GiB of dedicated VRAM
    has_discrete = False
    for card in glob.glob("/sys/class/drm/card[0-9]*"):
        device = os.path.join(card, "device")
        try:
            with open(os.path.join(device, "uevent"), encoding="utf-8") as fh:
                driver = next(
                    (line.strip().split("=", 1)[1]
                     for line in fh if line.startswith("DRIVER=")), "")
        except OSError:
            continue
        if driver in virtual_drivers:
            os.environ["GSK_RENDERER"] = "cairo"
            return
        try:
            with open(os.path.join(device, "mem_info_vram_total"),
                      encoding="utf-8") as fh:
                if int(fh.read().strip()) >= discrete_vram_threshold:
                    has_discrete = True
        except (OSError, ValueError):
            pass
    if not has_discrete:
        os.environ["GSK_RENDERER"] = "cairo"


_prefer_software_renderer()

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gio, GLib, Gdk  # noqa: E402

_BUTTON_RE = re.compile(
    r"\[(DESKTOP|HA)\]\s+Button\s+(\d+)\s+->\s+(SINGLE|DOUBLE|HOLD)",
    re.IGNORECASE,
)


# ---------------------------------------------------------------------------
# Serial events (called from the listener thread → moved to the GTK main loop)
# ---------------------------------------------------------------------------
def _handle_serial_line(line: str) -> bool:
    import state
    from serial_listener import parse_hello

    state.emit("serial-log", line=line)

    # Identity line from the firmware: learn the pad name and firmware version.
    hello = parse_hello(line)
    if hello is not None:
        state.set_device_name(hello["name"])
        state.set_firmware_version(hello["version"])

    upper = line.upper()
    if "MODE: HOME ASSISTANT" in upper:
        state.set_mode(1)
    elif "MODE: DESKTOP" in upper:
        state.set_mode(0)

    match = _BUTTON_RE.search(line)
    if match:
        firmware_mode = 1 if match.group(1).upper() == "HA" else 0
        if firmware_mode != state.mode_index:
            state.set_mode(firmware_mode)
        state.emit("hardware-event",
                   index=int(match.group(2)) - 1,
                   trigger=match.group(3).lower())
    return False


def _on_serial_line(line: str) -> None:
    GLib.idle_add(_handle_serial_line, line)


# ---------------------------------------------------------------------------
# Preload data in the background (never blocks the main loop)
# ---------------------------------------------------------------------------
def _refresh_ha_status() -> None:
    from home_assistant import ha_client
    import state
    from ui import util

    status = ha_client.get_status(ping=False)
    if isinstance(status, dict):
        state.set_ha_status(bool(status.get("connected")),
                            str(status.get("message", "")))

    def done(result) -> None:
        if isinstance(result, dict):
            state.set_ha_status(bool(result.get("connected")),
                                str(result.get("message", "")))

    util.run_async(lambda: ha_client.get_status(ping=True), done)


def _preload_apps() -> None:
    from app_scanner import get_installed_apps
    import state
    from ui import util

    def done(result) -> None:
        state.set_apps(result if isinstance(result, list) else [])

    util.run_async(get_installed_apps, done)


# ---------------------------------------------------------------------------
# Application
# ---------------------------------------------------------------------------
class OctoPadApplication(Adw.Application):
    def __init__(self) -> None:
        super().__init__(application_id="io.github.samvivan1.octopad",
                         flags=0)
        self.listener = None
        self.ble = None
        self.tray = None
        self.tray_ok = False
        self.window = None
        self._quitting = False
        # Autostart passes --hidden: keep the app in the tray, no window.
        self._start_hidden = False
        self.add_main_option(
            "hidden", 0, GLib.OptionFlags.NONE, GLib.OptionArg.NONE,
            "Start in the background (tray only, no window)", None)

    def do_handle_local_options(self, options) -> int:
        if options.contains("hidden"):
            self._start_hidden = True
        return -1

    def _request_quit(self) -> None:
        self._quitting = True
        self.quit()

    def _show_app_window(self) -> None:
        if self.window is None:
            from ui.window import OctoPadWindow
            self.window = OctoPadWindow(self)
        self.window.present()

    def do_startup(self) -> None:
        Adw.Application.do_startup(self)

        from ui.css import load_css
        Adw.StyleManager.get_default().set_color_scheme(
            Adw.ColorScheme.FORCE_DARK)
        display = Gdk.Display.get_default()
        if display is not None:
            load_css(display)

        self.set_accels_for_action("win.save", ["<Control>s"])
        self.set_accels_for_action("win.save-preset", ["<Control><Shift>s"])
        self.set_accels_for_action("app.quit", ["<Control>q"])
        quit_action = Gio.SimpleAction.new("quit", None)
        quit_action.connect("activate", lambda *_: self._request_quit())
        self.add_action(quit_action)

        import state

        try:
            from serial_listener import SerialDaemonListener
            self.listener = SerialDaemonListener(on_event_callback=_on_serial_line)
            state.listener = self.listener
            self.listener.start()
        except Exception as exc:  # noqa: BLE001
            print(f"[APP] Serial listener unavailable: {exc}")

        try:
            from ble_bridge import BleBridge
            self.ble = BleBridge(on_event_callback=_on_serial_line)
            state.ble = self.ble
            self.ble.start()
        except Exception as exc:  # noqa: BLE001
            print(f"[APP] BLE bridge unavailable: {exc}")

        # The tray must come up regardless of peripheral failures: it is what
        # keeps the app alive and provides the hide-to-tray behaviour.
        try:
            self._setup_tray()
        except Exception as exc:  # noqa: BLE001
            print(f"[APP] Tray unavailable: {exc}")

        # Stay alive in the tray even with no window (autostart/background).
        if self.tray_ok:
            self.hold()

        try:
            _refresh_ha_status()
        except Exception as exc:  # noqa: BLE001
            print(f"[APP] Home Assistant status unavailable: {exc}")
        try:
            _preload_apps()
        except Exception as exc:  # noqa: BLE001
            print(f"[APP] App scan unavailable: {exc}")

    def _setup_tray(self) -> None:
        from tray import StatusNotifierTray
        import state

        def status_labels():
            port = state.serial_port or "—"
            ble_state = "Connected" if state.ble_connected else "—"
            mode = "Home Assistant" if state.mode_index == 1 else "Desktop"
            return [f"Serial: {port}", f"BLE: {ble_state}", f"Mode: {mode}"]

        self.tray = StatusNotifierTray(
            on_show=self._show_app_window,
            on_quit=self._request_quit,
            status_labels=status_labels)
        self.tray_ok = self.tray.start()

    def do_activate(self) -> None:
        if not getattr(self, "_css_loaded", False):
            from ui.css import load_css
            display = Gdk.DisplayManager.get().get_default_display()
            if display is not None:
                load_css(display)
            self._css_loaded = True
        if self._start_hidden:
            # Autostart: keep running in the tray without opening a window.
            # A later launch (or the tray icon) activates and shows it.
            self._start_hidden = False
            if self.tray_ok:
                return
        self._show_app_window()

    def do_shutdown(self) -> None:
        if self.tray is not None:
            try:
                self.tray.stop()
            except Exception as exc:  # noqa: BLE001
                print(f"[APP] Failed to stop tray: {exc}")
        if self.ble is not None:
            try:
                self.ble.stop()
            except Exception as exc:  # noqa: BLE001
                print(f"[APP] Failed to stop BLE bridge: {exc}")
        if self.listener is not None:
            try:
                self.listener.stop()
            except Exception as exc:  # noqa: BLE001
                print(f"[APP] Failed to stop listener: {exc}")
        Adw.Application.do_shutdown(self)


def main() -> int:
    return OctoPadApplication().run(sys.argv)


if __name__ == "__main__":
    sys.exit(main())
