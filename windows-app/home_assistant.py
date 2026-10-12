#!/usr/bin/env python3
"""
OctoPad - Native Home Assistant REST Client (Windows).

Connects directly to the Home Assistant API without external curl or scripts.
Credentials live in %APPDATA%\\OctoPad\\ha_config.json.
"""

import json
import os
import socket
import time
import urllib.error
import urllib.request
from typing import Any, Callable, Dict, List, Optional, Tuple


def _appdata_dir() -> str:
    base = os.environ.get("APPDATA") or os.path.expanduser("~")
    return os.path.join(base, "OctoPad")


LOCAL_CONFIG = os.path.join(_appdata_dir(), "ha_config.json")
ENTITIES_CACHE = os.path.join(_appdata_dir(), "ha_entities_cache.json")

# Optional desktop notifier (wired to the tray bubble by the application).
notifier: Optional[Callable[[str, str], None]] = None


def _http_request(method: str, url: str, headers: Optional[dict] = None,
                  body: Optional[bytes] = None,
                  timeout: float = 5.0) -> Tuple[int, bytes]:
    """Minimal HTTP request using only the standard library."""
    request = urllib.request.Request(
        url, data=body, headers=headers or {}, method=method)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read()


# Timeout requests (connect, read) — short so the UI never feels frozen.
TIMEOUT_TEST = (2.0, 3.0)
TIMEOUT_PING = (1.0, 2.0)
TIMEOUT_SCAN = (2.0, 8.0)
TIMEOUT_SCAN_BG = (1.5, 5.0)

SCAN_RETRY_DELAY = 30.0
STATUS_TTL = 15

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

# UI hint for the icon shown per domain (the Qt UI maps these to glyphs).
DOMAIN_ICONS: Dict[str, str] = {
    "light": "lightbulb", "switch": "toggle-right", "group": "package",
    "input_boolean": "toggle-left", "fan": "fan", "cover": "chevrons-up-down",
    "climate": "thermometer", "humidifier": "droplets", "water_heater": "droplet",
    "lock": "lock", "vacuum": "bot", "media_player": "music",
    "remote": "radio", "scene": "clapperboard", "script": "scroll-text",
    "automation": "settings", "button": "circle-dot", "input_button": "circle-dot",
    "timer": "timer", "number": "hash", "input_number": "hash",
    "select": "list", "input_select": "list", "text": "type",
    "siren": "siren", "alarm_control_panel": "shield",
    "update": "package", "tts": "volume-2",
}
DEFAULT_ICON = "package"

INTERACTIVE_DOMAINS = set(INTERACTIVE_SERVICES.keys())


def resolve_icon(raw_icon: Optional[str], domain: str) -> str:
    """Map a Home Assistant icon (mdi:xxx) -> a UI icon key."""
    if isinstance(raw_icon, str) and raw_icon.strip():
        name = raw_icon.strip()
        if name.startswith("mdi:"):
            name = name.split(":", 1)[1]
        mapped = DOMAIN_ICONS.get(name.lower().replace("_", "-"))
        if mapped:
            return mapped
    return DOMAIN_ICONS.get(domain, DOMAIN_ICONS.get("switch", "toggle-right"))


def _load_disk_cache() -> Dict[str, Any]:
    try:
        with open(ENTITIES_CACHE, "r", encoding="utf-8") as handle:
            data = json.load(handle)
        if isinstance(data, dict):
            return data
    except Exception:  # noqa: BLE001
        pass
    return {}


def _save_disk_entities(entities: List[Dict[str, Any]], url: str) -> None:
    try:
        os.makedirs(os.path.dirname(ENTITIES_CACHE), exist_ok=True)
        with open(ENTITIES_CACHE, "w", encoding="utf-8") as handle:
            json.dump({"saved_at": int(time.time()), "url": url,
                       "entities": entities}, handle, indent=2)
    except Exception as exc:  # noqa: BLE001
        print(f"[HA] Failed to write entity cache: {exc}")


def _clear_disk_entities() -> None:
    try:
        if os.path.exists(ENTITIES_CACHE):
            os.remove(ENTITIES_CACHE)
    except Exception:  # noqa: BLE001
        pass


def decorate_entities(entities: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """Attach services / icon / default service to each entity."""
    decorated = []
    for entity in entities:
        entity_id = entity.get("entity_id", "")
        domain = entity.get("domain") or (
            entity_id.split(".")[0] if "." in entity_id else "unknown")
        services = INTERACTIVE_SERVICES.get(domain)
        if services is None:
            continue
        decorated.append({
            "entity_id": entity_id,
            "domain": domain,
            "friendly_name": entity.get("friendly_name", entity_id),
            "state": entity.get("state"),
            "icon": entity.get("icon"),
            "services": services,
            "default_service": DEFAULT_SERVICE_OVERRIDES.get(
                domain, "toggle" if "toggle" in services else services[0]),
        })
    return decorated


class HomeAssistantClient:
    def __init__(self) -> None:
        self.url: Optional[str] = None
        self.token: Optional[str] = None
        self._cached_entities: List[Dict[str, Any]] = []
        self._last_check: Optional[Dict[str, Any]] = None
        self._last_scan_live = False
        self._last_scan_at = 0.0
        self._last_scan_ok = False
        self._disk_loaded = False
        self.load_config()

    # ------------------------------------------------------------------
    def load_config(self) -> None:
        if os.path.exists(LOCAL_CONFIG):
            try:
                with open(LOCAL_CONFIG, "r", encoding="utf-8") as handle:
                    data = json.load(handle)
                self.url = data.get("url")
                self.token = data.get("token")
                if self.url and self.token:
                    return
            except Exception:  # noqa: BLE001
                pass

    def save_config(self, url: str, token: str) -> bool:
        self.url = url.strip().rstrip("/")
        self.token = token.strip()
        self._cached_entities = []
        self._last_check = None
        self._last_scan_live = False
        self._last_scan_at = 0.0
        self._last_scan_ok = False
        self._disk_loaded = True
        _clear_disk_entities()
        try:
            os.makedirs(os.path.dirname(LOCAL_CONFIG), exist_ok=True)
            with open(LOCAL_CONFIG, "w", encoding="utf-8") as handle:
                json.dump({"url": self.url, "token": self.token}, handle, indent=2)
            # The file holds a long-lived access token: restrict it to the owner
            # where the platform supports POSIX permissions (best effort).
            try:
                os.chmod(LOCAL_CONFIG, 0o600)
            except OSError:
                pass
            return True
        except Exception as exc:  # noqa: BLE001
            print(f"[HA] Error saving config: {exc}")
            return False

    def is_configured(self) -> bool:
        return bool(self.url and self.token)

    def check_connection(self, timeout: Tuple[float, float] = (2.0, 3.0)) -> Tuple[bool, str]:
        if not self.url or not self.token:
            return False, "Home Assistant URL or Token not configured"
        try:
            headers = {"Authorization": f"Bearer {self.token}"}
            status, body = _http_request("GET", f"{self.url}/api/", headers=headers,
                                         timeout=timeout[1])
            if status == 200:
                return True, "Connected to Home Assistant"
            if status == 401:
                return False, "Authentication failed (401 Unauthorized) — check the token"
            return False, f"Server responded with HTTP {status}"
        except (socket.timeout, TimeoutError):
            return False, f"Connection timeout to {self.url} (check the LAN)"
        except Exception as exc:  # noqa: BLE001
            return False, f"Failed to reach Home Assistant: {exc}"

    def test_connection(self, url: str, token: str) -> Tuple[bool, str]:
        saved = (self.url, self.token)
        self.url, self.token = url.strip().rstrip("/"), token.strip()
        try:
            return self.check_connection()
        finally:
            self.url, self.token = saved

    def get_status(self, ping: bool = False) -> Dict[str, Any]:
        if not self.is_configured():
            return {"configured": False, "connected": False,
                    "message": "Home Assistant not configured", "url": None}
        if self._last_check is None or time.time() - self._last_check["at"] > 15:
            connected, message = self.check_connection()
            self._last_check = {"at": time.time(), "ok": connected, "message": message}
        return {"configured": True, "url": self.url,
                "connected": bool(self._last_check.get("ok")),
                "message": self._last_check.get("message", "")}

    def get_entities(self, force_refresh: bool = False) -> List[Dict[str, Any]]:
        if not self._disk_loaded:
            self._load_cached_entities()
        if self._cached_entities and not force_refresh:
            return self._cached_entities
        if (not force_refresh and self._last_scan_at and not self._last_scan_ok
                and (time.monotonic() - self._last_scan_at) < 30.0):
            return self._cached_entities

        if self.is_configured():
            timeout = 8.0 if force_refresh else 5.0
            self._last_scan_at = time.monotonic()
            try:
                headers = {"Authorization": f"Bearer {self.token}"}
                status, body = _http_request(
                    "GET", f"{self.url}/api/states", headers=headers, timeout=timeout)
                if status == 200:
                    raw_states = json.loads(body.decode("utf-8", "replace"))
                    parsed = []
                    for state in raw_states:
                        entity_id = state.get("entity_id", "")
                        domain = entity_id.split(".")[0] if "." in entity_id else "unknown"
                        if domain not in INTERACTIVE_SERVICES:
                            continue
                        attrs = state.get("attributes", {})
                        parsed.append({
                            "entity_id": entity_id, "domain": domain,
                            "friendly_name": attrs.get("friendly_name", entity_id),
                            "state": state.get("state"), "icon": attrs.get("icon"),
                        })
                    parsed.sort(key=lambda x: (x["domain"], x["friendly_name"].lower()))
                    self._cached_entities = decorate_entities(parsed)
                    self._last_scan_live = True
                    self._last_scan_ok = True
                    _save_disk_entities(self._cached_entities, (self.url or "").rstrip("/"))
                    return self._cached_entities
            except Exception as exc:  # noqa: BLE001
                print(f"[HA] Failed to fetch live entities: {exc}")
            self._last_scan_ok = False

        self._last_scan_live = False
        return self._cached_entities

    def _load_cached_entities(self) -> None:
        self._disk_loaded = True
        cache = _load_disk_cache()
        if cache.get("url") and cache.get("url") != (self.url or "").rstrip("/"):
            return
        entities = [e for e in (cache.get("entities") or []) if isinstance(e, dict)]
        if entities:
            self._cached_entities = decorate_entities(entities)

    def get_status_dict(self, ping: bool = False) -> Dict[str, Any]:
        if not self.is_configured():
            return {"configured": False, "connected": False,
                    "message": "Home Assistant not configured"}
        return self._check_dict(ping)

    def _check_dict(self, live: bool) -> Dict[str, Any]:
        if live:
            ok, message = self.check_connection()
            self._last_check = {"at": time.time(), "ok": ok, "message": message}
            return {"configured": True, "connected": ok, "message": message}
        if self._last_check and (time.time() - self._last_check["at"]) <= 15:
            return {"configured": True, "connected": self._last_check["ok"],
                    "message": self._last_check["message"]}
        return {"configured": True, "connected": False,
                "message": "Not checked yet — click Test Connection"}

    def call_service(self, domain: str, service: str, entity_id: Optional[str] = None,
                     data: Optional[Dict[str, Any]] = None) -> Tuple[bool, str]:
        if not self.is_configured():
            _notify("OctoPad / Home Assistant", "Home Assistant config not set")
            return False, "Home Assistant not configured"

        payload: Dict[str, Any] = data or {}
        if entity_id:
            payload["entity_id"] = entity_id

        url = f"{self.url}/api/services/{domain}/{service}"
        headers = {"Authorization": f"Bearer {self.token}",
                   "Content-Type": "application/json"}
        try:
            status, body = _http_request(
                "POST", url, headers=headers,
                body=json.dumps(payload).encode("utf-8"), timeout=5.0)
            if status == 200:
                target = entity_id or f"{domain}.{service}"
                _notify("OctoPad / Home Assistant", f"{domain}.{service}: {target}")
                return True, f"Service {domain}.{service} executed successfully"
            message = f"HTTP {status}: {body[:100].decode('utf-8', 'replace')}"
            _notify("OctoPad / Home Assistant", f"Failed ({status})")
            return False, message
        except Exception as exc:  # noqa: BLE001
            _notify("OctoPad / Home Assistant", "Failed to reach Home Assistant")
            return False, f"Connection error: {exc}"


def _notify(title: str, message: str) -> None:
    if notifier is not None:
        try:
            notifier(title, message)
        except Exception:  # noqa: BLE001
            pass


ha_client = HomeAssistantClient()


if __name__ == "__main__":
    print(f"HA Configured: {ha_client.is_configured()} (URL: {ha_client.url})")
    print(ha_client.check_connection())
