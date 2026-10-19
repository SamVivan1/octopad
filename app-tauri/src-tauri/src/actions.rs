#[cfg(test)]
mod tests {
    use super::{ActionDispatcher, ActionExecutor, ActionResult, ActionServices};
    use octopad_core::config::Action;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingServices {
        calls: Mutex<Vec<String>>,
    }

    impl RecordingServices {
        fn record(&self, action: &Action) -> ActionResult {
            self.calls.lock().unwrap().push(action.action_type.clone());
            if action.action_type == "shortcut" {
                ActionResult {
                    ok: false,
                    message: "wtype is unavailable".to_string(),
                }
            } else {
                ActionResult {
                    ok: true,
                    message: format!("{} handled", action.action_type),
                }
            }
        }
    }

    macro_rules! service_method {
        ($name:ident) => {
            fn $name(&self, action: &Action) -> ActionResult {
                self.record(action)
            }
        };
    }

    impl ActionServices for RecordingServices {
        service_method!(wait);
        service_method!(shortcut);
        service_method!(text);
        service_method!(media);
        service_method!(audio_output);
        service_method!(launch_app);
        service_method!(system_action);
        service_method!(bash_script);
        service_method!(ha_webhook);
        service_method!(home_assistant);
    }

    fn action(action_type: &str) -> Action {
        Action {
            action_type: action_type.to_string(),
            extra: HashMap::new(),
        }
    }

    #[test]
    fn execute_action_dispatches_each_supported_action_type() {
        let services = Arc::new(RecordingServices::default());
        let executor = ActionDispatcher::new(services.clone());
        let types = [
            "none",
            "mode_toggle",
            "delay",
            "shortcut",
            "text",
            "media",
            "audio_output",
            "launch_app",
            "system_action",
            "bash_script",
            "ha_webhook",
            "home_assistant",
        ];

        let results: Vec<_> = types
            .iter()
            .map(|action_type| executor.execute(&action(action_type)))
            .collect();

        assert!(results[0].ok);
        assert!(results[1].ok);
        assert!(
            !results[3].ok,
            "an unavailable tool must surface as failure"
        );
        assert_eq!(results[3].message, "wtype is unavailable");
        assert_eq!(
            services.calls.lock().unwrap().as_slice(),
            [
                "delay",
                "shortcut",
                "text",
                "media",
                "audio_output",
                "launch_app",
                "system_action",
                "bash_script",
                "ha_webhook",
                "home_assistant",
            ]
        );
    }

    #[test]
    fn execute_action_rejects_unknown_action_types() {
        let executor = ActionDispatcher::new(Arc::new(RecordingServices::default()));

        let result = executor.execute(&action("unsupported"));

        assert!(!result.ok);
        assert!(result.message.contains("unsupported"));
    }
}
use octopad_core::config::Action;
use octopad_core::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::State;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    pub message: String,
}

impl ActionResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

pub trait ActionExecutor: Send + Sync {
    fn execute(&self, action: &Action) -> ActionResult;
}

pub trait ActionServices: Send + Sync {
    fn wait(&self, action: &Action) -> ActionResult;
    fn shortcut(&self, action: &Action) -> ActionResult;
    fn text(&self, action: &Action) -> ActionResult;
    fn media(&self, action: &Action) -> ActionResult;
    fn audio_output(&self, action: &Action) -> ActionResult;
    fn launch_app(&self, action: &Action) -> ActionResult;
    fn system_action(&self, action: &Action) -> ActionResult;
    fn bash_script(&self, action: &Action) -> ActionResult;
    fn ha_webhook(&self, action: &Action) -> ActionResult;
    fn home_assistant(&self, action: &Action) -> ActionResult;
}

pub struct ActionDispatcher {
    services: Arc<dyn ActionServices>,
}

impl ActionDispatcher {
    pub fn new(services: Arc<dyn ActionServices>) -> Self {
        Self { services }
    }
}

impl ActionExecutor for ActionDispatcher {
    fn execute(&self, action: &Action) -> ActionResult {
        match action.action_type.as_str() {
            "none" => ActionResult::success("No action configured"),
            "mode_toggle" => {
                ActionResult::success("Mode layer is controlled by the macropad firmware (LED)")
            }
            "delay" => self.services.wait(action),
            "shortcut" => self.services.shortcut(action),
            "text" => self.services.text(action),
            "media" => self.services.media(action),
            "audio_output" => self.services.audio_output(action),
            "launch_app" => self.services.launch_app(action),
            "system_action" => self.services.system_action(action),
            "bash_script" => self.services.bash_script(action),
            "ha_webhook" => self.services.ha_webhook(action),
            "home_assistant" | "ha_service" => self.services.home_assistant(action),
            action_type => ActionResult::failure(format!("Unknown action type: {action_type}")),
        }
    }
}

pub struct LinuxActionServices;

impl ActionServices for LinuxActionServices {
    fn wait(&self, action: &Action) -> ActionResult {
        let seconds = match action.extra.get("seconds") {
            Some(value) => match value_as_f64(value) {
                Some(seconds) => seconds,
                None => return ActionResult::failure("Delay needs a valid duration."),
            },
            None => 0.5,
        };
        if !seconds.is_finite() {
            return ActionResult::failure("Delay needs a valid duration.");
        }
        let seconds = seconds.clamp(0.0, 600.0);
        thread::sleep(Duration::from_secs_f64(seconds));
        ActionResult::success(format!("Waited {seconds}s."))
    }

    fn shortcut(&self, action: &Action) -> ActionResult {
        let key = string_value(action, "key").unwrap_or_default();
        if key.is_empty() {
            return ActionResult::failure("Target key not selected yet");
        }
        let modifiers = action
            .extra
            .get("modifiers")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|modifier| ["ctrl", "alt", "shift", "super"].contains(modifier))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut args = Vec::new();
        if command_available("wtype") {
            for modifier in &modifiers {
                args.extend(["-M".to_string(), modifier.clone()]);
            }
            let key = key_name(&key);
            args.extend(["-P".to_string(), key.clone(), "-p".to_string(), key]);
            for modifier in modifiers.iter().rev() {
                args.extend(["-m".to_string(), modifier.clone()]);
            }
            return run_command("wtype", &args, Duration::from_secs(6));
        }
        if command_available("ydotool") {
            let codes = ydotool_key_codes(&modifiers, &key);
            if codes.is_empty() {
                return ActionResult::failure(format!("Key '{key}' not recognized by ydotool"));
            }
            args.extend(codes);
            return run_command(
                "ydotool",
                &["key".to_string()]
                    .into_iter()
                    .chain(args)
                    .collect::<Vec<_>>(),
                Duration::from_secs(6),
            );
        }
        if command_available("xdotool") {
            let mut combo = modifiers;
            combo.push(key_name(&key));
            return run_command(
                "xdotool",
                &[
                    "key".to_string(),
                    "--clearmodifiers".to_string(),
                    combo.join("+"),
                ],
                Duration::from_secs(6),
            );
        }
        ActionResult::failure(keyboard_help())
    }

    fn text(&self, action: &Action) -> ActionResult {
        let text = string_value(action, "text").unwrap_or_default();
        if text.is_empty() {
            return ActionResult::failure("Empty text");
        }
        let (program, args) = if command_available("wtype") {
            ("wtype", vec!["--".to_string(), text])
        } else if command_available("ydotool") {
            ("ydotool", vec!["type".to_string(), "--".to_string(), text])
        } else if command_available("xdotool") {
            (
                "xdotool",
                vec![
                    "type".to_string(),
                    "--clearmodifiers".to_string(),
                    "--".to_string(),
                    text,
                ],
            )
        } else {
            return ActionResult::failure(keyboard_help());
        };
        run_command(program, &args, Duration::from_secs(6))
    }

    fn media(&self, action: &Action) -> ActionResult {
        let key = string_value(action, "mediaKey").unwrap_or_default();
        let volume_step = match key.as_str() {
            "VOL_UP" => Some("5%+"),
            "VOL_DOWN" => Some("5%-"),
            _ => None,
        };
        if let Some(step) = volume_step {
            if command_available("wpctl") {
                return run_command(
                    "wpctl",
                    &[
                        "set-volume".to_string(),
                        "@DEFAULT_AUDIO_SINK@".to_string(),
                        step.to_string(),
                    ],
                    Duration::from_secs(6),
                );
            }
            if command_available("pactl") {
                return run_command(
                    "pactl",
                    &[
                        "set-sink-volume".to_string(),
                        "@DEFAULT_SINK@".to_string(),
                        if key == "VOL_UP" { "+5%" } else { "-5%" }.to_string(),
                    ],
                    Duration::from_secs(6),
                );
            }
            return ActionResult::failure("Volume requires wpctl or pactl (pipewire/pulseaudio).");
        }
        if key == "MUTE" {
            if command_available("wpctl") {
                return run_command(
                    "wpctl",
                    &[
                        "set-mute".to_string(),
                        "@DEFAULT_AUDIO_SINK@".to_string(),
                        "toggle".to_string(),
                    ],
                    Duration::from_secs(6),
                );
            }
            if command_available("pactl") {
                return run_command(
                    "pactl",
                    &[
                        "set-sink-mute".to_string(),
                        "@DEFAULT_SINK@".to_string(),
                        "toggle".to_string(),
                    ],
                    Duration::from_secs(6),
                );
            }
            return ActionResult::failure("Volume requires wpctl or pactl (pipewire/pulseaudio).");
        }
        let player_command = match key.as_str() {
            "PLAY_PAUSE" => Some("play-pause"),
            "NEXT_TRACK" => Some("next"),
            "PREV_TRACK" => Some("previous"),
            _ => None,
        };
        if let Some(command) = player_command {
            if command_available("playerctl") {
                return run_command("playerctl", &[command.to_string()], Duration::from_secs(6));
            }
            let key = match key.as_str() {
                "PLAY_PAUSE" => "XF86AudioPlay",
                "NEXT_TRACK" => "XF86AudioNext",
                _ => "XF86AudioPrev",
            };
            let mut shortcut = Action {
                action_type: "shortcut".to_string(),
                extra: action.extra.clone(),
            };
            shortcut
                .extra
                .insert("key".to_string(), Value::String(key.to_string()));
            return self.shortcut(&shortcut);
        }
        ActionResult::failure(format!("Unknown media key: {key}"))
    }

    fn audio_output(&self, action: &Action) -> ActionResult {
        let sinks = action
            .extra
            .get("sinks")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let name = item.get("name")?.as_str()?;
                        Some((
                            name.to_string(),
                            item.get("description")
                                .and_then(Value::as_str)
                                .unwrap_or(name)
                                .to_string(),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if sinks.is_empty() {
            return ActionResult::failure("No audio output selected");
        }
        if !command_available("pactl") {
            return ActionResult::failure("pactl is required to switch the audio output");
        }
        let current = command_output(
            "pactl",
            &["get-default-sink".to_string()],
            Duration::from_secs(5),
        )
        .ok()
        .map(|out| out.trim().to_string());
        let current_index = current
            .as_deref()
            .and_then(|name| sinks.iter().position(|(sink, _)| sink == name))
            .unwrap_or(sinks.len() - 1);
        let target = &sinks[(current_index + 1) % sinks.len()];
        let result = run_command(
            "pactl",
            &["set-default-sink".to_string(), target.0.clone()],
            Duration::from_secs(5),
        );
        if !result.ok {
            return result;
        }
        if let Ok(inputs) = command_output(
            "pactl",
            &[
                "list".to_string(),
                "short".to_string(),
                "sink-inputs".to_string(),
            ],
            Duration::from_secs(5),
        ) {
            for input_id in inputs.lines().filter_map(|line| {
                let id = line.split('\t').next()?.trim();
                id.parse::<u64>().ok().map(|_| id.to_string())
            }) {
                let _ = run_command(
                    "pactl",
                    &["move-sink-input".to_string(), input_id, target.0.clone()],
                    Duration::from_secs(5),
                );
            }
        }
        ActionResult::success(format!("Audio output: {}", target.1))
    }

    fn launch_app(&self, action: &Action) -> ActionResult {
        let target = string_value(action, "desktopId")
            .filter(|value| !value.is_empty())
            .or_else(|| string_value(action, "path"))
            .unwrap_or_default();
        if target.is_empty() {
            return ActionResult::failure("Target desktop ID or path is empty");
        }
        let basename = std::path::Path::new(&target)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&target)
            .trim_end_matches(".desktop")
            .to_string();
        if command_available("gtk-launch") {
            return spawn_detached("gtk-launch", &[basename.clone()])
                .map(|_| ActionResult::success(format!("Launched via gtk-launch: {basename}")))
                .unwrap_or_else(ActionResult::failure);
        }
        let desktop_file = resolve_desktop_file(&target);
        if let Some(desktop_file) = desktop_file.filter(|_| command_available("gio")) {
            let desktop_file = desktop_file.to_string_lossy().into_owned();
            return spawn_detached("gio", &["launch".to_string(), desktop_file.clone()])
                .map(|_| ActionResult::success(format!("Launched via gio: {desktop_file}")))
                .unwrap_or_else(ActionResult::failure);
        }
        ActionResult::failure(format!("Could not find or launch application: {target}"))
    }

    fn system_action(&self, action: &Action) -> ActionResult {
        let preset = string_value(action, "presetId").unwrap_or_default();
        match preset.as_str() {
            "mic_toggle_mute" => {
                if command_available("wpctl") {
                    run_command(
                        "wpctl",
                        &[
                            "set-mute".to_string(),
                            "@DEFAULT_AUDIO_SOURCE@".to_string(),
                            "toggle".to_string(),
                        ],
                        Duration::from_secs(6),
                    )
                } else if command_available("pactl") {
                    run_command(
                        "pactl",
                        &[
                            "set-source-mute".to_string(),
                            "@DEFAULT_SOURCE@".to_string(),
                            "toggle".to_string(),
                        ],
                        Duration::from_secs(6),
                    )
                } else if command_available("amixer") {
                    run_command(
                        "amixer",
                        &[
                            "set".to_string(),
                            "Capture".to_string(),
                            "toggle".to_string(),
                        ],
                        Duration::from_secs(6),
                    )
                } else {
                    ActionResult::failure("Microphone mute requires wpctl, pactl, or amixer")
                }
            }
            "volume_up_5" | "volume_down_5" => {
                let up = preset == "volume_up_5";
                let (program, args) = if command_available("wpctl") {
                    (
                        "wpctl",
                        vec![
                            "set-volume".to_string(),
                            "@DEFAULT_AUDIO_SINK@".to_string(),
                            if up { "5%+" } else { "5%-" }.to_string(),
                        ],
                    )
                } else if command_available("pactl") {
                    (
                        "pactl",
                        vec![
                            "set-sink-volume".to_string(),
                            "@DEFAULT_SINK@".to_string(),
                            if up { "+5%" } else { "-5%" }.to_string(),
                        ],
                    )
                } else {
                    return ActionResult::failure("Volume requires wpctl or pactl");
                };
                run_command(program, &args, Duration::from_secs(6))
            }
            "volume_toggle_mute" => {
                let (program, args) = if command_available("wpctl") {
                    (
                        "wpctl",
                        vec![
                            "set-mute".to_string(),
                            "@DEFAULT_AUDIO_SINK@".to_string(),
                            "toggle".to_string(),
                        ],
                    )
                } else if command_available("pactl") {
                    (
                        "pactl",
                        vec![
                            "set-sink-mute".to_string(),
                            "@DEFAULT_SINK@".to_string(),
                            "toggle".to_string(),
                        ],
                    )
                } else {
                    return ActionResult::failure("Speaker mute requires wpctl or pactl");
                };
                run_command(program, &args, Duration::from_secs(6))
            }
            "lock_screen" => {
                let (program, args) = if command_available("loginctl") {
                    ("loginctl", vec!["lock-session".to_string()])
                } else if command_available("xdg-screensaver") {
                    ("xdg-screensaver", vec!["lock".to_string()])
                } else {
                    return ActionResult::failure(
                        "Screen lock requires loginctl or xdg-screensaver",
                    );
                };
                run_command(program, &args, Duration::from_secs(6))
            }
            "open_terminal" => {
                for program in [
                    "gnome-terminal",
                    "x-terminal-emulator",
                    "alacritty",
                    "kitty",
                    "konsole",
                    "xfce4-terminal",
                ] {
                    if command_available(program) {
                        return spawn_detached(program, &[])
                            .map(|_| ActionResult::success(format!("Opened terminal: {program}")))
                            .unwrap_or_else(ActionResult::failure);
                    }
                }
                ActionResult::failure("No supported terminal application was found")
            }
            "screenshot_interactive" => {
                if command_available("gnome-screenshot") {
                    spawn_detached("gnome-screenshot", &["-a".to_string()])
                        .map(|_| ActionResult::success("Triggered interactive screenshot"))
                        .unwrap_or_else(ActionResult::failure)
                } else if command_available("notify-send") {
                    spawn_detached(
                        "notify-send",
                        &[
                            "OctoPad".to_string(),
                            "Use PrintScreen to take a screenshot".to_string(),
                        ],
                    )
                    .map(|_| ActionResult::success("Screenshot requested"))
                    .unwrap_or_else(ActionResult::failure)
                } else {
                    ActionResult::failure("Screenshot requires gnome-screenshot or notify-send")
                }
            }
            "media_play_pause" | "media_next" | "media_prev" => {
                if !command_available("playerctl") {
                    return ActionResult::failure(
                        "playerctl not installed (sudo apt install playerctl)",
                    );
                }
                let command = match preset.as_str() {
                    "media_play_pause" => "play-pause",
                    "media_next" => "next",
                    _ => "previous",
                };
                run_command("playerctl", &[command.to_string()], Duration::from_secs(6))
            }
            _ => ActionResult::failure(format!("Unknown preset: {preset}")),
        }
    }

    fn bash_script(&self, action: &Action) -> ActionResult {
        let script = string_value(action, "script")
            .unwrap_or_default()
            .trim()
            .to_string();
        if script.is_empty() {
            return ActionResult::failure("Script content is empty");
        }
        match Command::new("/bin/bash")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => ActionResult::success("Bash script dispatched in background"),
            Err(error) => ActionResult::failure(format!("Failed to start bash script: {error}")),
        }
    }

    fn ha_webhook(&self, action: &Action) -> ActionResult {
        let url = string_value(action, "url")
            .unwrap_or_default()
            .trim()
            .to_string();
        if url.is_empty() {
            return ActionResult::failure("Home Assistant Webhook URL is empty");
        }
        if !command_available("curl") {
            return ActionResult::failure("Home Assistant webhooks require curl");
        }
        let payload = action
            .extra
            .get("payload")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let body = match serde_json::to_string(&payload) {
            Ok(body) => body,
            Err(error) => return ActionResult::failure(error.to_string()),
        };
        let args = [
            "--silent".to_string(),
            "--show-error".to_string(),
            "--max-time".to_string(),
            "4".to_string(),
            "--request".to_string(),
            "POST".to_string(),
            "--header".to_string(),
            "Content-Type: application/json".to_string(),
            "--data".to_string(),
            body,
            "--output".to_string(),
            "/dev/null".to_string(),
            "--write-out".to_string(),
            "%{http_code}".to_string(),
            url,
        ];
        match command_output("curl", &args, Duration::from_secs(5)) {
            Ok(status) if status.trim().starts_with('2') => ActionResult::success(format!(
                "Webhook sent to Home Assistant (HTTP {})",
                status.trim()
            )),
            Ok(_) | Err(_) => ActionResult::failure("Home Assistant webhook request failed"),
        }
    }

    fn home_assistant(&self, _action: &Action) -> ActionResult {
        ActionResult::failure("Home Assistant service is not configured")
    }
}

pub fn linux_action_executor() -> Arc<dyn ActionExecutor> {
    Arc::new(ActionDispatcher::new(Arc::new(LinuxActionServices)))
}

pub(crate) fn is_runnable_action(action: &Action) -> bool {
    !action.action_type.trim().is_empty() && action.action_type != "none"
}

pub(crate) fn run_action_sequence(
    actions: &[Action],
    executor: &dyn ActionExecutor,
) -> Vec<ActionResult> {
    actions
        .iter()
        .filter(|action| is_runnable_action(action))
        .map(|action| executor.execute(action))
        .collect()
}

#[tauri::command]
pub async fn test_action_cmd(
    action: Action,
    executor: State<'_, Arc<dyn ActionExecutor>>,
) -> Result<ActionResult, String> {
    let executor = executor.inner().clone();
    tokio::task::spawn_blocking(move || executor.execute(&action))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn test_trigger_cmd(
    trigger: String,
    state: State<'_, Arc<AppState>>,
    executor: State<'_, Arc<dyn ActionExecutor>>,
) -> Result<Vec<ActionResult>, String> {
    if !matches!(trigger.as_str(), "single" | "double" | "hold") {
        return Err(format!("Unknown trigger: {trigger}"));
    }
    let actions = state.get_actions(&trigger);
    let executor = executor.inner().clone();
    tokio::task::spawn_blocking(move || run_action_sequence(&actions, executor.as_ref()))
        .await
        .map_err(|error| error.to_string())
}

fn string_value(action: &Action, field: &str) -> Option<String> {
    action
        .extra
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn command_available(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| directory.join(program).is_file())
}

fn run_command(program: &str, args: &[String], timeout: Duration) -> ActionResult {
    match command_output(program, args, timeout) {
        Ok(_) => ActionResult::success(format!("{program} completed")),
        Err(error) => ActionResult::failure(error),
    }
}

fn command_output(program: &str, args: &[String], timeout: Duration) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("{program} failed to start: {error}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("{program} timed out"));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("{program} failed: {error}"));
            }
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        let detail = stderr
            .lines()
            .chain(stdout.lines())
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        if detail.is_empty() {
            Err(format!("{program} exited with {status}"))
        } else {
            Err(format!("{program}: {detail}"))
        }
    }
}

fn read_pipe(pipe: Option<impl std::io::Read>) -> String {
    use std::io::Read;
    let mut contents = String::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_string(&mut contents);
    }
    contents
}

fn spawn_detached(program: &str, args: &[String]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{program} failed to start: {error}"))?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(100) {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{program} exited with {status}")),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(format!("{program} failed: {error}")),
        }
    }
    Ok(())
}

fn resolve_desktop_file(target: &str) -> Option<std::path::PathBuf> {
    let target = std::path::Path::new(target);
    if target.exists() {
        return Some(target.to_path_buf());
    }
    let mut search_dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        search_dirs.push(std::path::PathBuf::from(home).join(".local/share/applications"));
    }
    search_dirs.extend([
        std::path::PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        std::path::PathBuf::from("/var/lib/snapd/desktop/applications"),
        std::path::PathBuf::from("/usr/share/applications"),
    ]);
    search_dirs
        .into_iter()
        .map(|dir| dir.join(target))
        .find(|candidate| candidate.exists())
}

fn key_name(key: &str) -> String {
    match key.to_ascii_uppercase().as_str() {
        "ENTER" => "Return".to_string(),
        "ESC" => "Escape".to_string(),
        "BACKSPACE" => "BackSpace".to_string(),
        "TAB" => "Tab".to_string(),
        "SPACE" => "space".to_string(),
        "UP" => "Up".to_string(),
        "DOWN" => "Down".to_string(),
        "LEFT" => "Left".to_string(),
        "RIGHT" => "Right".to_string(),
        "HOME" => "Home".to_string(),
        "END" => "End".to_string(),
        "PAGE_UP" => "Page_Up".to_string(),
        "PAGE_DOWN" => "Page_Down".to_string(),
        "DELETE" => "Delete".to_string(),
        "XF86AUDIOPLAY" => "XF86AudioPlay".to_string(),
        "XF86AUDIONEXT" => "XF86AudioNext".to_string(),
        "XF86AUDIOPREV" => "XF86AudioPrev".to_string(),
        _ if key.len() == 1 => key.to_ascii_lowercase(),
        _ => key.to_string(),
    }
}

fn ydotool_key_codes(modifiers: &[String], key: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for modifier in modifiers {
        let code = match modifier.as_str() {
            "ctrl" => Some(29),
            "alt" => Some(56),
            "shift" => Some(42),
            "super" => Some(125),
            _ => None,
        };
        if let Some(code) = code {
            codes.push(code);
        }
    }
    let normalized = key.to_ascii_lowercase();
    let key_code = match normalized.as_str() {
        "return" | "enter" => Some(28),
        "escape" | "esc" => Some(1),
        "backspace" => Some(14),
        "tab" => Some(15),
        "space" => Some(57),
        "up" => Some(103),
        "down" => Some(108),
        "left" => Some(105),
        "right" => Some(106),
        "home" => Some(102),
        "end" => Some(107),
        "page_up" => Some(104),
        "page_down" => Some(109),
        "delete" => Some(111),
        "xf86audioplay" => Some(207),
        "xf86audionext" => Some(163),
        "xf86audioprev" => Some(165),
        _ if normalized.len() == 1 && normalized.as_bytes()[0].is_ascii_lowercase() => {
            Some(normalized.as_bytes()[0] as u16 - 96 + 29)
        }
        _ if normalized.len() == 1 && normalized.as_bytes()[0].is_ascii_digit() => {
            let digit = normalized.as_bytes()[0];
            Some(if digit == b'0' {
                11
            } else {
                (digit - b'0') as u16 + 1
            })
        }
        _ if normalized.starts_with('f') => normalized[1..]
            .parse::<u16>()
            .ok()
            .filter(|number| (1..=24).contains(number))
            .map(|number| number + 58),
        _ => None,
    };
    if let Some(key_code) = key_code {
        codes.push(key_code);
    } else if codes.is_empty() {
        return Vec::new();
    }
    let mut args: Vec<String> = codes.iter().map(|code| format!("{code}:1")).collect();
    args.extend(codes.iter().rev().map(|code| format!("{code}:0")));
    args
}

fn keyboard_help() -> String {
    "Keyboard simulation requires wtype, ydotool, or xdotool".to_string()
}
