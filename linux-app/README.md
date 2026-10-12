# 🎛️ OctoPad - Native Linux Desktop Application

A native Linux Ubuntu desktop application (like **Razer Synapse, Logitech G Hub, Vial, Piper, or OpenRGB**),
built with **GTK4 + Libadwaita** (GNOME HIG).

This application is **not a daemon that keeps running in the background** — it is a standalone desktop app:
* Appears in the **Ubuntu Application Menu** (Dash / App Launcher).
* Opens a native GTK4 window with hardware acceleration.
* Automatically scans the applications installed on the Ubuntu system (`.desktop` files).
* Lets you configure keys, shortcut actions, application launchers, and audio automation.
* The serial listener runs inside the same process — actions are executed directly from Python,
  with no local HTTP server.
* When the window is closed, the entire application process shuts down cleanly.

---

## 📖 Application Pages

| Page | Contents |
| :--- | :--- |
| **Buttons** | Visual grid of 8 keycaps (index, GPIO, label, 3 trigger chips), the *Desktop / Home Assistant* mode switch, and a per-trigger action inspector (Single, Double, Hold). |
| **Serial Console** | Real-time hardware log with coloring + an input to send commands to the macropad. |
| **Profiles** | Save (Ctrl+S), import & export JSON profiles, and apply built-in presets. |
| **Settings** | Timing parameters (debounce / click / hold), device name (rename your pad), Home Assistant connection, application rescan, and application info. |

Also available in the header: the **Serial** pill (click → open console), the **HA** pill (click → connection dialog),
and the **Save** button.

---

## 🚀 Installing to the Ubuntu Application Menu

To install the application into the Ubuntu application menu (complete with icon and desktop shortcut):

```bash
cd octopad/linux-app
./install.sh
```

Once installed, you simply:
1. Press the **Super / Windows** key on your keyboard.
2. Type **"OctoPad"**.
3. Click the application icon to open it.

---

## 💻 Running Directly from the Terminal

You can also run the application directly from the terminal:
```bash
./run.sh
```
or:
```bash
python3 main.py
```

---

## 🏠 Native Home Assistant Integration (No Scripts)

The application calls Home Assistant directly through the **REST API** — no
`bash script`, `curl`, or `notify-send` wrapper scripts required.

How it works:
1. Open the application, then click the **`HA: ...`** pill in the header (or the **Connection** button on a
   *Home Assistant (Native)* action in the Key Inspector). The dialog opens **instantly** — status is read
   from cache, and a live check runs in the background.
2. Enter the **Home Assistant URL** (`http://<ip>:8123`) and a **Long-Lived Access Token**
   (HA → Profile → Security → Long-Lived Access Tokens → Create).
3. Click **Test Connection**, then **Save Connection** — the system automatically scans all interactive
   entities (light, switch, cover, media_player, etc.) from `/api/states`.
4. The **Entity Picker** opens: search / filter by domain, then click an entity to assign it
   to the button currently being edited.
5. On the button action, choose the service (`toggle`, `turn_on`, `trigger`, `press`, ...),
   then click **Test** to execute it immediately.

Technical details:
- **No hardcoding**: there are no built-in IPs, tokens, or entities in the code. All URLs/tokens
  come from user input; the Home Assistant mode button list uses generic names
  (`HA Key 1` … `HA Key 7`) until you pick your own entity.
- Credentials are stored in `~/.config/octopad/ha_config.json`
  (fallback read: `~/.config/home-assistant/env`, format `HA_URL=` / `HA_TOKEN=`).
- Entity scan results are cached in `~/.config/octopad/ha_entities_cache.json`
  so the list stays available even while HA is offline (`live: false`).
- Module: [`home_assistant.py`](home_assistant.py) (`HomeAssistantClient`) — called
  **directly** from the UI and listener; no local HTTP endpoint is set up.
- Domain/entity icons use GTK/Adwaita theme icons with automatic fallback
  (the UI is emoji-free).
- Every HA action execution sends a desktop notification via `notify-send`.
- If HA is unreachable, the entity list falls back to the last on-disk cache; rescans
  are held back for 30 seconds so UI responses stay instant.

---

## 🔌 Serial Console

* Logs flow in automatically from the listener (`[HARDWARE]`, `[ACTION]`, `[TX]`, errors).
* Click the **Serial** pill in the header to go to the console.
* Type a command (e.g. `CMD:PING`) then press **Enter / Send** to send it
  to the macropad through the currently open port.
* Ports are discovered automatically in `/dev/ttyACM*` and `/dev/ttyUSB*` (115200 baud),
  with auto-reconnect if the cable is unplugged.

---


## ⌨️ Shortcut & Text Simulation (Keyboard Injection)

The `Shortcut` and `Text` actions require a keyboard simulation tool to send input to the active window.
Because GNOME Wayland restricts input injection, the application uses the following automatic priority order:

1. `wtype` (recommended on modern Wayland, wlroots-based)
2. `ydotool` (uinput, works in most Wayland/GNOME sessions — requires a daemon)
3. `xdotool` (X11 only)

### Installation (Ubuntu)

```bash
# Option A – wtype
sudo apt install wtype

# Option B – ydotool (recommended on GNOME Wayland 44+)
sudo apt install ydotool
# Run the uinput daemon
systemctl --user enable --now ydotool
# or: sudo ydotoold &

# Option C – xdotool (X11 / XWayland only)
sudo apt install xdotool
```

**ydotool note**: some versions require uinput access; make sure `ydotoold` is running so shortcuts/typing work.


## 🗑️ Uninstalling

To remove the application from the Ubuntu application menu and clean up shortcuts and icons:

```bash
cd octopad/linux-app
./uninstall.sh
```