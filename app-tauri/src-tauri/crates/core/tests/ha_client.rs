//! Task 4 (M3.2): the Home Assistant REST client core.
//!
//! Source of truth: `linux-app/home_assistant.py` (client, lines quoted per
//! test) and `linux-app/system_actions.py:420-442` (`ha_webhook`). Every
//! reason string is pinned Python-verbatim.
//!
//! Nothing touches the network or `$HOME`: HTTP goes through an injected
//! [`HaHttp`] double, the clock through [`HaClock`], paths through
//! `load_with_seams` into a `tempfile` directory.

use std::path::Path;
use std::sync::{Arc, Mutex};

use octopad_core::exec::{ActionOutcome, CommandCall, RecordingRunner};
use octopad_core::ha::{
    HaClock, HaHttp, HaState, HttpError, HttpErrorKind, HttpRequest, HttpResponse, TIMEOUT_SCAN,
    TIMEOUT_SCAN_BG, TIMEOUT_TEST,
};
use serde_json::{json, Value};
use std::collections::VecDeque;

const URL_A: &str = "https://ha.local";
const TOKEN_A: &str = "token-a";

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

/// One request as the client handed it to the transport.
#[derive(Debug, Clone)]
struct RecordedReq {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout: f64,
}

impl RecordedReq {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// A scripted HTTP transport: records every request, replays outcomes in
/// order, and panics on an unexpected extra request so a test flubs loudly.
#[derive(Clone)]
struct FakeHttp {
    state: Arc<Mutex<FakeHttpState>>,
}

#[derive(Default)]
struct FakeHttpState {
    requests: Vec<RecordedReq>,
    script: VecDeque<Result<HttpResponse, HttpError>>,
}

/// `Ok((status, body_json))` or `Err(...)` from the client's point of view.
type Scripted = Result<(u16, Vec<u8>), HttpError>;

impl FakeHttp {
    fn scripted(items: Vec<Scripted>) -> FakeHttp {
        FakeHttp {
            state: Arc::new(Mutex::new(FakeHttpState {
                requests: Vec::new(),
                script: items
                    .into_iter()
                    .map(|item| item.map(|(status, body)| HttpResponse { status, body }))
                    .collect(),
            })),
        }
    }

    fn requests(&self) -> Vec<RecordedReq> {
        self.state.lock().unwrap().requests.clone()
    }

    fn count(&self) -> usize {
        self.state.lock().unwrap().requests.len()
    }

    /// Replays a new script into a transport that has already been used.
    fn rearm(&self, items: Vec<Scripted>) {
        let mut state = self.state.lock().unwrap();
        state.script = items
            .into_iter()
            .map(|item| item.map(|(status, body)| HttpResponse { status, body }))
            .collect();
    }
}

impl HaHttp for FakeHttp {
    fn request(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        let mut state = self.state.lock().unwrap();
        state.requests.push(RecordedReq {
            method: req.method.to_string(),
            url: req.url.clone(),
            headers: req.headers.clone(),
            body: req
                .body
                .as_deref()
                .map(String::from_utf8_lossy)
                .map(String::from),
            timeout: req.timeout,
        });
        let scripted = state
            .script
            .pop_front()
            .expect("FakeHttp: request without a scripted response");
        scripted
    }
}

/// A stopped clock the test moves by hand (`time.monotonic()`).
#[derive(Clone)]
struct FakeClock(Arc<Mutex<f64>>);

impl FakeClock {
    fn new(secs: f64) -> FakeClock {
        FakeClock(Arc::new(Mutex::new(secs)))
    }

    fn set(&self, secs: f64) {
        *self.0.lock().unwrap() = secs;
    }
}

impl HaClock for FakeClock {
    fn secs(&self) -> f64 {
        *self.0.lock().unwrap()
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// `ha_config.json`, key-for-key like the Python app writes it
/// (`home_assistant.py:292-293`).
fn write_config(dir: &Path, url: &str, token: &str) {
    let file = dir.join("ha_config.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&serde_json::json!({"url": url, "token": token})).unwrap(),
    )
    .unwrap();
}

fn read_config(dir: &Path) -> Value {
    let text = std::fs::read_to_string(dir.join("ha_config.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

struct Fixture {
    dir: tempfile::TempDir,
    env: tempfile::TempDir,
    http: FakeHttp,
    clock: FakeClock,
    state: HaState,
}

/// A configured client with one scripted HTTP outcome per call.
fn fixture(script: Vec<Scripted>) -> Fixture {
    fixture_with_config(script, URL_A, TOKEN_A)
}

fn fixture_with_config(script: Vec<Scripted>, url: &str, token: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), url, token);
    let env = tempfile::tempdir().unwrap();
    let http = FakeHttp::scripted(script);
    let clock = FakeClock::new(100.0);
    let state = HaState::load_with_seams(
        dir.path(),
        &env.path().join("env"),
        Box::new(http.clone()),
        Box::new(clock.clone()),
    );
    Fixture {
        dir,
        env,
        http,
        clock,
        state,
    }
}

fn status(status: u16, body: &Value) -> Scripted {
    Ok((status, serde_json::to_vec(body).unwrap()))
}

fn raw_status(status: u16, body: &str) -> Scripted {
    Ok((status, body.as_bytes().to_vec()))
}

fn timeout_err() -> Scripted {
    Err(HttpError {
        kind: HttpErrorKind::Timeout,
        message: "timed out reading response".to_string(),
    })
}

fn unreachable_err(message: &str) -> Scripted {
    Err(HttpError {
        kind: HttpErrorKind::Unreachable,
        message: message.to_string(),
    })
}

fn notify_send(message: &str) -> CommandCall {
    CommandCall::detached("notify-send", vec!["OctoPad / Home Assistant", message])
}

/// A state list shaped like `GET /api/states`.
fn states_fixture() -> Vec<Value> {
    vec![
        json!({
            "entity_id": "sensor.temperature",
            "state": "21.5",
            "attributes": {"friendly_name": "Temperature"}
        }),
        json!({
            "entity_id": "light.bed_lamp",
            "state": "on",
            "attributes": {"friendly_name": "Bed Lamp", "icon": "mdi:lightbulb"}
        }),
        json!({
            "entity_id": "weird",
            "state": "on"
        }),
        json!({
            "entity_id": "switch.alarm_relay",
            "state": "off",
            "attributes": {"friendly_name": "Alarm Relay"}
        }),
        json!({
            "entity_id": "automation.morning",
            "state": "on",
            "attributes": {"friendly_name": "Morning Routine"}
        }),
        json!({
            "entity_id": "fan.lounge",
            "state": "idle",
            "attributes": {"friendly_name": "Lounge Fan"}
        }),
        json!({
            "entity_id": "cover.garage",
            "state": "closed",
            "attributes": {"friendly_name": "Garage Door"}
        }),
        json!({
            "entity_id": "media_player.tv",
            "state": "playing",
            "attributes": {"friendly_name": "TV", "icon": "mdi:magnet"}
        }),
    ]
}

fn ok_scan() -> Scripted {
    status(200, &Value::Array(states_fixture()))
}

// ---------------------------------------------------------------------------
// check_connection (home_assistant.py:338-366)
// ---------------------------------------------------------------------------

// 347-358: `GET {url}/api/` with `Bearer {token}`; 200 → the body's message.
#[test]
fn a_200_check_reports_the_api_message_and_uses_the_saved_credentials() {
    let fx = fixture(vec![status(200, &json!({"message": "API running."}))]);
    let result = fx.state.check_connection(None, None);
    assert!(result.ok);
    assert_eq!(result.message, "Connected to Home Assistant (API running.)");
    let req = &fx.http.requests()[0];
    assert_eq!(req.method, "GET");
    assert_eq!(req.url, format!("{URL_A}/api/"));
    assert_eq!(req.header("Authorization"), Some("Bearer token-a"));
}

// 341: `url or self.url` — groundless (empty-string) args mean "use saved".
#[test]
fn empty_check_arguments_fall_back_to_the_saved_credentials() {
    let fx = fixture(vec![status(200, &json!({}))]);
    let result = fx
        .state
        .check_connection(Some(String::new()), Some(String::new()));
    assert!(result.ok);
    // 354-357: a body without a message falls back to "API running".
    assert_eq!(result.message, "Connected to Home Assistant (API running)");
    assert_eq!(fx.http.requests()[0].url, format!("{URL_A}/api/"));
}

// 341: unsaved (draft) credentials are used when both are present.
#[test]
fn a_check_with_both_draft_fields_tests_those_instead() {
    let fx = fixture(vec![status(200, &json!({}))]);
    let result = fx.state.check_connection(
        Some("https://other.local".to_string()),
        Some("tok-b".to_string()),
    );
    assert!(result.ok);
    let req = &fx.http.requests()[0];
    assert_eq!(req.url, "https://other.local/api/");
    assert_eq!(req.header("Authorization"), Some("Bearer tok-b"));
}

// 359-360: `Authentication failed: invalid Token (401 Unauthorized)`.
#[test]
fn a_401_check_names_the_invalid_token() {
    let fx = fixture(vec![status(401, &json!({"message": "Unauthorized"}))]);
    let result = fx.state.check_connection(None, None);
    assert!(!result.ok);
    assert_eq!(
        result.message,
        "Authentication failed: invalid Token (401 Unauthorized)"
    );
}

// 361-362: plain `Server responded with HTTP {status}` — Python has no body
// branch on the check-connection path (I-1).
#[test]
fn a_500_check_reports_the_status() {
    let fx = fixture(vec![raw_status(500, "backend is on fire")]);
    let result = fx.state.check_connection(None, None);
    assert!(!result.ok);
    assert_eq!(result.message, "Server responded with HTTP 500");
}

// I-1 repin (was a_500_check_appends_the_body_message_when_the_body_is_json):
// even a JSON error body with a `message` stays status-only here; the append
// belongs to service-call failures (`a_failed_service_call_carries_the_body_message`).
#[test]
fn a_502_check_with_a_json_body_stays_status_only() {
    let fx = fixture(vec![status(502, &json!({"message": "proxy jailbreak"}))]);
    let result = fx.state.check_connection(None, None);
    assert!(!result.ok);
    assert_eq!(result.message, "Server responded with HTTP 502");
}

// 363-364: `Connection timeout to {url} (check the LAN)`.
#[test]
fn a_timed_out_check_blames_the_lan() {
    let fx = fixture(vec![timeout_err()]);
    let result = fx.state.check_connection(None, None);
    assert!(!result.ok);
    assert_eq!(
        result.message,
        format!("Connection timeout to {URL_A} (check the LAN)")
    );
}

// 365-366: `Failed to reach Home Assistant: {e}`.
#[test]
fn an_unreachable_check_repeats_the_transport_error() {
    let fx = fixture(vec![unreachable_err("Connection refused (os error 111)")]);
    let result = fx.state.check_connection(None, None);
    assert!(!result.ok);
    assert_eq!(
        result.message,
        "Failed to reach Home Assistant: Connection refused (os error 111)"
    );
}

// 344-345: `Home Assistant URL or Token not configured`, no request.
#[test]
fn a_check_without_credentials_never_touches_the_transport() {
    let dir = tempfile::tempdir().unwrap();
    let env = tempfile::tempdir().unwrap();
    let http = FakeHttp::scripted(Vec::new());
    let clock = FakeClock::new(0.0);
    let state = HaState::load_with_seams(
        dir.path(),
        &env.path().join("env"),
        Box::new(http.clone()),
        Box::new(clock.clone()),
    );
    let result = state.check_connection(None, None);
    assert!(!result.ok);
    assert_eq!(result.message, "Home Assistant URL or Token not configured");
    assert_eq!(http.count(), 0);
}

// 42: `TIMEOUT_TEST = (2.0, 3.0)` — the read half is what Python passes on.
#[test]
fn the_check_uses_the_test_timeouts() {
    assert_eq!(TIMEOUT_TEST, (2.0, 3.0));
    let fx = fixture(vec![status(200, &json!({}))]);
    let _ = fx.state.check_connection(None, None);
    assert_eq!(fx.http.requests()[0].timeout, 3.0);
}

// ---------------------------------------------------------------------------
// load (home_assistant.py:235-266)
// ---------------------------------------------------------------------------

// 242-252: `{dir}/ha_config.json` wins.
#[test]
fn load_reads_the_octopad_config_file() {
    let fx = fixture_with_config(vec![timeout_err()], "https://loaded.local/", " tok ");
    let result = fx.state.check_connection(None, None);
    // 341-342: url is rstrip('/')-ed and token stripped at use time.
    assert!(!result.ok);
    assert_eq!(fx.http.requests()[0].url, "https://loaded.local/api/");
    assert_eq!(
        fx.http.requests()[0].header("Authorization"),
        Some("Bearer tok")
    );
}

// 254-266: fallback `~/.config/home-assistant/env` lines HA_URL=/HA_TOKEN=,
// quotes stripped (read-only migration, never written).
#[test]
fn load_falls_back_to_the_home_assistant_env_file() {
    let dir = tempfile::tempdir().unwrap();
    let env_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        env_dir.path().join("env"),
        "OTHER=1\nHA_URL=\"https://env.local\"\nHA_TOKEN='env-token'\n",
    )
    .unwrap();
    let http = FakeHttp::scripted(vec![status(200, &json!({}))]);
    let clock = FakeClock::new(0.0);
    let state = HaState::load_with_seams(
        dir.path(),
        &env_dir.path().join("env"),
        Box::new(http.clone()),
        Box::new(clock.clone()),
    );
    assert!(state.check_connection(None, None).ok);
    assert_eq!(http.requests()[0].url, "https://env.local/api/");
    assert_eq!(
        http.requests()[0].header("Authorization"),
        Some("Bearer env-token")
    );
}

// 249: a config file missing one credential does not short-circuit; the env
// file supplies the rest (Python keeps the partial value, too).
#[test]
fn a_partial_config_file_still_falls_back_to_the_env_file() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), URL_A, "");
    let env_dir = tempfile::tempdir().unwrap();
    std::fs::write(env_dir.path().join("env"), "HA_TOKEN=env-token\n").unwrap();
    let http = FakeHttp::scripted(vec![status(200, &json!({}))]);
    let clock = FakeClock::new(0.0);
    let state = HaState::load_with_seams(
        dir.path(),
        &env_dir.path().join("env"),
        Box::new(http.clone()),
        Box::new(clock.clone()),
    );
    assert!(state.check_connection(None, None).ok);
    assert_eq!(http.requests()[0].url, format!("{URL_A}/api/"));
    assert_eq!(
        http.requests()[0].header("Authorization"),
        Some("Bearer env-token")
    );
}

// ---------------------------------------------------------------------------
// save (home_assistant.py:278-302)
// ---------------------------------------------------------------------------

// 289-298: writes the exact Python file (keys url/token, indent 2) and
// chmods 0o600 (the chmod failure itself is non-fatal, Python 297-298).
#[cfg(unix)]
#[test]
fn save_writes_the_config_file_owned_only() {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture(Vec::new());
    let dir = fx.dir.path();
    // Python strips spaces from the token and slashes from the url, not
    // whitespace from the url (`:280-281`).
    assert!(fx.state.save("https://new.local/".into(), " tok ".into()));
    let value = read_config(dir);
    assert_eq!(value["url"], "https://new.local");
    assert_eq!(value["token"], "tok");
    let mode = std::fs::metadata(dir.join("ha_config.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "token file must be owner-only");
}

// 300-302: an IO error returns false instead of panicking.
#[test]
fn a_save_into_an_unusable_directory_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("occupied");
    std::fs::write(&blocked, b"a file where the config dir should be").unwrap();
    let env = tempfile::tempdir().unwrap();
    let http = FakeHttp::scripted(Vec::new());
    let clock = FakeClock::new(0.0);
    let state = HaState::load_with_seams(
        &blocked,
        &env.path().join("env"),
        Box::new(http),
        Box::new(clock),
    );
    assert!(!state.save(URL_A.into(), TOKEN_A.into()));
}

// ---------------------------------------------------------------------------
// get_entities (home_assistant.py:368-423, 200-221)
// ---------------------------------------------------------------------------

// 399-400: only INTERACTIVE_SERVICES domains survive; sensors never appear.
#[test]
fn a_scan_filters_non_interactive_domains() {
    let fx = fixture(vec![ok_scan()]);
    let entities = fx.state.get_entities(false);
    let ids: Vec<&str> = entities.iter().map(|e| e.entity_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "automation.morning",
            "cover.garage",
            "fan.lounge",
            "light.bed_lamp",
            "media_player.tv",
            "switch.alarm_relay"
        ]
    );
}

// 410: sort `(domain, friendly_name.lower())`.
#[test]
fn a_scan_sorts_by_domain_then_lowercase_friendly_name() {
    let mut fx = fixture(vec![ok_scan()]);
    // Fixture already arrives sorted correctly; shuffle the input to prove it.
    let shuffled = json!([
        json!({"entity_id": "switch.zzz", "state": "off", "attributes": {"friendly_name": "Zulu Switch"}}),
        json!({"entity_id": "light.aaa", "state": "on", "attributes": {"friendly_name": "accent strip"}}),
        json!({"entity_id": "light.bbb", "state": "on", "attributes": {"friendly_name": "Bed Lamp"}}),
    ]);
    fx.http.rearm(vec![status(200, &shuffled)]);
    let entities = fx.state.get_entities(true);
    let names: Vec<&str> = entities.iter().map(|e| e.friendly_name.as_str()).collect();
    assert_eq!(names, vec!["accent strip", "Bed Lamp", "Zulu Switch"]);
}

// 214-218 + 142-155: services come from INTERACTIVE_SERVICES; the default
// service from DEFAULT_SERVICE_OVERRIDES (`toggle` fallback).
#[test]
fn a_scan_decorates_services_and_the_default_service() {
    let fx = fixture(vec![ok_scan()]);
    let entities = fx.state.get_entities(false);

    let automation = entities.iter().find(|e| e.domain == "automation").unwrap();
    assert_eq!(automation.services, vec!["trigger", "turn_on", "turn_off"]);
    assert_eq!(automation.default_service, "trigger"); // home_assistant.py:143

    let cover = entities.iter().find(|e| e.domain == "cover").unwrap();
    assert_eq!(
        cover.services,
        vec![
            "toggle",
            "open_cover",
            "close_cover",
            "stop_cover",
            "set_cover_position"
        ]
    );
    assert_eq!(cover.default_service, "toggle"); // home_assistant.py:149

    let fan = entities.iter().find(|e| e.domain == "fan").unwrap();
    assert_eq!(
        fan.services,
        vec!["toggle", "turn_on", "turn_off", "set_percentage"]
    );
    assert_eq!(fan.default_service, "toggle");

    let light = entities.iter().find(|e| e.domain == "light").unwrap();
    // 402-408: state & friendly_name straight from the state object.
    assert_eq!(light.friendly_name, "Bed Lamp");
    assert_eq!(light.state.as_deref(), Some("on"));
    assert_eq!(light.entity_id, "light.bed_lamp");
}

// 157-166 + 105-137: mdi icons map to internal names; unknown icons fall back
// to the domain icon; the domain fallback is `package`.
#[test]
fn a_scan_resolves_icons_like_the_python_tables() {
    let fx = fixture(vec![ok_scan()]);
    let entities = fx.state.get_entities(false);
    let light = entities.iter().find(|e| e.domain == "light").unwrap();
    assert_eq!(light.icon.as_deref(), Some("lightbulb")); // mdi:lightbulb alias
    let switch = entities.iter().find(|e| e.domain == "switch").unwrap();
    assert_eq!(switch.icon.as_deref(), Some("toggle-right")); // DOMAIN_ICONS
    let automation = entities.iter().find(|e| e.domain == "automation").unwrap();
    assert_eq!(automation.icon.as_deref(), Some("settings")); // DOMAIN_ICONS
}

// 157-166: an icon the alias table does not know falls back to that domain's
// icon (mdi:magnet → media_player → "music").
#[test]
fn an_icon_the_alias_table_does_not_know_falls_back_to_the_domain_icon() {
    let fx = fixture(vec![ok_scan()]);
    let entities = fx.state.get_entities(false);
    let mp = entities
        .iter()
        .find(|e| e.domain == "media_player")
        .expect("media_player.tv must survive the filter");
    assert_eq!(mp.icon.as_deref(), Some("music"));
}

// 100: the last-chance domain fallback (`DEFAULT_ICON`): `resolve_icon` on an
// empty icon name for a domain… every interactive domain owns an icon, so the
// fallback is respected structurally - assert the sentinel constant.
#[test]
fn the_default_icon_sentinel_is_package() {
    assert_eq!(octopad_core::ha::DEFAULT_ICON, "package");
}

// 376-377: a warm memory cache answers without a transport round trip.
#[test]
fn a_warm_cache_serves_entities_without_a_request() {
    let fx = fixture(vec![ok_scan()]);
    assert_eq!(fx.state.get_entities(false).len(), 6);
    let before = fx.http.count();
    assert_eq!(fx.state.get_entities(false).len(), 6);
    assert_eq!(fx.http.count(), before);
}

// 181-189 + 268-276: the last scan is written to disk and an old install's
// scan survives migration (url match) — a fresh client reads it with no
// network request.
#[test]
fn the_disk_cache_survives_a_new_process() {
    let shared_dir = tempfile::tempdir().unwrap();
    let env = tempfile::tempdir().unwrap();
    write_config(shared_dir.path(), URL_A, TOKEN_A);

    let http1 = FakeHttp::scripted(vec![ok_scan()]);
    let clock1 = FakeClock::new(100.0);
    let state1 = HaState::load_with_seams(
        shared_dir.path(),
        &env.path().join("env"),
        Box::new(http1),
        Box::new(clock1),
    );
    let entities = state1.get_entities(false);
    assert_eq!(entities.len(), 6);

    // 186: the cache file records `url` (stripped) and `entities`.
    let file = shared_dir.path().join("ha_entities_cache.json");
    let cached: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(cached["url"], URL_A);
    assert_eq!(cached["entities"].as_array().unwrap().len(), 6);

    let http2 = FakeHttp::scripted(Vec::new()); // any request would panic
    let clock2 = FakeClock::new(0.0);
    let state2 = HaState::load_with_seams(
        shared_dir.path(),
        &env.path().join("env"),
        Box::new(http2.clone()),
        Box::new(clock2.clone()),
    );
    assert_eq!(state2.get_entities(false), entities);
    assert_eq!(http2.count(), 0);
}

// 272: a cache written for another HA instance is ignored.
#[test]
fn a_new_process_from_another_url_ignores_the_cached_scan() {
    let shared_dir = tempfile::tempdir().unwrap();
    let env = tempfile::tempdir().unwrap();
    write_config(shared_dir.path(), URL_A, TOKEN_A);

    let http1 = FakeHttp::scripted(vec![ok_scan()]);
    let state1 = HaState::load_with_seams(
        shared_dir.path(),
        &env.path().join("env"),
        Box::new(http1),
        Box::new(FakeClock::new(0.0)),
    );
    state1.get_entities(false);

    write_config(shared_dir.path(), "https://other.local", "tok-b");
    let http2 = FakeHttp::scripted(vec![timeout_err()]); // url B scan stubbed away
    let state2 = HaState::load_with_seams(
        shared_dir.path(),
        &env.path().join("env"),
        Box::new(http2.clone()),
        Box::new(FakeClock::new(0.0)),
    );
    let entities = state2.get_entities(false);
    assert_eq!(http2.count(), 1); // disk was skipped, a live attempt was made
    assert!(entities.is_empty());
}

// 386 + 44-45: background (non-forced) scans use (1.5, 5), forced use (2, 8).
#[test]
fn background_and_forced_scans_use_their_own_timeouts() {
    assert_eq!(TIMEOUT_SCAN_BG, (1.5, 5.0));
    assert_eq!(TIMEOUT_SCAN, (2.0, 8.0));
    let fx = fixture(vec![ok_scan(), ok_scan()]);
    let _ = fx.state.get_entities(false);
    assert_eq!(fx.http.requests()[0].timeout, 5.0);
    let _ = fx.state.get_entities(true);
    assert_eq!(fx.http.requests()[1].timeout, 8.0);
}

// 47-52 + 379-383 (plan Review Focus 2, named there).
#[test]
fn the_retry_guard_blocks_a_second_live_scan() {
    let fx = fixture(vec![timeout_err()]);
    let clock = fx.clock.clone();
    let http = fx.http.clone();

    assert!(fx.state.get_entities(false).is_empty()); // failed scan, t=100
    assert_eq!(http.count(), 1);

    clock.set(110.0); // 10 s later, still within the 30 s guard
    let again = fx.state.get_entities(false);
    assert!(again.is_empty());
    assert_eq!(http.count(), 1, "the guard must not spend a second request");
}

// 381: `force_refresh` bypasses the guard (manual scan is on demand).
#[test]
fn a_forced_scan_ignores_the_retry_guard() {
    let fx = fixture(vec![timeout_err(), ok_scan()]);
    let clock = fx.clock.clone();
    fx.state.get_entities(false);
    clock.set(110.0);
    let entities = fx.state.get_entities(true);
    assert_eq!(entities.len(), 6);
    assert_eq!(fx.http.count(), 2);
}

// 421-423 + 374-375: a fresh client in an unknown place with an unusable cache
// file still answers (empty, offline) instead of panicking.
#[test]
fn a_scan_when_unconfigured_stays_empty_without_requests() {
    let dir = tempfile::tempdir().unwrap();
    let env = tempfile::tempdir().unwrap();
    let http = FakeHttp::scripted(Vec::new());
    let clock = FakeClock::new(0.0);
    let state = HaState::load_with_seams(
        dir.path(),
        &env.path().join("env"),
        Box::new(http.clone()),
        Box::new(clock.clone()),
    );
    assert!(state.get_entities(false).is_empty());
    assert_eq!(http.count(), 0);
}

// ---------------------------------------------------------------------------
// save × entity cache (plan Review Focus 3, named there)
// ---------------------------------------------------------------------------

// 282-289: saving a different URL drops the old cache (memory + disk) so a
// stale HA's entities never show; saving the same URL keeps them.
#[test]
fn a_url_change_discards_the_entity_cache() {
    let fx = fixture(vec![ok_scan()]);
    let entities_before = fx.state.get_entities(false);
    assert_eq!(entities_before.len(), 6);
    assert!(fx.dir.path().join("ha_entities_cache.json").exists());

    // Same URL: keep the cache.
    assert!(fx.state.save(format!("{URL_A}/"), TOKEN_A.into()));
    assert_eq!(fx.state.get_entities(false).len(), 6);

    // Different URL: cache gone (Python 282-289 clears on every save; the new
    // rule keeps it iff the URL did not change).
    assert!(fx.state.save("https://other.local".into(), TOKEN_A.into()));
    assert!(!fx.dir.path().join("ha_entities_cache.json").exists());
    fx.http.rearm(vec![timeout_err()]);
    assert!(fx.state.get_entities(false).is_empty());
}

// ---------------------------------------------------------------------------
// call_service (home_assistant.py:425-455 + spec §2 table)
// ---------------------------------------------------------------------------

// 445-448: 200 → `Service {domain}.{service} executed successfully`. Owner
// decision 2026-10-19: no notify-send on success, ever.
#[test]
fn a_successful_service_call_reports_exact_python_success() {
    let runner = RecordingRunner::new(ActionOutcome::ok("sent"));
    let info = json!({"volume": 30});
    let fx = fixture(vec![status(200, &json!([{"state": "playing"}]))]);
    let outcome = fx.state.call_service(
        "media_player",
        "volume_set",
        Some("media_player.tv"),
        &info,
        &runner,
    );
    assert!(outcome.ok, "unexpected failure: {}", outcome.reason);
    assert_eq!(
        outcome.reason,
        "Service media_player.volume_set executed successfully"
    );
    let req = &fx.http.requests()[0];
    assert_eq!(
        req.url,
        format!("{URL_A}/api/services/media_player/volume_set")
    );
    assert_eq!(req.header("Authorization"), Some("Bearer token-a"));
    assert_eq!(req.header("Content-Type"), Some("application/json"));
    assert_eq!(req.timeout, 5.0);
    let body: Value = serde_json::from_str(req.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["entity_id"], "media_player.tv");
    assert_eq!(body["volume"], 30);
    // failure-only notifications: zero runner calls on success
    assert_eq!(runner.call_count(), 0);
}

// 431-433: `data or {}`, and `entity_id` only when truthy.
#[test]
fn a_service_call_body_is_data_or_empty_with_entity_id() {
    let fx = fixture(vec![status(200, &json!([])), status(200, &json!([]))]);
    let _ = fx.state.call_service(
        "script",
        "turn_on",
        None,
        &json!({}),
        &RecordingRunner::new(ActionOutcome::ok("")),
    );
    let _ = fx.state.call_service(
        "script",
        "turn_on",
        Some(""),
        &json!({"fields": {}}),
        &RecordingRunner::new(ActionOutcome::ok("")),
    );
    let first_body: Value =
        serde_json::from_str(fx.http.requests()[0].body.as_deref().unwrap()).unwrap();
    assert_eq!(first_body, json!({}));
    let second_body: Value =
        serde_json::from_str(fx.http.requests()[1].body.as_deref().unwrap()).unwrap();
    assert_eq!(
        second_body,
        json!({"fields": {}}),
        "an empty entity id is skipped"
    );
}

// spec §2 service-call failure strings (brief: "401 body → Authentication
// failed: invalid Token (401 Unauthorized)") + failure-only notify.
#[test]
fn an_unauthorized_service_call_names_the_token_problem() {
    let runner = RecordingRunner::new(ActionOutcome::ok("sent"));
    let fx = fixture(vec![status(401, &json!({}))]);
    let outcome = fx
        .state
        .call_service("light", "toggle", None, &json!({}), &runner);
    assert!(!outcome.ok);
    assert_eq!(
        outcome.reason,
        "Authentication failed: invalid Token (401 Unauthorized)"
    );
    assert_eq!(runner.calls(), vec![notify_send("Failed (401)")]);
}

// spec §2: HTTP error body contributes its message when present.
#[test]
fn a_failed_service_call_carries_the_body_message() {
    let runner = RecordingRunner::new(ActionOutcome::ok("sent"));
    let fx = fixture(vec![status(
        503,
        &json!({"message": "Service uploader unavailable"}),
    )]);
    let outcome = fx
        .state
        .call_service("update", "install", None, &json!({}), &runner);
    assert!(!outcome.ok);
    assert_eq!(
        outcome.reason,
        "Server responded with HTTP 503: Service uploader unavailable"
    );
    assert_eq!(runner.calls(), vec![notify_send("Failed (503)")]);
}

// plan Review Focus 1 (named there): HA lost mid-action fails fast with the
// exact timeout reason. The Python `except` message "Connection error: …" is
// superseded by the pinned spec string.
#[test]
fn a_lost_ha_fails_fast() {
    let runner = RecordingRunner::new(ActionOutcome::ok("sent"));
    let fx = fixture(vec![timeout_err()]);
    let outcome = fx
        .state
        .call_service("light", "toggle", None, &json!({}), &runner);
    assert!(!outcome.ok);
    assert_eq!(
        outcome.reason,
        format!("Connection timeout to {URL_A} (check the LAN)")
    );
    assert_eq!(
        runner.calls(),
        vec![notify_send("Failed to reach Home Assistant")]
    );
}

// Review Focus 1's second leg: transport errors repeat the Python wording.
#[test]
fn an_unreachable_service_call_repeats_the_transport_error() {
    let runner = RecordingRunner::new(ActionOutcome::ok("sent"));
    let fx = fixture(vec![unreachable_err("Connection refused (os error 111)")]);
    let outcome = fx
        .state
        .call_service("light", "toggle", None, &json!({}), &runner);
    assert!(!outcome.ok);
    assert_eq!(
        outcome.reason,
        "Failed to reach Home Assistant: Connection refused (os error 111)"
    );
    assert_eq!(
        runner.calls(),
        vec![notify_send("Failed to reach Home Assistant")]
    );
}

// 427-429: `Home Assistant not configured`, notify, and no request at all.
#[test]
fn an_unconfigured_service_call_fails_without_a_request() {
    let dir = tempfile::tempdir().unwrap();
    let env = tempfile::tempdir().unwrap();
    let http = FakeHttp::scripted(Vec::new());
    let clock = FakeClock::new(0.0);
    let state = HaState::load_with_seams(
        dir.path(),
        &env.path().join("env"),
        Box::new(http),
        Box::new(clock),
    );
    let runner = RecordingRunner::new(ActionOutcome::ok("sent"));
    let outcome = state.call_service("light", "toggle", None, &json!({}), &runner);
    assert!(!outcome.ok);
    assert_eq!(outcome.reason, "Home Assistant not configured");
    assert_eq!(
        runner.calls(),
        vec![notify_send("Home Assistant config not set")]
    );
}

// ---------------------------------------------------------------------------
// send_webhook (system_actions.py:420-442)
// ---------------------------------------------------------------------------

// 430-431: `Home Assistant Webhook URL is empty`.
#[test]
fn a_webhook_without_a_url_is_rejected() {
    let fx = fixture(Vec::new());
    let outcome = fx.state.send_webhook("   ", &json!({"on": true}));
    assert!(!outcome.ok);
    assert_eq!(outcome.reason, "Home Assistant Webhook URL is empty");
    assert_eq!(fx.http.count(), 0);
}

// 433-442: POST, application/json, 4 s, `Webhook sent to Home Assistant
// (HTTP 200)`. No auth header on webhooks.
#[test]
fn a_webhook_reports_the_http_status() {
    let fx = fixture(vec![status(200, &json!({}))]);
    let payload = json!({"on": true});
    let outcome = fx.state.send_webhook("https://hooks.local/way", &payload);
    assert!(outcome.ok, "unexpected failure: {}", outcome.reason);
    assert_eq!(outcome.reason, "Webhook sent to Home Assistant (HTTP 200)");
    let req = &fx.http.requests()[0];
    assert_eq!(req.method, "POST");
    assert_eq!(req.url, "https://hooks.local/way");
    assert_eq!(req.header("Content-Type"), Some("application/json"));
    assert_eq!(req.header("Authorization"), None);
    assert_eq!(req.timeout, 4.0);
    assert_eq!(
        req.body.as_deref().unwrap(),
        serde_json::to_string(&payload).unwrap()
    );
}

// 433: an empty payload is sent as `{}`.
#[test]
fn an_empty_webhook_payload_sends_an_empty_object() {
    let fx = fixture(vec![status(201, &json!({}))]);
    let outcome = fx.state.send_webhook("https://hooks.local/way", &json!({}));
    assert!(outcome.ok);
    assert_eq!(outcome.reason, "Webhook sent to Home Assistant (HTTP 201)");
    assert_eq!(fx.http.requests()[0].body.as_deref().unwrap(), "{}");
}
