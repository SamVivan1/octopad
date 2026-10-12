#!/usr/bin/env python3
"""
OctoPad - Ubuntu Desktop Application Scanner & Launcher
Parses XDG .desktop files across system, local user, snap, and flatpak locations.
"""

import os
import glob
import subprocess
import shutil
from typing import List, Dict, Optional, Tuple

SEARCH_DIRS = [
    os.path.expanduser('~/.local/share/applications'),
    '/var/lib/flatpak/exports/share/applications',
    '/var/lib/snapd/desktop/applications',
    '/usr/share/applications'
]

_cached_apps: Optional[List[Dict[str, str]]] = None

def parse_desktop_file(filepath: str) -> Optional[Dict[str, str]]:
    """Parse an XDG .desktop file and extract application metadata."""
    data = {}
    in_entry = False
    try:
        with open(filepath, 'r', encoding='utf-8', errors='ignore') as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith('#'):
                    continue
                if line.startswith('['):
                    in_entry = (line == '[Desktop Entry]')
                    continue
                if in_entry and '=' in line:
                    key, val = line.split('=', 1)
                    key = key.strip()
                    val = val.strip()
                    # Only grab default locale (ignore Key[locale])
                    if '[' not in key and key not in data:
                        data[key] = val
    except Exception:
        return None

    # Filter out non-applications or hidden helper launchers
    if data.get('Type') != 'Application':
        return None
    if data.get('NoDisplay', '').lower() == 'true':
        return None

    name = data.get('Name')
    exec_cmd = data.get('Exec')
    if not name or not exec_cmd:
        return None

    # Clean up exec command placeholders (%f, %F, %u, %U, etc.)
    clean_exec = exec_cmd
    for field in ['%f', '%F', '%u', '%U', '%d', '%D', '%n', '%N', '%i', '%c', '%k', '%v', '%m']:
        clean_exec = clean_exec.replace(field, '').strip()

    return {
        'id': os.path.basename(filepath),
        'name': name,
        'exec': clean_exec,
        'raw_exec': exec_cmd,
        'icon': data.get('Icon', 'application-x-executable'),
        'comment': data.get('Comment', ''),
        'categories': data.get('Categories', ''),
        'terminal': data.get('Terminal', '').lower() == 'true',
        'path': filepath
    }

def get_installed_apps(force_refresh: bool = False) -> List[Dict[str, str]]:
    """Scan all standard directories and return a sorted list of unique visible applications."""
    global _cached_apps
    if _cached_apps is not None and not force_refresh:
        return _cached_apps

    apps = []
    seen_ids = set()

    for directory in SEARCH_DIRS:
        if not os.path.isdir(directory):
            continue
        desktop_files = glob.glob(os.path.join(directory, '*.desktop'))
        for filepath in desktop_files:
            app_id = os.path.basename(filepath)
            if app_id in seen_ids:
                continue
            seen_ids.add(app_id)
            parsed = parse_desktop_file(filepath)
            if parsed:
                apps.append(parsed)

    apps.sort(key=lambda x: x['name'].lower())
    _cached_apps = apps
    return apps

def launch_app(desktop_id_or_path: str) -> Tuple[bool, str]:
    """
    Launch an application on the user's graphical session.
    Prioritizes gio launch / gtk-launch for proper environment detachment.
    """
    if not desktop_id_or_path:
        return False, "Target desktop ID or path is empty"

    # 1. Try gio launch if full path or desktop id
    if os.path.exists(desktop_id_or_path):
        target_path = desktop_id_or_path
    else:
        # Search for full path
        target_path = None
        for d in SEARCH_DIRS:
            candidate = os.path.join(d, desktop_id_or_path)
            if os.path.exists(candidate):
                target_path = candidate
                break

    # If gtk-launch is available, use it with basename
    desktop_basename = os.path.basename(desktop_id_or_path)
    if shutil.which('gtk-launch'):
        try:
            # Strip .desktop suffix if present for gtk-launch
            launcher_name = desktop_basename
            if launcher_name.endswith('.desktop'):
                launcher_name = launcher_name[:-8]
            subprocess.Popen(['gtk-launch', launcher_name], start_new_session=True)
            return True, f"Launched via gtk-launch: {launcher_name}"
        except Exception:
            pass

    # 2. Try gio launch
    if target_path and shutil.which('gio'):
        try:
            subprocess.Popen(['gio', 'launch', target_path], start_new_session=True)
            return True, f"Launched via gio launch: {target_path}"
        except Exception:
            pass

    # 3. Fallback: Parse Exec from file and run directly
    if target_path:
        entry = parse_desktop_file(target_path)
        if entry and entry.get('exec'):
            try:
                subprocess.Popen(entry['exec'], shell=True, start_new_session=True)
                return True, f"Launched via direct shell exec: {entry['exec']}"
            except Exception as e:
                return False, f"Direct exec failed: {str(e)}"

    return False, f"Could not find or launch application: {desktop_id_or_path}"

if __name__ == '__main__':
    all_apps = get_installed_apps()
    print(f"Total applications discovered: {len(all_apps)}")
    for a in all_apps[:10]:
        print(f" • {a['name']:<30} [{a['id']}] -> {a['exec']}")
