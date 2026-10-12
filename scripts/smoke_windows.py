#!/usr/bin/env python3
"""Headless smoke test for the Windows application.

The Windows app is a PySide6/Qt6 program, so it can be exercised on any
platform that can install PySide6 — no display needed. Running it with the Qt
``offscreen`` platform plugin builds the main window, every page and every
dialog, then runs the event loop for a moment. This catches import errors,
missing widget APIs and construction-time exceptions that a plain
``compileall`` cannot see.

Usage:
    python scripts/smoke_windows.py            # from the repository root
    QT_QPA_PLATFORM=offscreen python scripts/smoke_windows.py

Requires: PySide6 (from ``windows-app/requirements.txt``).
"""

import os
import sys
import traceback

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WINDOWS_APP = os.path.join(REPO_ROOT, "windows-app")

# Never touch (or require) a real Windows profile and always run headless.
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ.setdefault("APPDATA", os.path.join(REPO_ROOT, ".smoke-appdata"))
os.environ.setdefault("XDG_CONFIG_HOME", os.path.join(REPO_ROOT, ".smoke-config"))

if WINDOWS_APP not in sys.path:
    sys.path.insert(0, WINDOWS_APP)


class _FakeApp:
    """Minimal stand-in for OctoPadApp (avoids starting threads/hardware)."""

    _quitting = False
    tray_ok = False


def _check(label: str) -> bool:
    print(f"  ok  {label}")
    return True


def main() -> int:
    try:
        from PySide6.QtCore import QTimer
        from PySide6.QtWidgets import QApplication
    except ImportError as exc:
        print(f"PySide6 is required for the smoke test: {exc}")
        print("Install it with: pip install -r windows-app/requirements.txt")
        return 2

    app = QApplication(sys.argv)
    failures = 0

    def build(label, factory):
        nonlocal failures
        try:
            widget = factory()
            if widget is not None and hasattr(widget, "show"):
                widget.show()
            return _check(label)
        except Exception:  # noqa: BLE001 - report every failing page
            nonlocal failures
            failures += 1
            print(f"FAIL  {label}")
            traceback.print_exc(limit=4)
            return False

    import state
    from ui import window as window_module
    from ui import action_editor, app_picker, console_page, ha_dialogs
    from ui import keys_page, settings_page

    pages = getattr(window_module, "_PAGES", []) or [("keys", "Keys")]

    main_window = [None]

    def _make_main():
        win = window_module.OctoPadWindow(_FakeApp())
        main_window[0] = win
        return win

    build("main window", _make_main)

    win = main_window[0]
    if win is not None:
        for name, _title in pages:
            page = getattr(win, "_pages", {}).get(name)
            if page is None:
                try:
                    page = win._get_page(name)
                except Exception:  # noqa: BLE001
                    failures += 1
                    print(f"FAIL  page: {name}")
                    traceback.print_exc(limit=4)
                    continue
            build(f"page: {name}", lambda p=page: p)

        trigger = state.triggers[0] if getattr(state, "triggers", None) else "single"
        label = "Trigger"
        build("action editor",
              lambda: action_editor.ActionEditor(win, trigger, label))
        build("app picker",
              lambda: app_picker.AppPickerDialog(win, lambda _app: None))
        build("entity picker",
              lambda: ha_dialogs.EntityPickerDialog(win, lambda _entity: None))
        build("HA connection dialog",
              lambda: ha_dialogs.HaConnectionDialog(win))

    # Run the event loop briefly so queued timers/signals fire.
    QTimer.singleShot(1500, app.quit)
    app.exec()
    print("  ok  event loop ran")

    print(f"smoke test {'passed' if failures == 0 else f'failed ({failures})'}")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
