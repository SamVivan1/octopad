#!/usr/bin/env python3
"""
OctoPad - Native Windows application (PySide6 / Qt6).

Everything runs directly from Python: serial listener, BLE bridge, action
execution, button mapping, and Home Assistant REST - no HTTP server, no web UI.
"""

import re
import sys

from PySide6.QtNetwork import QLocalServer, QLocalSocket
from PySide6.QtWidgets import QApplication, QMessageBox

SERVER_NAME = "OctoPad-single-instance"

_BUTTON_RE = re.compile(
    r"\[(DESKTOP|HA)\]\s+Button\s+(\d+)\s+->\s+(SINGLE|DOUBLE|HOLD)",
    re.IGNORECASE,
)

_STYLESHEET = """
QMainWindow { background: #1e1e1e; }
QWidget { color: #e0e0e0; font-size: 13px; }
QListWidget { background: #242424; border: none; padding: 6px; }
QListWidget::item { padding: 10px 8px; border-radius: 6px; }
QListWidget::item:selected { background: #3584e4; color: white; }
QPushButton { background: #333333; border: 1px solid #454545; border-radius: 6px;
              padding: 6px 12px; }
QPushButton:hover { background: #3c3c3c; }
QPushButton:checked { background: #3584e4; border-color: #3584e4; color: white; }
QPushButton[active="true"] { background: #3584e4; border-color: #3584e4; color: white; }
QLineEdit, QPlainTextEdit, QComboBox, QSpinBox, QDoubleSpinBox {
    background: #2a2a2a; border: 1px solid #454545; border-radius: 6px; padding: 4px; }
QGroupBox { border: 1px solid #3a3a3a; border-radius: 8px; margin-top: 12px;
            padding: 10px; }
QGroupBox::title { subcontrol-origin: margin; left: 12px; padding: 0 4px; }
QLabel[css~="hint"] { color: #9a9a9a; }
QMenuBar, QMenu { background: #2a2a2a; }
QToolBar { background: #242424; border: none; padding: 4px; }
QStatusBar { background: #242424; }
"""


def _handle_serial_line(line: str) -> None:
    import state
    from serial_listener import parse_hello_line

    state.emit("serial-log", line=line)

    hello = parse_hello_line(line)
    if hello is not None:
        state.set_device_name(hello)

    match = _BUTTON_RE.search(line)
    if match:
        mode = "ha" if match.group(1).lower() == "ha" else "desktop"
        index = int(match.group(2)) - 1
        trigger = match.group(3).lower()
        state.emit("hardware-event", mode=mode, index=index, trigger=trigger)


class OctoPadApp:
    def __init__(self, argv) -> None:
        self.start_hidden = "--hidden" in argv
        self.qt_app = QApplication([a for a in argv if a != "--hidden"])
        self.qt_app.setApplicationName("OctoPad")
        self.qt_app.setApplicationDisplayName("OctoPad")
        self.qt_app.setOrganizationName("OctoPad")
        self.qt_app.setStyleSheet(_STYLESHEET)

        import state
        self.state = state
        self.listener = None
        self.ble = None
        self.tray = None
        self.tray_ok = False
        self.window = None
        self._quitting = False

        self.qt_app.aboutToQuit.connect(self._shutdown)

    # ------------------------------------------------------------------
    def run(self) -> int:
        if self._notify_existing():
            return 0

        self._server = QLocalServer()
        QLocalServer.removeServer(SERVER_NAME)
        self._server.listen(SERVER_NAME)
        self._server.newConnection.connect(self._on_new_connection)

        self._start_listener()
        self._start_ble()
        self._start_tray()
        self._refresh_ha_status()
        self._preload_apps()

        # Autostart runs hidden in the tray; without a tray we still need a
        # window, otherwise the event loop would have nothing to keep it alive.
        if not self.start_hidden or not self.tray_ok:
            self._show_window()

        return self.qt_app.exec()

    # ------------------------------------------------------------------
    def _notify_existing(self) -> bool:
        socket = QLocalSocket()
        socket.connectToServer(SERVER_NAME)
        if not socket.waitForConnected(300):
            return False
        if not self.start_hidden:
            socket.write(b"show\n")
            socket.waitForBytesWritten(300)
        socket.disconnectFromServer()
        return True

    def _on_new_connection(self) -> None:
        connection = self._server.nextPendingConnection()
        if connection is not None:
            connection.readyRead.connect(lambda: self._on_message(connection))

    def _on_message(self, connection) -> None:
        _ = bytes(connection.readAll()).decode("utf-8", "ignore")
        self._show_window()

    # ------------------------------------------------------------------
    def _show_window(self) -> None:
        if self.window is None:
            from ui.window import OctoPadWindow
            self.window = OctoPadWindow(self)
        self.window.show()
        self.window.raise_()
        self.window.activateWindow()

    def _start_listener(self) -> None:
        from serial_listener import SerialDaemonListener
        self.listener = SerialDaemonListener(on_event_callback=_handle_serial_line)
        self.state.listener = self.listener
        self.listener.start()

    def _start_ble(self) -> None:
        from ble_bridge import BleBridge
        self.ble = BleBridge(on_event_callback=_handle_serial_line)
        self.ble.start()

    def _start_tray(self) -> None:
        from tray import OctoTray

        def status_labels():
            return [
                f"Serial: {self.state.serial_port or '-'}",
                f"BLE: {'Connected' if self.state.ble_connected else '-'}",
                f"Mode: {'Home Assistant' if self.state.mode_index == 1 else 'Desktop'}",
            ]

        self.tray = OctoTray(on_show=self._show_window, on_quit=self._request_quit,
                             status_labels=status_labels)
        self.tray_ok = self.tray.start()

    # ------------------------------------------------------------------
    def _refresh_ha_status(self) -> None:
        from home_assistant import ha_client

        def done(result):
            if isinstance(result, dict):
                self.state.set_ha_status(bool(result.get("connected")),
                                         str(result.get("message", "")))

        def work():
            status = ha_client.get_status(ping=False)
            if isinstance(status, dict):
                self.state.set_ha_status(bool(status.get("connected")),
                                         str(status.get("message", "")))
            return ha_client.get_status(ping=True)

        self.state.run_async(work, done)

    def _preload_apps(self) -> None:
        from app_scanner import get_installed_apps

        def done(result):
            self.state.set_apps(result if isinstance(result, list) else [])

        self.state.run_async(get_installed_apps, done)

    # ------------------------------------------------------------------
    def _request_quit(self) -> None:
        self._quitting = True
        self.qt_app.quit()

    def _shutdown(self) -> None:
        for component in (self.tray, self.ble, self.listener):
            if component is not None:
                try:
                    component.stop()
                except Exception:  # noqa: BLE001
                    pass


def main() -> int:
    app = OctoPadApp(sys.argv)
    try:
        return app.run()
    except Exception as exc:  # noqa: BLE001
        QMessageBox.critical(None, "OctoPad", f"Fatal error: {exc}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
