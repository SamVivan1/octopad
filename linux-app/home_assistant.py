#!/usr/bin/env python3
"""
OctoPad - Native Home Assistant REST Client
Connects directly to Home Assistant API without bash scripts or external curl dependencies.
"""

import os
import json
import shutil
import subprocess
import socket
import time
import urllib.error
import urllib.request
from typing import Dict, Any, List, Tuple, Optional


def _http_request(method: str, url: str, headers: Optional[dict] = None,
                  body: Optional[bytes] = None,
                  timeout: float = 5.0) -> Tuple[int, bytes]:
    """Minimal HTTP request using only the standard library.

    Using urllib instead of the third-party ``requests`` package keeps the
    process about 20 MB lighter. Returns ``(status_code, body_bytes)``; HTTP
    error responses are returned normally (not raised). Network/timeout
    failures propagate as exceptions for the caller to handle.
    """
    request = urllib.request.Request(
        url, data=body, headers=headers or {}, method=method)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read()


ENV_CONFIG = os.path.expanduser('~/.config/home-assistant/env')
LOCAL_CONFIG = os.path.expanduser('~/.config/octopad/ha_config.json')
ENTITIES_CACHE = os.path.expanduser('~/.config/octopad/ha_entities_cache.json')

# Timeout requests (connect, read) — short so the UI never feels frozen.
TIMEOUT_TEST = (2.0, 3.0)    # explicit connection test (Test button)
TIMEOUT_PING = (1.0, 2.0)    # background check (status pill)
TIMEOUT_SCAN = (2.0, 8.0)    # forced live scan (Refresh / Save button)
TIMEOUT_SCAN_BG = (1.5, 5.0)  # first scan while the cache is still empty

# Minimum delay before rescanning live after a previous scan failed (seconds).
# Prevent repeated network requests that make the endpoint feel slow.
SCAN_RETRY_DELAY = 30.0

# Maximum age of the last connection check before it is considered stale (seconds).
STATUS_TTL = 15

# Domains that can actually be controlled from the macropad (they have HA services).
# Sensor / binary_sensor / device_tracker etc. are intentionally skipped as non-interactive.
INTERACTIVE_SERVICES: Dict[str, List[str]] = {
    "light": ["toggle", "turn_on", "turn_off"],
    "switch": ["toggle", "turn_on", "turn_off"],
    "group": ["toggle", "turn_on", "turn_off"],
    "input_boolean": ["toggle", "turn_on", "turn_off"],
    "fan": ["toggle", "turn_on", "turn_off", "set_percentage"],
    "cover": ["toggle", "open_cover", "close_cover", "stop_cover", "set_cover_position"],
    "climate": ["toggle", "set_temperature", "set_hvac_mode", "set_fan_mode"],
    "humidifier": ["toggle", "turn_on", "turn_off", "set_humidity"],
    "water_heater": ["turn_on", "turn_off", "set_temperature"],
    "lock": ["lock", "unlock"],
    "vacuum": ["start", "pause", "stop", "return_to_base", "locate", "toggle"],
    "media_player": ["play_pause", "media_play", "media_pause", "media_next_track",
                     "media_previous_track", "volume_up", "volume_down", "volume_mute",
                     "volume_set", "toggle", "turn_on", "turn_off"],
    "remote": ["toggle", "turn_on", "turn_off", "send_command"],
    "scene": ["turn_on"],
    "script": ["turn_on"],
    "automation": ["trigger", "turn_on", "turn_off"],
    "button": ["press"],
    "input_button": ["press"],
    "timer": ["start", "pause", "cancel"],
    "number": ["set_value"],
    "input_number": ["set_value"],
    "select": ["select_option"],
    "input_select": ["select_option"],
    "text": ["set_value"],
    "siren": ["turn_on", "turn_off", "toggle"],
    "alarm_control_panel": ["disarm", "arm_home", "arm_away", "arm_night"],
    "update": ["install"],
    "tts": ["speak"],
}

DOMAIN_ICONS: Dict[str, str] = {
    "light": "lightbulb",      "switch": "toggle-right", "group": "package",
    "input_boolean": "toggle-left", "fan": "fan",        "cover": "chevrons-up-down",
    "climate": "thermometer",  "humidifier": "droplets", "water_heater": "droplet",
    "lock": "lock",            "vacuum": "bot",          "media_player": "music",
    "remote": "radio",         "scene": "clapperboard",  "script": "scroll-text",
    "automation": "settings",  "button": "circle-dot",   "input_button": "circle-dot",
    "timer": "timer",          "number": "hash",         "input_number": "hash",
    "select": "list",          "input_select": "list",   "text": "type",
    "siren": "siren",          "alarm_control_panel": "shield",
    "update": "package",       "tts": "volume-2",
}
DEFAULT_ICON = "package"

# Built-in Home Assistant icons (format "mdi:xxx") -> internal app icon names
# (used as entity icon labels; the UI uses GTK theme icons when available).
MDI_ICON_ALIASES: Dict[str, str] = {
    "lightbulb": "lightbulb", "lightbulb-outline": "lightbulb", "lamp": "lightbulb",
    "ceiling-light": "lightbulb", "wall-sconce": "lightbulb",
    "power-plug": "plug", "power-socket": "plug", "power-socket-eu": "plug",
    "lightswitch": "toggle-right", "lightbulb-on": "lightbulb",
    "curtain": "chevrons-up-down", "blinds": "chevrons-up-down",
    "blinds-horizontal": "chevrons-up-down", "garage": "chevrons-up-down",
    "window-closed": "chevrons-up-down", "window-open": "chevrons-up-down",
    "thermometer": "thermometer", "thermometer-water": "thermometer",
    "heating-coil": "thermometer", "humidity": "droplets",
    "water-pump": "droplet", "shower": "droplet", "shower-head": "droplet",
    "faucet": "droplet", "water": "droplet", "hot-tub": "droplet",
    "lock": "lock", "lock-open": "lock", "lock-pattern": "lock",
    "robot-vacuum": "bot", "robot": "bot", "robot-industrial": "bot",
    "speaker": "music", "music": "music", "music-note": "music",
    "television": "monitor", "television-classic": "monitor", "projector": "monitor",
    "remote": "radio", "remote-tv": "radio",
    "movie": "clapperboard", "movie-open": "clapperboard", "film": "clapperboard",
    "script-text": "scroll-text", "script": "scroll-text", "note-text": "scroll-text",
    "cog": "settings", "cogs": "settings", "cog-outline": "settings",
    "timer": "timer", "timer-outline": "timer", "stopwatch": "timer",
    "counter": "hash", "sort-numeric": "hash", "numeric": "hash",
    "format-list-bulleted": "list", "format-list-bulleted-square": "list",
    "format-list-checks": "list", "playlist": "list",
    "text": "type", "form-textbox": "type", "format-letter-case": "type",
    "bell-ring": "siren", "alarm": "siren", "bell": "siren",
    "shield": "shield", "shield-home": "shield", "shield-check": "shield",
    "package": "package", "package-variant": "package", "update": "package",
    "volume-high": "volume-2", "volume": "volume-2", "volume-medium": "volume-2",
    "fan": "fan", "ceiling-fan": "fan", "fan-off": "fan",
    "home": "house", "home-outline": "house", "house": "house",
    "circle": "circle-dot", "circle-outline": "circle-dot",
}

INTERACTIVE_DOMAINS = set(INTERACTIVE_SERVICES.keys())

# Most sensible service for each domain when the user has just picked an entity
DEFAULT_SERVICE_OVERRIDES: Dict[str, str] = {
    "automation": "trigger",
    "button": "press",
    "input_button": "press",
    "scene": "turn_on",
    "script": "turn_on",
    "media_player": "play_pause",
    "cover": "toggle",
    "lock": "lock",
    "vacuum": "start",
    "timer": "start",
    "update": "install",
    "alarm_control_panel": "disarm",
}

def resolve_icon(raw_icon: Optional[str], domain: str) -> str:
    """Map a Home Assistant icon (mdi:xxx) / Lucide name -> Lucide icon name."""
    if isinstance(raw_icon, str) and raw_icon.strip():
        name = raw_icon.strip()
        if name.startswith("mdi:"):
            name = name.split(":", 1)[1]
        mapped = MDI_ICON_ALIASES.get(name.lower().replace("_", "-"))
        if mapped:
            return mapped
    return DOMAIN_ICONS.get(domain, DEFAULT_ICON)


def _load_disk_cache() -> Dict[str, Any]:
    """Read the last scan cache (for when HA is offline)."""
    try:
        with open(ENTITIES_CACHE, 'r', encoding='utf-8') as f:
            data = json.load(f)
        if isinstance(data, dict):
            return data
    except Exception:
        pass
    return {}


def _save_disk_entities(entities: List[Dict[str, Any]], url: str) -> None:
    """Save the last scan to disk so it stays selectable while HA is offline."""
    try:
        os.makedirs(os.path.dirname(ENTITIES_CACHE), exist_ok=True)
        with open(ENTITIES_CACHE, 'w', encoding='utf-8') as f:
            json.dump({"saved_at": int(time.time()), "url": url,
                       "entities": entities}, f, indent=2)
    except Exception as e:
        print(f"[HA] Failed to write entity cache: {e}")


def _clear_disk_entities() -> None:
    try:
        if os.path.exists(ENTITIES_CACHE):
            os.remove(ENTITIES_CACHE)
    except Exception:
        pass


def decorate_entities(entities: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """Attach the service list, icon & state so it is ready for the UI entity picker."""
    decorated: List[Dict[str, Any]] = []
    for ent in entities:
        entity_id = ent.get("entity_id", "")
        domain = ent.get("domain") or (entity_id.split(".")[0] if "." in entity_id else "")
        if domain not in INTERACTIVE_SERVICES:
            continue
        services = INTERACTIVE_SERVICES[domain]
        item = dict(ent)
        item["entity_id"] = entity_id
        item["domain"] = domain
        item["state"] = ent.get("state")
        item["services"] = services
        item["default_service"] = (
            ent.get("default_service")
            or DEFAULT_SERVICE_OVERRIDES.get(domain)
            or ("toggle" if "toggle" in services else services[0])
        )
        item["icon"] = resolve_icon(ent.get("icon"), domain)
        decorated.append(item)
    return decorated

class HomeAssistantClient:
    def __init__(self):
        self.url: Optional[str] = None
        self.token: Optional[str] = None
        self._cached_entities: List[Dict[str, Any]] = []
        self._last_check: Optional[Dict[str, Any]] = None  # {"at", "ok", "message"}
        self._last_scan_live = False  # True when the cache came from a live scan
        self._last_scan_at = 0.0
        self._last_scan_ok = False
        self._disk_loaded = False  # entity cache is loaded lazily on first use
        self.load_config()

    def load_config(self):
        """Load HA_URL and HA_TOKEN from local config or ~/.config/home-assistant/env.

        The (potentially large) entity cache is intentionally *not* read here:
        it is loaded lazily by get_entities() so an idle/background app does not
        hold hundreds of entity dicts in memory.
        """
        # 1. Try ~/.config/octopad/ha_config.json
        if os.path.exists(LOCAL_CONFIG):
            try:
                with open(LOCAL_CONFIG, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                    self.url = data.get('url')
                    self.token = data.get('token')
                    if self.url and self.token:
                        return
            except Exception:
                pass

        # 2. Try ~/.config/home-assistant/env (migrated from the old setup)
        if os.path.exists(ENV_CONFIG):
            try:
                with open(ENV_CONFIG, 'r', encoding='utf-8') as f:
                    for line in f:
                        line = line.strip()
                        if line.startswith('HA_URL='):
                            self.url = line.split('=', 1)[1].strip('"\'')
                        elif line.startswith('HA_TOKEN='):
                            self.token = line.split('=', 1)[1].strip('"\'')

            except Exception:
                pass

    def _load_cached_entities(self):
        """Fetch the last scan from disk, as long as it came from the same HA instance."""
        self._disk_loaded = True
        cache = _load_disk_cache()
        if cache.get("url") and cache.get("url") != (self.url or "").rstrip('/'):
            return
        entities = [e for e in (cache.get("entities") or []) if isinstance(e, dict)]
        if entities:
            self._cached_entities = decorate_entities(entities)

    def save_config(self, url: str, token: str) -> bool:
        """Save HA credentials."""
        self.url = url.rstrip('/')
        self.token = token.strip()
        # The URL may have changed -> drop the old cache so stale HA entities are not kept.
        self._cached_entities = []
        self._last_check = None
        self._last_scan_live = False
        self._last_scan_at = 0.0
        self._last_scan_ok = False
        self._disk_loaded = True
        _clear_disk_entities()
        os.makedirs(os.path.dirname(LOCAL_CONFIG), exist_ok=True)
        try:
            with open(LOCAL_CONFIG, 'w', encoding='utf-8') as f:
                json.dump({"url": self.url, "token": self.token}, f, indent=2)
            # The file holds a long-lived access token: keep it owner-only.
            try:
                os.chmod(LOCAL_CONFIG, 0o600)
            except OSError:
                pass
            return True
        except Exception as e:
            print(f"[HA] Error saving config: {e}")
            return False

    def is_configured(self) -> bool:
        return bool(self.url and self.token)

    def get_status(self, ping: bool = False) -> Dict[str, Any]:
        """Concise HA connection status.

        Default (ping=False): instant, no network — only reads the last still-cached
        check result. ping=True performs a live check (max ~3 s).
        """
        if not self.is_configured():
            self._last_check = None
            return {
                "configured": False,
                "url": self.url,
                "connected": False,
                "message": "Not configured — enter the Home Assistant URL & token",
                "cached": True,
            }

        if ping:
            ok, msg = self.check_connection(timeout=TIMEOUT_PING)
            self._last_check = {"at": time.time(), "ok": ok, "message": msg}
            return {"configured": True, "url": self.url, "connected": ok,
                    "message": msg, "cached": False}

        if self._last_check and (time.time() - self._last_check["at"]) <= STATUS_TTL:
            return {"configured": True, "url": self.url,
                    "connected": self._last_check["ok"],
                    "message": self._last_check["message"], "cached": True}

        return {"configured": True, "url": self.url, "connected": False,
                "message": "Not checked yet — click Test Connection for a live check",
                "cached": True}

    def check_connection(self, url: Optional[str] = None, token: Optional[str] = None,
                         timeout: Tuple[float, float] = TIMEOUT_TEST) -> Tuple[bool, str]:
        """Test API reachability and authentication (optionally with unsaved credentials)."""
        target_url = (url or self.url or "").rstrip('/')
        target_token = (token or self.token or "").strip()

        if not target_url or not target_token:
            return False, "Home Assistant URL or Token not configured"

        try:
            headers = {"Authorization": f"Bearer {target_token}"}
            status, body = _http_request(
                "GET", f"{target_url}/api/", headers=headers,
                timeout=timeout[1])
            if status == 200:
                try:
                    data = json.loads(body.decode("utf-8", "replace"))
                except ValueError:
                    data = {}
                msg = data.get("message", "API running")
                return True, f"Connected to Home Assistant ({msg})"
            elif status == 401:
                return False, "Authentication failed: invalid Token (401 Unauthorized)"
            else:
                return False, f"Server responded with HTTP {status}"
        except (socket.timeout, TimeoutError):
            return False, f"Connection timeout to {target_url} (check the LAN)"
        except Exception as e:
            return False, f"Failed to reach Home Assistant: {str(e)}"

    def get_entities(self, force_refresh: bool = False) -> List[Dict[str, Any]]:
        """Scan all interactive (controllable) entities from Home Assistant states.

        The last scan is also saved to disk so the entity list stays available
        even while HA is offline. There is no hardcoded IP/entity here.
        """
        if not self._disk_loaded:
            self._load_cached_entities()
        if self._cached_entities and not force_refresh:
            return self._cached_entities

        # The last live scan failed -> do not retry within SCAN_RETRY_DELAY seconds,
        # so the endpoint stays instant when HA really is unreachable.
        if (not force_refresh and self._last_scan_at and not self._last_scan_ok
                and (time.monotonic() - self._last_scan_at) < SCAN_RETRY_DELAY):
            return self._cached_entities

        if self.is_configured():
            timeout = TIMEOUT_SCAN if force_refresh else TIMEOUT_SCAN_BG
            self._last_scan_at = time.monotonic()
            try:
                headers = {"Authorization": f"Bearer {self.token}"}
                status, body = _http_request(
                    "GET", f"{self.url}/api/states", headers=headers,
                    timeout=timeout[1])
                if status == 200:
                    raw_states = json.loads(body.decode("utf-8", "replace"))
                    parsed = []
                    for state in raw_states:
                        entity_id = state.get("entity_id", "")
                        domain = entity_id.split(".")[0] if "." in entity_id else "unknown"
                        if domain not in INTERACTIVE_DOMAINS:
                            continue  # skip sensor, binary_sensor, device_tracker, etc.
                        attrs = state.get("attributes", {})
                        parsed.append({
                            "entity_id": entity_id,
                            "domain": domain,
                            "friendly_name": attrs.get("friendly_name", entity_id),
                            "state": state.get("state"),
                            "icon": attrs.get("icon"),
                        })

                    parsed.sort(key=lambda x: (x["domain"], x["friendly_name"].lower()))
                    self._cached_entities = decorate_entities(parsed)
                    self._last_scan_live = True
                    self._last_scan_ok = True
                    _save_disk_entities(self._cached_entities, (self.url or "").rstrip('/'))
                    return self._cached_entities
            except Exception as e:
                print(f"[HA] Failed to fetch live entities from {self.url}: {e}")

            self._last_scan_ok = False

        # HA unreachable / not configured -> use the last scan from disk
        self._last_scan_live = False
        return self._cached_entities

    def call_service(self, domain: str, service: str, entity_id: Optional[str] = None, data: Optional[Dict[str, Any]] = None) -> Tuple[bool, str]:
        """Call a Home Assistant service natively via HTTP REST API."""
        if not self.is_configured():
            self._notify_desktop("OctoPad / Home Assistant", "Home Assistant config not set")
            return False, "Home Assistant not configured"

        payload = data or {}
        if entity_id:
            payload["entity_id"] = entity_id

        url = f"{self.url}/api/services/{domain}/{service}"
        headers = {
            "Authorization": f"Bearer {self.token}",
            "Content-Type": "application/json"
        }

        try:
            status, body = _http_request(
                "POST", url, headers=headers,
                body=json.dumps(payload).encode("utf-8"), timeout=5.0)
            if status == 200:
                target_label = entity_id or f"{domain}.{service}"
                self._notify_desktop("OctoPad / Home Assistant", f"{domain}.{service}: {target_label}")
                return True, f"Service {domain}.{service} executed successfully"
            else:
                msg = f"HTTP {status}: {body[:100].decode('utf-8', 'replace')}"
                self._notify_desktop("OctoPad / Home Assistant", f"Failed ({status})")
                return False, msg
        except Exception as e:
            self._notify_desktop("OctoPad / Home Assistant", "Failed to reach Home Assistant")
            return False, f"Connection error: {str(e)}"

    @staticmethod
    def _notify_desktop(title: str, message: str):
        """Send native desktop notification without blocking."""
        if shutil.which("notify-send"):
            try:
                subprocess.Popen(["notify-send", title, message], start_new_session=True)
            except Exception:
                pass

ha_client = HomeAssistantClient()

if __name__ == '__main__':
    print(f"HA Configured: {ha_client.is_configured()} (URL: {ha_client.url})")
    ok, msg = ha_client.check_connection()
    print(f"Status: {ok} -> {msg}")
    entities = ha_client.get_entities()
    print(f"Entities count: {len(entities)}")
    for e in entities[:5]:
        print(f" - [{e['domain']}] {e['friendly_name']} ({e['entity_id']})")
