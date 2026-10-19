# M3.1 notes — action execution

Milestone: pressing a button now *does* something. Until now (M0–M2) a button press
was parsed and logged; the dispatcher did not exist.

## What works now

| Action type | Status | Notes |
| --- | --- | --- |
| `none` | ✅ skipped | never executed, never logged as an action |
| `mode_toggle` | ✅ | reports that the firmware owns the LED |
| `delay` | ✅ | 0–600 s, clamped, runs off the UI thread |
| `shortcut` | ✅ Linux | wtype → ydotool → xdotool |
| `shortcut` | ⚠️ Windows | compiles and type-checks for Windows; **the SendInput call has never run** — Windows CI is the first execution |
| `text` | ✅ Linux / ⚠️ Windows | same split as above |
| `media` | ✅ | wpctl → pactl → playerctl → XF86 on Linux; VK media keys on Windows |
| `bash_script` | ✅ | `/bin/sh -c` detached (`cmd.exe /C` on Windows) |
| `system_action` | ✅ | the ten built-in presets |
| `audio_output`, `launch_app`, `home_assistant`, `ha_service`, `ha_webhook` | ❌ **M3.2** | fail loudly with "Not available until M3.2: <type>" — never a silent success |

## Log format (matches the Python apps)

```
Executing action for Button 3 (single) -> Type: shortcut
[ACTION] B3 (SINGLE): Shortcut sent [OK]
[ACTION] B3 (SINGLE): Target key not selected yet [FAIL]
```

`[FAIL]` renders red in the console, `[OK]` as a normal system line.

## Two silent-failure bugs found and fixed during M3.1

1. **Microphone mute.** The `pactl` fallback used `@DEFAULT_AUDIO_SOURCE@`, which is
   the *wpctl* alias. Verified on this machine against PulseAudio on PipeWire 1.6.8:
   ```
   pactl get-source-mute @DEFAULT_SOURCE@        -> Mute: yes
   pactl get-source-mute @DEFAULT_AUDIO_SOURCE@  -> No such entity
   ```
   Mic mute would have failed without a single message. Now pinned by a test whose
   comment carries the transcript.
2. **Failed actions looked fine.** The console classified errors with
   `includes(' FAIL]')`, which does not match `[ACTION] … [FAIL]` (a `[` sits between
   the space and `FAIL`). A failed action rendered as ordinary grey text. The legacy
   Python console has the same blind spot; here it is fixed.

## Known limitation

A chain sitting in a `delay` keeps its blocking task (and the shutdown sequence)
alive until the delay expires — the `Sleeper` abstraction has no cancellation
channel. Tracked for M3.2.

## Manual smoke checklist (owner, real pad)

The automated tests assert the commands that get built, never that keystrokes reach
another application. This is the part only a human can prove.

**Prerequisite — pick an injector.** Without one of these, every `shortcut`/`text`
fails with the help text:

```bash
command -v wtype || command -v ydotool || command -v xdotool
# none installed?  sudo apt install ydotool && sudo ydotoold &   (or xdotool on X11)
```

Then, with the app running and the pad connected:

- [ ] Bind key 1 / single → `shortcut` `ctrl+alt+t`, press it. A terminal opens.
- [ ] Bind key 2 / single → `text` `hello octopad`, focus a text field, press it.
- [ ] Bind key 3 / single → a two-action chain: `text` "octopad" then `delay` 1.5 s
      then `shortcut` `Return`. The Enter must arrive ~1.5 s later.
- [ ] Bind key 4 / single → `system_action` `volume_up_5`, press it three times;
      the volume rises in 5% steps. Check the console says `Volume +5% via wpctl`.
- [ ] Bind key 5 / single → `media_play_pause`, press it; playback pauses.
- [ ] Bind key 6 / single → `bash_script` with `notify-send test hello`, press it;
      a notification appears.
- [ ] Confirm a *deliberately broken* action (e.g. `text` with an empty string)
      shows a red `[FAIL]` line in the console and does not stop the rest of its chain.
- [ ] Confirm the window stays responsive during a 5 s `delay` action.
- [ ] With `wtype`/`ydotool`/`xdotool` all uninstalled, confirm the shortcut fails
      with the help text instead of silently doing nothing.

**Windows:** the same list, after the CI artifact is installed. Expect the four
Windows-gated tests to be the first execution of the SendInput path — report
anything that misbehaves there rather than assuming it matches Linux.
