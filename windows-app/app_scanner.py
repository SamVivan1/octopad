#!/usr/bin/env python3
"""
OctoPad - Windows Application Scanner & Launcher.

Scans the per-user and machine-wide Start Menu folders for shortcuts
(``.lnk`` / ``.url``) and executable launchers, and opens them with the Shell
(``os.startfile``) so shortcuts resolve exactly like a Start-menu click.
"""

import os
from typing import Dict, List, Optional, Tuple

def _search_dirs() -> List[str]:
    dirs = []
    for base_dir in (
        os.path.join(os.environ.get("APPDATA", ""),
                     "Microsoft", "Windows", "Start Menu", "Programs"),
        os.path.join(os.environ.get("PROGRAMDATA", ""),
                     "Microsoft", "Windows", "Start Menu", "Programs"),
    ):
        if base_dir and os.path.isdir(base_dir):
            dirs.append(base_dir)
    return dirs


_cached_apps: Optional[List[Dict[str, str]]] = None


def get_installed_apps(force_refresh: bool = False) -> List[Dict[str, str]]:
    """Scan the Start Menu for launchable shortcuts (sorted by name)."""
    global _cached_apps
    if _cached_apps is not None and not force_refresh:
        return _cached_apps

    apps: List[Dict[str, str]] = []
    seen: set = set()
    for directory in _search_dirs():
        for root, _dirs, files in os.walk(directory):
            for filename in files:
                if not filename.lower().endswith((".lnk", ".exe")):
                    continue
                path = os.path.join(root, filename)
                name = os.path.splitext(filename)[0]
                key = name.casefold()
                if key in seen:
                    continue
                seen.add(key)
                apps.append({
                    "id": path,
                    "name": name,
                    "path": path,
                    "exec": path,
                    "icon": "",
                    "comment": os.path.relpath(root, directory),
                })

    apps.sort(key=lambda x: x["name"].lower())
    _cached_apps = apps
    return apps


def launch_app(target: str) -> Tuple[bool, str]:
    """Launch a shortcut/path using the Windows shell (os.startfile)."""
    if not target:
        return False, "Target path or application is empty"

    path = target
    if not os.path.exists(path):
        found = _find_by_name(target)
        if not found:
            return False, f"Could not find or launch application: {target}"
        path = found

    try:
        os.startfile(path)  # noqa: S606 - shell launch is the intent here
        return True, f"Launched {os.path.basename(path)}"
    except Exception as exc:  # noqa: BLE001
        return False, f"Failed to launch {os.path.basename(path)}: {exc}"


def _find_by_name(name: str) -> Optional[str]:
    """Best-effort lookup of a previously scanned app by name or basename."""
    basename = os.path.basename(name)
    for app in get_installed_apps():
        if app["id"] == name or app["path"] == name:
            return app["path"]
        if os.path.basename(app["path"]) == basename:
            return app["path"]
        if app["name"].casefold() == os.path.splitext(basename)[0].casefold():
            return app["path"]
    return None


if __name__ == "__main__":
    found = get_installed_apps()
    print(f"Total applications discovered: {len(found)}")
    for app in found[:10]:
        print(f" - {app['name']} [{app['id']}]")
