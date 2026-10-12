# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller build definition for the OctoPad Windows application."""

import os

from PyInstaller.utils.hooks import collect_data_files, collect_submodules

# Local modules imported lazily inside functions.
hiddenimports = [
    "config", "state", "platform_actions", "app_scanner", "home_assistant",
    "serial_listener", "ble_bridge", "tray",
    "ui", "ui.window", "ui.keys_page", "ui.console_page", "ui.profiles_page",
    "ui.settings_page", "ui.action_editor", "ui.app_picker", "ui.audio_picker",
    "ui.ha_dialogs", "ui.util",
    "serial.tools.list_ports",
]

# Third-party packages that load modules dynamically.
for package in ("bleak", "comtypes", "pycaw"):
    try:
        hiddenimports += collect_submodules(package)
    except Exception:  # noqa: BLE001 - package may be optional on some setups
        pass

datas = []
try:
    datas += collect_data_files("comtypes")
except Exception:  # noqa: BLE001
    pass

icon = "octopad.ico" if os.path.exists("octopad.ico") else None

a = Analysis(
    ["main.py"],
    pathex=[],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=["rthook_comtypes.py"],
    excludes=[],
    noarchive=False,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="OctoPad",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    icon=icon,
)
