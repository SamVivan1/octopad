//! Home Assistant REST client — the core port of
//! `linux-app/home_assistant.py` (and the `ha_webhook` branch of
//! `linux-app/system_actions.py:420-442`).
//!
//! The client pins Python's *endpoints, timeouts, reason strings, files and
//! semantics* (not its urllib internals). Every network request goes through
//! the injected [`HaHttp`] trait and every monotonic timestamp through
//! [`HaClock`], so tests run offline and UI scans share one throttle clock,
//! one memory cache and one disk cache — the module-level `ha_client`
//! singleton of the Python app, as an `Arc`-managed [`HaState`].
//!
//! Storage is byte-compatible with the Python app: `ha_config.json` (keys
//! `url`/`token`, mode 0o600), the read-only `~/.config/home-assistant/env`
//! migration fallback and `ha_entities_cache.json` (`url` + `entities`).

use std::error::Error as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::exec::{ActionOutcome, CommandRunner};

// ---------------------------------------------------------------------------
// Storage / timing constants (home_assistant.py:37-52)
// ---------------------------------------------------------------------------

/// `~/.config/octopad/ha_config.json` (`home_assistant.py:38`), relative to
/// the OctoPad config dir handed to [`HaState::load`].
pub const CONFIG_FILE: &str = "ha_config.json";

/// `~/.config/octopad/ha_entities_cache.json` (`home_assistant.py:39`).
pub const ENTITIES_CACHE_FILE: &str = "ha_entities_cache.json";

// Timeout requests (connect, read) — short so the UI never feels frozen.
//
// Note: Python only ever passes the *read* half of these tuples to
// `urlopen(timeout=…)` (e.g. `home_assistant.py:350-351`), so the requests
// built below use `.1` as the single request timeout, exactly like Python.
//
// TIMEOUT_TEST / TIMEOUT_PING / TIMEOUT_SCAN / TIMEOUT_SCAN_BG.
pub const TIMEOUT_TEST: (f64, f64) = (2.0, 3.0);
/// Background check (`TIMEOUT_PING`); exposed for the status pill command.
pub const TIMEOUT_PING: (f64, f64) = (1.0, 2.0);
/// Forced live scan (`TIMEOUT_SCAN`, manual scan 8 s, spec §2).
pub const TIMEOUT_SCAN: (f64, f64) = (2.0, 8.0);
/// First scan while the cache is still empty (`TIMEOUT_SCAN_BG`).
pub const TIMEOUT_SCAN_BG: (f64, f64) = (1.5, 5.0);
/// Service call timeout (`home_assistant.py:444`).
pub const TIMEOUT_SERVICE: f64 = 5.0;
/// Webhook timeout (`system_actions.py:440`).
pub const TIMEOUT_WEBHOOK: f64 = 4.0;

/// Minimum delay before rescanning live after a scan failed
/// (`SCAN_RETRY_DELAY`); prevents repeated requests against a dead endpoint.
pub const SCAN_RETRY_DELAY: f64 = 30.0;

// ---------------------------------------------------------------------------
// Reason strings, Python-verbatim
// ---------------------------------------------------------------------------

/// `home_assistant.py:360`, verbatim.
pub const AUTH_FAILED_401: &str = "Authentication failed: invalid Token (401 Unauthorized)";

/// `home_assistant.py:345`, verbatim.
pub const NOT_CONFIGURED: &str = "Home Assistant URL or Token not configured";

// Success prefix and failure table for scans / service calls. The table rows
// are composed in `describe_status` because Python composes them with
// f-strings (`Connected to Home Assistant ({msg})`, etc.).

// ---------------------------------------------------------------------------
// Domain tables (Python `INTERACTIVE_SERVICES`, `DEFAULT_SERVICE_OVERRIDES`,
// `DOMAIN_ICONS`, `MDI_ICON_ALIASES`) — exact, in Python's order.
// ---------------------------------------------------------------------------

/// Domains that can actually be controlled from the macropad
/// (`home_assistant.py:56-87`). Sensors etc. are intentionally skipped as
/// non-interactive.
pub const INTERACTIVE_SERVICES: &[(&str, &[&str])] = &[
    ("light", &["toggle", "turn_on", "turn_off"]),
    ("switch", &["toggle", "turn_on", "turn_off"]),
    ("group", &["toggle", "turn_on", "turn_off"]),
    ("input_boolean", &["toggle", "turn_on", "turn_off"]),
    ("fan", &["toggle", "turn_on", "turn_off", "set_percentage"]),
    (
        "cover",
        &[
            "toggle",
            "open_cover",
            "close_cover",
            "stop_cover",
            "set_cover_position",
        ],
    ),
    (
        "climate",
        &["toggle", "set_temperature", "set_hvac_mode", "set_fan_mode"],
    ),
    (
        "humidifier",
        &["toggle", "turn_on", "turn_off", "set_humidity"],
    ),
    ("water_heater", &["turn_on", "turn_off", "set_temperature"]),
    ("lock", &["lock", "unlock"]),
    (
        "vacuum",
        &[
            "start",
            "pause",
            "stop",
            "return_to_base",
            "locate",
            "toggle",
        ],
    ),
    (
        "media_player",
        &[
            "play_pause",
            "media_play",
            "media_pause",
            "media_next_track",
            "media_previous_track",
            "volume_up",
            "volume_down",
            "volume_mute",
            "volume_set",
            "toggle",
            "turn_on",
            "turn_off",
        ],
    ),
    ("remote", &["toggle", "turn_on", "turn_off", "send_command"]),
    ("scene", &["turn_on"]),
    ("script", &["turn_on"]),
    ("automation", &["trigger", "turn_on", "turn_off"]),
    ("button", &["press"]),
    ("input_button", &["press"]),
    ("timer", &["start", "pause", "cancel"]),
    ("number", &["set_value"]),
    ("input_number", &["set_value"]),
    ("select", &["select_option"]),
    ("input_select", &["select_option"]),
    ("text", &["set_value"]),
    ("siren", &["turn_on", "turn_off", "toggle"]),
    (
        "alarm_control_panel",
        &["disarm", "arm_home", "arm_away", "arm_night"],
    ),
    ("update", &["install"]),
    ("tts", &["speak"]),
];

/// Most sensible service per domain for a freshly picked entity
/// (`home_assistant.py:142-155`, pinned by the spec §3).
pub const DEFAULT_SERVICE_OVERRIDES: &[(&str, &str)] = &[
    ("automation", "trigger"),
    ("button", "press"),
    ("input_button", "press"),
    ("scene", "turn_on"),
    ("script", "turn_on"),
    ("media_player", "play_pause"),
    ("cover", "toggle"),
    ("lock", "lock"),
    ("vacuum", "start"),
    ("timer", "start"),
    ("update", "install"),
    ("alarm_control_panel", "disarm"),
];

/// Built-in icon fallback per domain (`home_assistant.py:89-100`).
pub const DOMAIN_ICONS: &[(&str, &str)] = &[
    ("light", "lightbulb"),
    ("switch", "toggle-right"),
    ("group", "package"),
    ("input_boolean", "toggle-left"),
    ("fan", "fan"),
    ("cover", "chevrons-up-down"),
    ("climate", "thermometer"),
    ("humidifier", "droplets"),
    ("water_heater", "droplet"),
    ("lock", "lock"),
    ("vacuum", "bot"),
    ("media_player", "music"),
    ("remote", "radio"),
    ("scene", "clapperboard"),
    ("script", "scroll-text"),
    ("automation", "settings"),
    ("button", "circle-dot"),
    ("input_button", "circle-dot"),
    ("timer", "timer"),
    ("number", "hash"),
    ("input_number", "hash"),
    ("select", "list"),
    ("input_select", "list"),
    ("text", "type"),
    ("siren", "siren"),
    ("alarm_control_panel", "shield"),
    ("update", "package"),
    ("tts", "volume-2"),
];

/// `home_assistant.py:101` — the icon when the domain is unknown to the map.
pub const DEFAULT_ICON: &str = "package";

/// Home Assistant `mdi:xxx` icons (after the `mdi:` prefix, `_` → `-`) mapped
/// to internal Lucide names (`home_assistant.py:105-137`).
pub const MDI_ICON_ALIASES: &[(&str, &str)] = &[
    ("lightbulb", "lightbulb"),
    ("lightbulb-outline", "lightbulb"),
    ("lamp", "lightbulb"),
    ("ceiling-light", "lightbulb"),
    ("wall-sconce", "lightbulb"),
    ("power-plug", "plug"),
    ("power-socket", "plug"),
    ("power-socket-eu", "plug"),
    ("lightswitch", "toggle-right"),
    ("lightbulb-on", "lightbulb"),
    ("curtain", "chevrons-up-down"),
    ("blinds", "chevrons-up-down"),
    ("blinds-horizontal", "chevrons-up-down"),
    ("garage", "chevrons-up-down"),
    ("window-closed", "chevrons-up-down"),
    ("window-open", "chevrons-up-down"),
    ("thermometer", "thermometer"),
    ("thermometer-water", "thermometer"),
    ("heating-coil", "thermometer"),
    ("humidity", "droplets"),
    ("water-pump", "droplet"),
    ("shower", "droplet"),
    ("shower-head", "droplet"),
    ("faucet", "droplet"),
    ("water", "droplet"),
    ("hot-tub", "droplet"),
    ("lock", "lock"),
    ("lock-open", "lock"),
    ("lock-pattern", "lock"),
    ("robot-vacuum", "bot"),
    ("robot", "bot"),
    ("robot-industrial", "bot"),
    ("speaker", "music"),
    ("music", "music"),
    ("music-note", "music"),
    ("television", "monitor"),
    ("television-classic", "monitor"),
    ("projector", "monitor"),
    ("remote", "radio"),
    ("remote-tv", "radio"),
    ("movie", "clapperboard"),
    ("movie-open", "clapperboard"),
    ("film", "clapperboard"),
    ("script-text", "scroll-text"),
    ("script", "scroll-text"),
    ("note-text", "scroll-text"),
    ("cog", "settings"),
    ("cogs", "settings"),
    ("cog-outline", "settings"),
    ("timer", "timer"),
    ("timer-outline", "timer"),
    ("stopwatch", "timer"),
    ("counter", "hash"),
    ("sort-numeric", "hash"),
    ("numeric", "hash"),
    ("format-list-bulleted", "list"),
    ("format-list-bulleted-square", "list"),
    ("format-list-checks", "list"),
    ("playlist", "list"),
    ("text", "type"),
    ("form-textbox", "type"),
    ("format-letter-case", "type"),
    ("bell-ring", "siren"),
    ("alarm", "siren"),
    ("bell", "siren"),
    ("shield", "shield"),
    ("shield-home", "shield"),
    ("shield-check", "shield"),
    ("package", "package"),
    ("package-variant", "package"),
    ("update", "package"),
    ("volume-high", "volume-2"),
    ("volume", "volume-2"),
    ("volume-medium", "volume-2"),
    ("fan", "fan"),
    ("ceiling-fan", "fan"),
    ("fan-off", "fan"),
    ("home", "house"),
    ("home-outline", "house"),
    ("house", "house"),
    ("circle", "circle-dot"),
    ("circle-outline", "circle-dot"),
];

fn interactive_services(domain: &str) -> Option<&'static [&'static str]> {
    INTERACTIVE_SERVICES
        .iter()
        .find(|(name, _)| *name == domain)
        .map(|(_, services)| *services)
}

fn default_service_override(domain: &str) -> Option<&'static str> {
    DEFAULT_SERVICE_OVERRIDES
        .iter()
        .find(|(name, _)| *name == domain)
        .map(|(_, service)| *service)
}

fn domain_icon(domain: &str) -> &'static str {
    DOMAIN_ICONS
        .iter()
        .find(|(name, _)| *name == domain)
        .map(|(_, icon)| *icon)
        .unwrap_or(DEFAULT_ICON)
}

/// `resolve_icon` (`home_assistant.py:157-166`): map an mdi/Lucide icon to
/// the internal icon name, falling back to the domain icon.
fn resolve_icon(raw: Option<&str>, domain: &str) -> String {
    if let Some(raw) = raw {
        let name = raw.trim();
        if !name.is_empty() {
            let stem = name.strip_prefix("mdi:").unwrap_or(name);
            let normalized = stem.to_lowercase().replace('_', "-");
            if let Some((_, mapped)) = MDI_ICON_ALIASES
                .iter()
                .find(|(alias, _)| *alias == normalized.as_str())
            {
                return (*mapped).to_string();
            }
        }
    }
    domain_icon(domain).to_string()
}

/// Python's `.rstrip('/')` (trailing slashes are stripped everywhere).
fn rstrip_slashes(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

// ---------------------------------------------------------------------------
// Request / response model and the injection seams
// ---------------------------------------------------------------------------

/// One blocking HTTP request, built by the client and executed by the
/// injected [`HaHttp`] transport (Python's `_http_request`, `:18-34`).
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    /// `GET` / `POST`.
    pub method: &'static str,
    /// Full URL (endpoint already appended by the client).
    pub url: String,
    /// E.g. `Authorization: Bearer …`, `Content-Type: application/json`.
    pub headers: Vec<(String, String)>,
    /// Serialized JSON body, when the request has one.
    pub body: Option<Vec<u8>>,
    /// The single socket timeout Python passes to `urlopen` (the *read* half
    /// of its timeout tuples; the connect half is dead config in Python).
    pub timeout: f64,
}

/// An HTTP response, returned for any status (like Python's `_http_request`,
/// which returns HTTP *error* responses normally and only raises on network
/// failures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// `socket.timeout` vs any other network failure — the two Python `except`
/// legs of `check_connection` (`:363-366`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpErrorKind {
    /// `socket.timeout` / `TimeoutError`.
    Timeout,
    /// Any other `Exception`.
    Unreachable,
}

/// A network-level failure (`urllib` raising instead of returning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub kind: HttpErrorKind,
    /// The `str(e)` Python would have shown.
    pub message: String,
}

/// The HTTP seam. Tests inject a scripted double; production uses
/// [`UreqTransport`].
pub trait HaHttp: Send + Sync {
    fn request(&self, req: HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// The monotonic clock seam (`time.monotonic()`), used for the scan retry
/// guard. Tests inject a stopped clock.
pub trait HaClock: Send + Sync {
    fn secs(&self) -> f64;
}

/// Production clock: `Instant` since construction.
#[derive(Debug, Clone)]
pub struct MonotonicClock {
    started: Instant,
}

impl Default for MonotonicClock {
    fn default() -> MonotonicClock {
        MonotonicClock::new()
    }
}

impl MonotonicClock {
    pub fn new() -> MonotonicClock {
        MonotonicClock {
            started: Instant::now(),
        }
    }
}

impl HaClock for MonotonicClock {
    fn secs(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
}

/// Production transport on top of `ureq` (`std::process`-style blocking, no
/// token in URLs). Mirrors Python's `_http_request`: HTTP error statuses come
/// back as normal responses; only network failures produce [`HttpError`].
#[derive(Debug, Clone)]
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> UreqTransport {
        UreqTransport::new()
    }
}

impl UreqTransport {
    pub fn new() -> UreqTransport {
        UreqTransport {
            agent: ureq::AgentBuilder::new().build(),
        }
    }
}

impl HaHttp for UreqTransport {
    fn request(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        let mut builder = self.agent.request(req.method, &req.url);
        for (name, value) in &req.headers {
            builder = builder.set(name, value);
        }
        builder = builder.timeout(Duration::from_secs_f64(req.timeout.max(0.0)));
        let sent = match &req.body {
            Some(bytes) => builder.send_bytes(bytes),
            None => builder.call(),
        };
        match sent {
            Ok(response) => Ok(HttpResponse {
                status: response.status(),
                body: read_body(response.into_reader()),
            }),
            // ureq surfaces 4xx/5xx as this error carrying the response.
            Err(ureq::Error::Status(status, response)) => Ok(HttpResponse {
                status,
                body: read_body(response.into_reader()),
            }),
            Err(ureq::Error::Transport(transport)) => Err(classify_ureq(transport)),
        }
    }
}

fn read_body(mut reader: impl Read) -> Vec<u8> {
    let mut body = Vec::new();
    // A truncated body after a valid status is still a response.
    let _ = reader.read_to_end(&mut body);
    body
}

fn classify_ureq(transport: ureq::Transport) -> HttpError {
    // ureq 2.12 has no `TimedOut` kind: timeouts surface as kind `Io` with a
    // `std::io::Error` timed-out source, so detect through the downcast.
    let timed_out = transport
        .source()
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
        || transport.to_string().contains("timed out");
    HttpError {
        kind: if timed_out {
            HttpErrorKind::Timeout
        } else {
            HttpErrorKind::Unreachable
        },
        message: transport.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Public result types
// ---------------------------------------------------------------------------

/// Result of the Test Connection button (`check_connection`'s `(bool, str)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub ok: bool,
    pub message: String,
}

/// One entity row as the UI entity picker consumes it — Python's decorated
/// entry (`decorate_entities`, `:200-221`) carrying the scanned fields plus
/// `services` and `default_service`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySummary {
    pub entity_id: String,
    pub domain: String,
    pub friendly_name: String,
    pub state: Option<String>,
    pub icon: Option<String>,
    pub services: Vec<String>,
    pub default_service: String,
}

// ---------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------

/// Mutable client state (one `Mutex` guards credentials-plus-caches; the lock
/// is never held across HTTP reads — see `get_entities`).
#[derive(Debug, Default)]
struct HaInner {
    url: String,
    token: String,
    cached_entities: Vec<EntitySummary>,
    /// Monotonic seconds of the last scan attempt (`_last_scan_at`, 0.0 when
    /// never scanned, modelled as `None`).
    last_scan_at: Option<f64>,
    /// `_last_scan_ok`: did the last live attempt succeed?
    last_scan_ok: bool,
    /// `_disk_loaded`: lazy disk-cache read done.
    disk_loaded: bool,
}

/// The Home Assistant client — managed singleton, `Arc`-safe. One throttle
/// clock, one memory cache, one disk cache, exactly the Python module-level
/// `ha_client`.
pub struct HaState {
    http: Box<dyn HaHttp>,
    clock: Box<dyn HaClock>,
    /// OctoPad config dir (`~/.config/octopad`), clones [`CONFIG_FILE`] /
    /// [`ENTITIES_CACHE_FILE`] into it.
    dir: PathBuf,
    inner: Mutex<HaInner>,
}

impl HaState {
    /// Production constructor: credentials from `{state_dir}/ha_config.json`
    /// with the `~/.config/home-assistant/env` fallback, a real `ureq`
    /// transport and a real monotonic clock. Never touches the network.
    pub fn load(state_dir: &Path) -> HaState {
        let env_config = dirs::home_dir()
            .unwrap_or_default()
            .join(".config")
            .join("home-assistant")
            .join("env");
        HaState::load_with_seams(
            state_dir,
            &env_config,
            Box::new(UreqTransport::new()),
            Box::new(MonotonicClock::new()),
        )
    }

    /// Paths + seams constructor: tests pass a temp state dir, an explicit
    /// env-file path (never `$HOME`), a scripted transport and a stopped
    /// clock.
    pub fn load_with_seams(
        state_dir: &Path,
        env_config: &Path,
        http: Box<dyn HaHttp>,
        clock: Box<dyn HaClock>,
    ) -> HaState {
        let mut inner = HaInner::default();
        load_credentials(state_dir, env_config, &mut inner.url, &mut inner.token);
        HaState {
            http,
            clock,
            dir: state_dir.to_path_buf(),
            inner: Mutex::new(inner),
        }
    }

    /// `is_configured` (`home_assistant.py:304-305`).
    pub fn is_configured(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        is_configured_pair(&inner.url, &inner.token)
    }

    /// `check_connection` (`home_assistant.py:338-366`), timeout
    /// [`TIMEOUT_TEST`]. Empty/missing arguments mean "use the saved value"
    /// (Python's `url or self.url or ""`).
    pub fn check_connection(&self, url: Option<String>, token: Option<String>) -> CheckResult {
        let (saved_url, saved_token) = {
            let inner = self.inner.lock().unwrap();
            (inner.url.clone(), inner.token.clone())
        };
        let target_url = rstrip_slashes(arg_or_saved(url.as_deref(), &saved_url));
        let target_token = arg_or_saved(token.as_deref(), &saved_token)
            .trim()
            .to_string();

        if target_url.is_empty() || target_token.is_empty() {
            return CheckResult {
                ok: false,
                message: NOT_CONFIGURED.to_string(),
            };
        }

        let request = HttpRequest {
            method: "GET",
            url: format!("{target_url}/api/"),
            headers: bearer_header(&target_token),
            body: None,
            timeout: TIMEOUT_TEST.1,
        };
        match self.http.request(request) {
            Ok(response) => {
                let (ok, message) = describe_status(&response);
                CheckResult { ok, message }
            }
            Err(HttpError {
                kind: HttpErrorKind::Timeout,
                ..
            }) => CheckResult {
                ok: false,
                message: format!("Connection timeout to {target_url} (check the LAN)"),
            },
            Err(HttpError { message, .. }) => CheckResult {
                ok: false,
                message: format!("Failed to reach Home Assistant: {message}"),
            },
        }
    }

    /// `save_config` (`home_assistant.py:278-302`): strips url/token, drops
    /// the entity cache *iff the URL changed* (so saving a token never wipes
    /// a fresh scan, while a URL switch can never show the other HA's
    /// entities), writes `ha_config.json` (0o600 best-effort) and returns
    /// false on IO failure instead of panicking.
    pub fn save(&self, url: String, token: String) -> bool {
        let url = rstrip_slashes(&url);
        let token = token.trim().to_string();
        let mut inner = self.inner.lock().unwrap();
        if inner.url != url {
            inner.cached_entities.clear();
            inner.last_scan_at = None;
            inner.last_scan_ok = false;
            clear_disk_entities(&self.dir);
        }
        inner.url = url.clone();
        inner.token = token.clone();
        inner.disk_loaded = true;
        write_credentials(&self.dir, &url, &token).is_ok()
    }

    /// `get_entities` (`home_assistant.py:368-423`): cache-first scan of the
    /// interactive entities, sorted `(domain, friendly_name.lower())`, disk
    /// cache at the Python filename, 30 s failed-scan retry guard.
    pub fn get_entities(&self, force_refresh: bool) -> Vec<EntitySummary> {
        // Decision phase: load the disk cache once, answer from memory, arm
        // the retry guard — all before any network I/O, lock never held
        // across the socket.
        let plan = self.plan_scan(force_refresh);
        match plan {
            ScanPlan::Return(cached) => cached,
            ScanPlan::Scan {
                url,
                token,
                timeout,
            } => {
                let stripped = rstrip_slashes(&url);
                let request = HttpRequest {
                    method: "GET",
                    url: format!("{stripped}/api/states"),
                    headers: bearer_header(token.trim()),
                    body: None,
                    timeout,
                };
                match self.http.request(request) {
                    Ok(response) if response.status == 200 => {
                        if let Some(entities) = parse_states(&response.body) {
                            let mut inner = self.inner.lock().unwrap();
                            inner.cached_entities = entities.clone();
                            inner.last_scan_ok = true;
                            save_disk_entities(&self.dir, &entities, &stripped);
                            return entities;
                        }
                        // Malformed body: Python's `except` around the parse
                        // (`:393-417`) marks the scan as failed.
                        self.record_scan_failure();
                    }
                    Ok(_) | Err(_) => self.record_scan_failure(),
                }
                // HA unreachable / not configured → the last scan from memory.
                let inner = self.inner.lock().unwrap();
                inner.cached_entities.clone()
            }
        }
    }

    /// `call_service` (`home_assistant.py:425-455`): POST the service, 5 s,
    /// `data-or-{} + entity_id`. Reason strings follow the spec §2 table
    /// (Python's 200 string verbatim; failures in the check-connection table
    /// style, timeout → the LAN string). **Desktop notifications are
    /// failure-only** (owner decision 2026-10-19) and always go through the
    /// runner's `spawn_detached` — the success path never touches it.
    pub fn call_service(
        &self,
        domain: &str,
        service: &str,
        entity_id: Option<&str>,
        data: &Value,
        runner: &dyn CommandRunner,
    ) -> ActionOutcome {
        let (saved_url, saved_token) = {
            let inner = self.inner.lock().unwrap();
            (inner.url.clone(), inner.token.clone())
        };
        let url = rstrip_slashes(&saved_url);
        let token = saved_token.trim().to_string();
        if url.is_empty() || token.is_empty() {
            self.notify(runner, "Home Assistant config not set");
            return ActionOutcome::fail("Home Assistant not configured");
        }

        // Python: `payload = data or {}`, `payload["entity_id"] = entity_id`
        // when truthy. A non-object payload (which Python would crash on) is
        // treated as empty — a config hand-edit never panics the chain.
        let mut payload = match data {
            Value::Object(map) if !map.is_empty() => map.clone(),
            _ => Map::new(),
        };
        if let Some(entity_id) = entity_id.filter(|id| !id.is_empty()) {
            payload.insert(
                "entity_id".to_string(),
                Value::String(entity_id.to_string()),
            );
        }

        let request = HttpRequest {
            method: "POST",
            url: format!("{url}/api/services/{domain}/{service}"),
            headers: {
                let mut headers = bearer_header(&token);
                headers.push(("Content-Type".to_string(), "application/json".to_string()));
                headers
            },
            body: Some(
                serde_json::to_vec(&Value::Object(payload)).unwrap_or_else(|_| b"{}".to_vec()),
            ),
            timeout: TIMEOUT_SERVICE,
        };
        match self.http.request(request) {
            Ok(response) if response.status == 200 => {
                ActionOutcome::ok(format!("Service {domain}.{service} executed successfully"))
            }
            Ok(response) => {
                let status = response.status;
                self.notify(runner, &format!("Failed ({status})"));
                let (_, message) = describe_status(&response);
                ActionOutcome::fail(message)
            }
            Err(HttpError {
                kind: HttpErrorKind::Timeout,
                ..
            }) => {
                self.notify(runner, "Failed to reach Home Assistant");
                ActionOutcome::fail(format!("Connection timeout to {url} (check the LAN)"))
            }
            Err(HttpError { message, .. }) => {
                self.notify(runner, "Failed to reach Home Assistant");
                ActionOutcome::fail(format!("Failed to reach Home Assistant: {message}"))
            }
        }
    }

    /// The `ha_webhook` branch (`system_actions.py:427-442`): POST `data`
    /// (or `{}`) to the URL with `Content-Type: application/json`, 4 s.
    /// Non-2xx statuses never come back from `urlopen` as values (it raises),
    /// so the honest port fails them in the module's table style; the two
    /// Python-pinned strings are the empty URL and the success line.
    pub fn send_webhook(&self, url: &str, data: &Value) -> ActionOutcome {
        let url = url.trim();
        if url.is_empty() {
            return ActionOutcome::fail("Home Assistant Webhook URL is empty");
        }
        let request = HttpRequest {
            method: "POST",
            url: url.to_string(),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: Some(serde_json::to_vec(data).unwrap_or_else(|_| b"{}".to_vec())),
            timeout: TIMEOUT_WEBHOOK,
        };
        match self.http.request(request) {
            Ok(response) if (200..300).contains(&response.status) => ActionOutcome::ok(format!(
                "Webhook sent to Home Assistant (HTTP {})",
                response.status
            )),
            Ok(response) => {
                ActionOutcome::fail(format!("Server responded with HTTP {}", response.status))
            }
            Err(HttpError {
                kind: HttpErrorKind::Timeout,
                ..
            }) => ActionOutcome::fail(format!("Connection timeout to {url} (check the LAN)")),
            Err(HttpError { message, .. }) => {
                ActionOutcome::fail(format!("Failed to reach Home Assistant: {message}"))
            }
        }
    }

    /// Failure-only desktop notification, Python's `_notify_desktop`
    /// (`:457-464`): `notify-send "OctoPad / Home Assistant" {message}`.
    /// Detached, never awaited, never blocking.
    fn notify(&self, runner: &dyn CommandRunner, message: &str) {
        runner.spawn_detached("notify-send", &["OctoPad / Home Assistant", message]);
    }

    /// One lock-around-filesystem phase of `get_entities`: lazy disk load,
    /// warm-cache return, retry guard, configured gate.
    fn plan_scan(&self, force_refresh: bool) -> ScanPlan {
        let mut inner = self.inner.lock().unwrap();
        if !inner.disk_loaded {
            inner.disk_loaded = true;
            let current_url = rstrip_slashes(&inner.url);
            if let Some(entities) = load_disk_entities(&self.dir, &current_url) {
                inner.cached_entities = entities;
            }
        }
        if !inner.cached_entities.is_empty() && !force_refresh {
            return ScanPlan::Return(inner.cached_entities.clone());
        }
        // The last live scan failed → do not retry within SCAN_RETRY_DELAY
        // (`:379-383`), so the endpoint stays instant when HA is unreachable.
        let now = self.clock.secs();
        if !force_refresh
            && inner.last_scan_at.is_some()
            && !inner.last_scan_ok
            && now - inner.last_scan_at.unwrap() < SCAN_RETRY_DELAY
        {
            return ScanPlan::Return(inner.cached_entities.clone());
        }
        if !is_configured_pair(&inner.url, &inner.token) {
            return ScanPlan::Return(inner.cached_entities.clone());
        }
        // Timeout: forced = TIMEOUT_SCAN, background = TIMEOUT_SCAN_BG
        // (`:386`; a non-forced scan only happens while the cache is empty or
        // after the guard expired — the warm cache returned already).
        let timeout = if force_refresh {
            TIMEOUT_SCAN.1
        } else {
            TIMEOUT_SCAN_BG.1
        };
        inner.last_scan_at = Some(now);
        ScanPlan::Scan {
            url: inner.url.clone(),
            token: inner.token.clone(),
            timeout,
        }
    }

    /// `self._last_scan_ok = False` (`:419`).
    fn record_scan_failure(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.last_scan_ok = false;
    }
}

enum ScanPlan {
    Return(Vec<EntitySummary>),
    Scan {
        url: String,
        token: String,
        timeout: f64,
    },
}

fn is_configured_pair(url: &str, token: &str) -> bool {
    !url.is_empty() && !token.trim().is_empty()
}

/// Python's `(url or self.url or "")`: an empty or missing draft field means
/// "use the saved value".
fn arg_or_saved<'a>(argument: Option<&'a str>, saved: &'a str) -> &'a str {
    match argument {
        Some(value) if !value.is_empty() => value,
        _ => saved,
    }
}

/// `{"Authorization": f"Bearer {token}"}` (`:348`).
fn bearer_header(token: &str) -> Vec<(String, String)> {
    vec![("Authorization".to_string(), format!("Bearer {token}"))]
}

/// The status → `(ok, message)` table of `check_connection` (`:350-362`):
/// 200 → the body's message, 401 → the auth failure, anything else →
/// `Server responded with HTTP {n}`, with the error body's JSON `message`
/// appended when present (spec §2 / task brief pin).
fn describe_status(response: &HttpResponse) -> (bool, String) {
    if response.status == 200 {
        (
            true,
            format!("Connected to Home Assistant ({})", api_message(response)),
        )
    } else if response.status == 401 {
        (false, AUTH_FAILED_401.to_string())
    } else {
        (false, server_error_message(response))
    }
}

/// 200 body: `data.get("message", "API running")` (`:352-357`).
fn api_message(response: &HttpResponse) -> String {
    let value: Value = match serde_json::from_slice(&response.body) {
        Ok(value) => value,
        Err(_) => return "API running".to_string(),
    };
    value["message"]
        .as_str()
        .unwrap_or("API running")
        .to_string()
}

/// Other statuses: `Server responded with HTTP {n}`, the HTTP error body's
/// `message` appended when a JSON `message` exists.
fn server_error_message(response: &HttpResponse) -> String {
    let base = format!("Server responded with HTTP {}", response.status);
    match serde_json::from_slice::<Value>(&response.body) {
        Ok(value) => match value["message"].as_str() {
            Some(message) if !message.is_empty() => format!("{base}: {message}"),
            _ => base,
        },
        Err(_) => base,
    }
}

// ---------------------------------------------------------------------------
// Credentials (`load_config` / `save_config`, `:235-302`)
// ---------------------------------------------------------------------------

/// `load_config` (`:235-266`): `{dir}/ha_config.json` first (both fields must
/// be present to short-circuit), else the read-only env migration file.
fn load_credentials(dir: &Path, env_config: &Path, url: &mut String, token: &mut String) {
    if let Ok(text) = fs::read_to_string(dir.join(CONFIG_FILE)) {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            *url = value["url"].as_str().unwrap_or("").to_string();
            *token = value["token"].as_str().unwrap_or("").to_string();
            if !url.is_empty() && !token.is_empty() {
                return;
            }
        }
    }
    if let Ok(text) = fs::read_to_string(env_config) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("HA_URL=") {
                *url = strip_quotes(rest);
            } else if let Some(rest) = line.strip_prefix("HA_TOKEN=") {
                *token = strip_quotes(rest);
            }
        }
    }
}

/// Python's `.strip("\"'")`: quote characters only, from both ends.
fn strip_quotes(value: &str) -> String {
    value.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// The file write half of `save_config` (`:290-298`): create the dir, write
/// `{"url", "token"}` with indent 2, chmod 0o600 best-effort (Python swallows
/// the `OSError`).
fn write_credentials(dir: &Path, url: &str, token: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let body = serde_json::to_string_pretty(&json!({"url": url, "token": token}))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    fs::write(dir.join(CONFIG_FILE), body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(
            dir.join(CONFIG_FILE),
            std::fs::Permissions::from_mode(0o600),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Entity disk cache (`:169-197`, `:223-232`)
// ---------------------------------------------------------------------------

/// `_load_disk_cache` + the URL guard of `_load_cached_entities` (`:272`): a
/// cache written for another HA instance is ignored. `None` → keep whatever
/// the memory holds.
fn load_disk_entities(dir: &Path, current_url: &str) -> Option<Vec<EntitySummary>> {
    let text = fs::read_to_string(dir.join(ENTITIES_CACHE_FILE)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let cache_url = value["url"].as_str().unwrap_or("");
    if !cache_url.is_empty() && cache_url != current_url {
        return None;
    }
    let mut entities = Vec::new();
    for entry in value["entities"].as_array().unwrap_or(&Vec::new()) {
        if let Ok(cached) = serde_json::from_value::<CachedEntity>(entry.clone()) {
            if let Some(summary) = cached.decorate() {
                entities.push(summary);
            }
        }
    }
    if entities.is_empty() {
        return None; // Python: only non-empty scans replace the cache.
    }
    Some(entities)
}

/// `_save_disk_entities` (`:181-189`): `{"saved_at", "url", "entities"}`,
/// decorated rows, indent 2. Best-effort: a failed write loses freshness, not
/// correctness.
fn save_disk_entities(dir: &Path, entities: &[EntitySummary], url: &str) {
    let value = json!({
        "saved_at": wall_seconds(),
        "url": url,
        "entities": entities,
    });
    if fs::create_dir_all(dir).is_ok() {
        let _ = fs::write(
            dir.join(ENTITIES_CACHE_FILE),
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string()),
        );
    }
}

/// `_clear_disk_entities` (`:192-197`): best-effort removal.
fn clear_disk_entities(dir: &Path) {
    let file = dir.join(ENTITIES_CACHE_FILE);
    if file.exists() {
        let _ = fs::remove_file(&file);
    }
}

fn wall_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Decorate (`:200-221`)
// ---------------------------------------------------------------------------

/// One cached (decorated) row from `ha_entities_cache.json`. The services
/// list is never trusted from the cache — Python re-decorates and always
/// overwrites `services` from `INTERACTIVE_SERVICES`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CachedEntity {
    entity_id: String,
    domain: String,
    friendly_name: String,
    state: Option<String>,
    icon: Option<String>,
    default_service: Option<String>,
}

impl CachedEntity {
    /// `decorate_entities` for one row: filter to interactive domains, fill
    /// services/default_service/icon. `None` → the row is not interactive.
    fn decorate(self) -> Option<EntitySummary> {
        let domain = if !self.domain.is_empty() {
            self.domain
        } else {
            match self.entity_id.split_once('.') {
                Some((domain, _)) => domain.to_string(),
                None => String::new(),
            }
        };
        let services = interactive_services(&domain)?
            .iter()
            .map(|service| (*service).to_string())
            .collect::<Vec<String>>();
        let default_service = self
            .default_service
            .filter(|service| !service.is_empty())
            .unwrap_or_else(|| default_summary_service(&domain, &services));
        let friendly_name = if self.friendly_name.is_empty() {
            self.entity_id.clone()
        } else {
            self.friendly_name
        };
        Some(EntitySummary {
            entity_id: self.entity_id,
            icon: Some(resolve_icon(self.icon.as_deref(), &domain)),
            domain,
            friendly_name,
            state: self.state,
            services,
            default_service,
        })
    }
}

/// `default_service` ladder (`:214-218`): the override table, else
/// `toggle` when offered, else the domain's first service.
fn default_summary_service(domain: &str, services: &[String]) -> String {
    if let Some(overridden) = default_service_override(domain) {
        return overridden.to_string();
    }
    if services.iter().any(|service| service == "toggle") {
        return "toggle".to_string();
    }
    services.first().cloned().unwrap_or_default()
}

/// The live `/api/states` rows → decoded, filtered, sorted, decorated
/// (`:393-415`).
fn parse_states(body: &[u8]) -> Option<Vec<EntitySummary>> {
    let states: Vec<Value> = serde_json::from_slice(body).ok()?;
    let mut parsed: Vec<(String, CachedEntity)> = Vec::new();
    for state in &states {
        let entity_id = state["entity_id"].as_str().unwrap_or("").to_string();
        // `entity_id.split(".")[0] if "." in entity_id else "unknown"`
        // (`:398`): a non-dotted id can never be interactive.
        let domain = match entity_id.split_once('.') {
            Some((domain, _)) => domain.to_string(),
            None => "unknown".to_string(),
        };
        let attributes = &state["attributes"];
        parsed.push((
            domain,
            CachedEntity {
                entity_id,
                domain: String::new(), // decorate re-derives from the prefix
                friendly_name: attributes["friendly_name"]
                    .as_str()
                    .unwrap_or(state["entity_id"].as_str().unwrap_or(""))
                    .to_string(),
                state: state["state"].as_str().map(str::to_string),
                icon: attributes["icon"].as_str().map(str::to_string),
                default_service: None,
            },
        ));
    }
    let mut entities: Vec<EntitySummary> = parsed
        .into_iter()
        .filter_map(|(_, row)| row.decorate())
        .collect();
    // `(x["domain"], x["friendly_name"].lower())` (`:410`).
    entities.sort_by(|a, b| {
        (a.domain.as_str(), a.friendly_name.to_lowercase().as_str())
            .cmp(&(b.domain.as_str(), b.friendly_name.to_lowercase().as_str()))
    });
    Some(entities)
}
