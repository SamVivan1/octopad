//! Running external commands for action execution.
//!
//! `RealCommandRunner` is the production implementation (`std::process`); it
//! mirrors the reason strings of `linux-app/system_actions.py:_run` so the
//! console lines stay identical. `RecordingRunner` is the test double: it
//! records every call and replays scripted outcomes, which is what lets the
//! dispatcher be tested without spawning a process.

use std::io;
use std::process::{Command, Stdio};

use super::ActionOutcome;

/// One command invocation, as handed to (and recorded by) a [`CommandRunner`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCall {
    pub program: String,
    pub args: Vec<String>,
    /// `true` for `spawn_detached` (fire and forget), `false` for `run`.
    pub detached: bool,
}

impl CommandCall {
    /// A `spawn_detached` call.
    pub fn detached(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> CommandCall {
        CommandCall {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            detached: true,
        }
    }

    /// A `run` call.
    pub fn waited(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> CommandCall {
        CommandCall {
            detached: false,
            ..CommandCall::detached(program, args)
        }
    }
}

/// Runs commands for real. Cheap to clone: it holds no state.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealCommandRunner;

impl RealCommandRunner {
    pub const fn new() -> RealCommandRunner {
        RealCommandRunner
    }
}

impl super::CommandRunner for RealCommandRunner {
    /// Blocks until the command exits; the outcome mirrors Python's `_run()`.
    fn run(&self, program: &str, args: &[&str]) -> ActionOutcome {
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output();
        match output {
            Ok(output) => {
                if output.status.success() {
                    // `_run()` reports `" ".join(cmd[:3])` on success.
                    let mut words: Vec<&str> = Vec::with_capacity(3);
                    words.push(program);
                    words.extend(args.iter().take(2).copied());
                    return ActionOutcome::ok(words.join(" "));
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let detail = if stderr.is_empty() { stdout } else { stderr };
                match first_line(&detail) {
                    Some(line) => ActionOutcome::fail(line.to_string()),
                    None => ActionOutcome::fail(match output.status.code() {
                        Some(code) => format!("{program} exit {code}"),
                        None => format!("{program} terminated by a signal"),
                    }),
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                ActionOutcome::fail(format!("{program} not found"))
            }
            Err(err) => ActionOutcome::fail(err.to_string()),
        }
    }

    /// Starts a command in its own session/group and does not wait for it, so
    /// quitting the app cannot kill it and it cannot hold the process alive.
    fn spawn_detached(&self, program: &str, args: &[&str]) -> ActionOutcome {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        detach(&mut command);
        match command.spawn() {
            Ok(_) => {
                let mut words: Vec<&str> = Vec::with_capacity(args.len() + 1);
                words.push(program);
                words.extend(args.iter().copied());
                ActionOutcome::ok(words.join(" "))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                ActionOutcome::fail(format!("{program} not found"))
            }
            Err(err) => ActionOutcome::fail(err.to_string()),
        }
    }
}

/// Python's `start_new_session=True` (`setsid`) / Windows' detached process.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// Python's `(stderr or stdout or "").strip().splitlines()[0]`.
fn first_line(stream: &str) -> Option<&str> {
    let trimmed = stream.trim();
    if trimmed.is_empty() {
        None
    } else {
        trimmed.lines().next()
    }
}

/// Test double: records every call and replays scripted outcomes.
///
/// Never spawns anything, which is what keeps the dispatcher tests instant and
/// platform independent. Cloneable, so a chain runner can share it.
#[derive(Clone)]
pub struct RecordingRunner {
    state: std::sync::Arc<std::sync::Mutex<RecordingState>>,
}

#[derive(Default)]
struct RecordingState {
    calls: Vec<CommandCall>,
    outcomes: Vec<ActionOutcome>,
}

impl RecordingRunner {
    /// Every call replays the same outcome.
    pub fn new(outcome: ActionOutcome) -> RecordingRunner {
        RecordingRunner::scripted(vec![outcome])
    }

    /// Calls replay `outcomes` in order; once the script is exhausted its last
    /// outcome repeats, so a short script never panics mid-suite.
    pub fn scripted(outcomes: Vec<ActionOutcome>) -> RecordingRunner {
        RecordingRunner {
            state: std::sync::Arc::new(std::sync::Mutex::new(RecordingState {
                calls: Vec::new(),
                outcomes,
            })),
        }
    }

    /// Every call recorded so far, in order.
    pub fn calls(&self) -> Vec<CommandCall> {
        self.state.lock().unwrap().calls.clone()
    }

    pub fn call_count(&self) -> usize {
        self.state.lock().unwrap().calls.len()
    }

    pub fn last_call(&self) -> Option<CommandCall> {
        self.state.lock().unwrap().calls.last().cloned()
    }

    fn record(&self, program: &str, args: &[&str], detached: bool) -> ActionOutcome {
        let mut state = self.state.lock().unwrap();
        state.calls.push(CommandCall {
            program: program.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            detached,
        });
        let index = state.calls.len() - 1;
        let scripted = &state.outcomes;
        if scripted.is_empty() {
            return ActionOutcome::fail("RecordingRunner: no scripted outcome");
        }
        scripted[index.min(scripted.len() - 1)].clone()
    }
}

impl super::CommandRunner for RecordingRunner {
    fn run(&self, program: &str, args: &[&str]) -> ActionOutcome {
        self.record(program, args, false)
    }

    fn spawn_detached(&self, program: &str, args: &[&str]) -> ActionOutcome {
        self.record(program, args, true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::CommandRunner;
    use super::*;

    fn ok() -> ActionOutcome {
        ActionOutcome::ok("ran")
    }

    #[test]
    fn real_runner_runs_a_process_and_reports_the_command() {
        let (program, args): (&str, &[&str]) = if cfg!(target_os = "windows") {
            ("cmd", &["/C", "echo", "octo"])
        } else {
            ("/bin/sh", &["-c", "echo octo"])
        };
        let outcome = RealCommandRunner::new().run(program, args);
        assert!(outcome.ok, "unexpected failure: {outcome:?}");
        // `_run()` truncates the command line to its first three words.
        assert_eq!(outcome.reason, [program, args[0], args[1]].join(" "));
    }

    #[test]
    fn real_runner_reports_a_missing_program() {
        let outcome = RealCommandRunner::new().run("octopad-does-not-exist", &[]);
        assert!(!outcome.ok);
        assert_eq!(outcome.reason, "octopad-does-not-exist not found");
    }

    #[test]
    fn real_runner_reports_the_first_stderr_line_on_failure() {
        let (program, args): (&str, &[&str]) = if cfg!(target_os = "windows") {
            ("cmd", &["/C", "echo boom 1>&2 & exit 3"])
        } else {
            ("/bin/sh", &["-c", "echo boom >&2; exit 3"])
        };
        let outcome = RealCommandRunner::new().run(program, args);
        assert!(!outcome.ok);
        assert_eq!(outcome.reason, "boom");
    }

    #[test]
    fn real_runner_reports_the_exit_code_without_output() {
        let (program, args): (&str, &[&str]) = if cfg!(target_os = "windows") {
            ("cmd", &["/C", "exit 3"])
        } else {
            ("/bin/sh", &["-c", "exit 3"])
        };
        let outcome = RealCommandRunner::new().run(program, args);
        assert!(!outcome.ok);
        assert_eq!(outcome.reason, format!("{program} exit 3"));
    }

    #[test]
    fn real_runner_spawns_a_detached_process() {
        let (program, args): (&str, &[&str]) = if cfg!(target_os = "windows") {
            ("cmd", &["/C", "echo", "detached"])
        } else {
            ("/bin/sh", &["-c", "echo detached"])
        };
        let outcome = RealCommandRunner::new().spawn_detached(program, args);
        assert!(outcome.ok, "unexpected failure: {outcome:?}");
        assert_eq!(outcome.reason, format!("{program} {}", args.join(" ")));
    }

    #[test]
    fn real_runner_reports_a_missing_program_for_spawns() {
        let outcome = RealCommandRunner::new().spawn_detached("octopad-does-not-exist", &[]);
        assert!(!outcome.ok);
        assert_eq!(outcome.reason, "octopad-does-not-exist not found");
    }

    #[test]
    fn recording_runner_records_and_replays() {
        let runner = RecordingRunner::scripted(vec![
            ActionOutcome::fail("first failed"),
            ActionOutcome::ok("second worked"),
        ]);
        let runner2 = runner.clone();
        assert!(!runner.run("a", &["1"]).ok);
        assert_eq!(
            runner.run("b", &["2", "3"]),
            ActionOutcome::ok("second worked")
        );
        // The script is exhausted: the last outcome repeats.
        assert!(runner.run("c", &[]).ok);
        assert_eq!(
            runner2.calls(),
            vec![
                CommandCall::waited("a", vec!["1"]),
                CommandCall::waited("b", vec!["2", "3"]),
                CommandCall::waited("c", Vec::<String>::new()),
            ]
        );
        assert_eq!(runner2.call_count(), 3);
        assert_eq!(
            runner2.last_call(),
            Some(CommandCall::waited("c", Vec::<String>::new()))
        );
    }

    #[test]
    fn recording_runner_without_a_script_fails_loudly() {
        let runner = RecordingRunner::scripted(Vec::new());
        let outcome = runner.run("a", &[]);
        assert!(!outcome.ok);
        assert_eq!(outcome.reason, "RecordingRunner: no scripted outcome");
    }

    #[test]
    fn recording_runner_marks_detached_calls() {
        let runner = RecordingRunner::new(ok());
        let outcome = runner.spawn_detached("sh", &["-c", "x"]);
        assert!(outcome.ok);
        assert_eq!(
            runner.calls(),
            vec![CommandCall::detached("sh", vec!["-c", "x"])]
        );
    }
}
