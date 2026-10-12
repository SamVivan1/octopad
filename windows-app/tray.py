#!/usr/bin/env python3
"""OctoPad - system tray icon (QSystemTrayIcon)."""

from typing import Callable, List, Optional

from PySide6.QtCore import Qt
from PySide6.QtGui import QAction, QColor, QIcon, QPainter, QPixmap
from PySide6.QtWidgets import QMenu, QSystemTrayIcon


def _make_icon() -> QIcon:
    """Draw a small rounded pad glyph so no external image file is needed."""
    size = 64
    pixmap = QPixmap(size, size)
    pixmap.fill(Qt.transparent)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.Antialiasing)
    painter.setBrush(QColor("#3584e4"))
    painter.setPen(Qt.NoPen)
    painter.drawRoundedRect(4, 4, size - 8, size - 8, 14, 14)
    painter.setPen(QColor("#ffffff"))
    font = painter.font()
    font.setBold(True)
    font.setPointSize(24)
    painter.setFont(font)
    painter.drawText(pixmap.rect(), Qt.AlignCenter, "O")
    painter.end()
    return QIcon(pixmap)


class OctoTray:
    def __init__(self, on_show: Callable[[], None], on_quit: Callable[[], None],
                 status_labels: Optional[Callable[[], List[str]]] = None) -> None:
        self.on_show = on_show
        self.on_quit = on_quit
        self.status_labels = status_labels
        self._tray: Optional[QSystemTrayIcon] = None
        self._menu: Optional[QMenu] = None
        self._status_actions: List[QAction] = []

    def start(self) -> bool:
        if not QSystemTrayIcon.isSystemTrayAvailable():
            return False
        self._tray = QSystemTrayIcon(_make_icon())
        self._tray.setToolTip("OctoPad")

        menu = QMenu()
        show_action = QAction("Open OctoPad", menu)
        show_action.triggered.connect(lambda: self.on_show())
        menu.addAction(show_action)
        menu.addSeparator()

        for _ in range(3):
            action = QAction("", menu)
            action.setEnabled(False)
            menu.addAction(action)
            self._status_actions.append(action)

        menu.addSeparator()
        quit_action = QAction("Quit", menu)
        quit_action.triggered.connect(lambda: self.on_quit())
        menu.addAction(quit_action)
        menu.aboutToShow.connect(self._refresh_status)

        self._menu = menu
        self._tray.setContextMenu(menu)
        self._tray.activated.connect(self._on_activated)
        self._refresh_status()
        self._tray.show()
        return True

    def _on_activated(self, reason) -> None:
        if reason in (QSystemTrayIcon.Trigger, QSystemTrayIcon.DoubleClick):
            self.on_show()

    def _refresh_status(self) -> None:
        labels = []
        if self.status_labels is not None:
            try:
                labels = list(self.status_labels())
            except Exception:  # noqa: BLE001
                labels = []
        for index, action in enumerate(self._status_actions):
            if index < len(labels):
                action.setText(labels[index])
                action.setVisible(True)
            else:
                action.setVisible(False)

    def stop(self) -> None:
        if self._tray is not None:
            self._tray.hide()
            self._tray = None
