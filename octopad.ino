#include <NimBLEDevice.h>
#include <Preferences.h>

// ---------------------------------------------------------------------------
// Layer BLE: custom service for sending button events to the application. All
// actions (shortcuts, text, media, Home Assistant, launch app, bash) are
// executed BY THE APPLICATION — whether over USB serial or over BLE — so that
// cable and Bluetooth modes stay consistent. The firmware only sends text lines
// identical to those the application used to parse from serial.
//
// The same command protocol is accepted on both transports: the application can
// talk to the pad over USB serial or by writing to the RX characteristic.
//
// The UUIDs below are unique to this project: the first four octets of the
// service UUID spell "octo" in ASCII, so the application can recognise the pad
// from its Bluetooth advertisement regardless of the user-configured name.
// ---------------------------------------------------------------------------
#define SERVICE_UUID "6f63746f-7061-4430-8001-000000000001"
#define TX_CHAR_UUID "6f63746f-7061-4430-8001-000000000002"
#define RX_CHAR_UUID "6f63746f-7061-4430-8001-000000000003"

// Identity / naming. The brand tag is what the application uses to confirm it
// is an OctoPad and learn the user-configured name. The name lives in NVS and
// can be anything — the app finds the pad by its OctoPad signature, not a name.
#define FIRMWARE_VERSION "2.6"
#define DEVICE_BRAND "OCTOPAD"
#define DEFAULT_DEVICE_NAME "OctoPad"
#define MAX_DEVICE_NAME_LEN 24

// Timing defaults. These can be changed at runtime by the application with the
// `TIMING:<debounce>,<click>,<hold>` command and are persisted in NVS.
#define DEFAULT_DEBOUNCE_MS 25
#define DEFAULT_CLICK_TIMEOUT_MS 250
#define DEFAULT_HOLD_TIMEOUT_MS 450

const uint8_t buttonPins[] = {20, 9, 2, 1, 21, 10, 3, 0};
const uint8_t BUTTON_COUNT = sizeof(buttonPins) / sizeof(buttonPins[0]);
const uint8_t MODE_BUTTON = 0;  // Button 1 toggles the layer
const uint8_t LED_PIN = 4;      // On-board LED (active-high)

enum MacroMode { DESKTOP_MODE, HOME_ASSISTANT_MODE };
enum ButtonEvent { NONE, SINGLE_CLICK, DOUBLE_CLICK, HOLD };

// Runtime-adjustable timings (persisted in NVS).
uint16_t debounceMs = DEFAULT_DEBOUNCE_MS;
uint16_t clickTimeoutMs = DEFAULT_CLICK_TIMEOUT_MS;
uint16_t holdTimeoutMs = DEFAULT_HOLD_TIMEOUT_MS;

MacroMode currentMode = DESKTOP_MODE;

String deviceName = DEFAULT_DEVICE_NAME;

// Non-blocking LED feedback state machine.
uint8_t ledTogglesRemaining = 0;
unsigned long ledNextToggleAt = 0;

NimBLEServer* pServer = nullptr;
NimBLECharacteristic* pTx = nullptr;
Preferences prefs;

struct Button {
  uint8_t pin;
  bool lastRawState;
  bool stableState;
  unsigned long lastDebounceTime;

  uint8_t clickCount;
  unsigned long lastReleaseTime;  // Button release time (double-click detection direction)
  unsigned long lastPressTime;    // Button press time (HOLD detection direction)
  bool holdReported;
  bool isPressed;
};

Button buttons[BUTTON_COUNT];

// Identity line sent over serial and Bluetooth so the application can confirm
// the device is an OctoPad and learn the user-configured name:
//   HELLO:OCTOPAD:<device-name>:v<firmware-version>
String helloLine();
void bleSend(const String& s);
void applyDeviceName();
void processCommand(const String& l);
void blinkLED(uint8_t count);

class ServerCallbacks : public NimBLEServerCallbacks {
  void onConnect(NimBLEServer* server, NimBLEConnInfo& connInfo) override {
    // Greet the application with the identity line (best effort: a client that
    // has not subscribed yet will instead read the TX value on connect).
    bleSend(helloLine());
  }
  void onDisconnect(NimBLEServer* server, NimBLEConnInfo& connInfo, int reason) override {
    NimBLEDevice::startAdvertising();
  }
};

// Commands can also be sent over Bluetooth by writing to the RX characteristic.
class CommandCallbacks : public NimBLECharacteristicCallbacks {
  void onWrite(NimBLECharacteristic* characteristic, NimBLEConnInfo& connInfo) override {
    String value = String(characteristic->getValue().c_str());
    value.trim();
    if (value.length() > 0) {
      processCommand(value);
    }
  }
};

String helloLine() {
  return String("HELLO:") + DEVICE_BRAND + ":" + deviceName + ":v" + FIRMWARE_VERSION;
}

// Reply on both transports; whichever link delivered the command (USB or BLE)
// gets the answer, the other one simply ignores it.
void respond(const String& s) {
  Serial.println(s);
  bleSend(s);
}

void loadSettings() {
  prefs.begin("mp", true);
  uint8_t storedMode = prefs.getUChar("mode", 0);
  debounceMs = prefs.getUShort("deb", DEFAULT_DEBOUNCE_MS);
  clickTimeoutMs = prefs.getUShort("clk", DEFAULT_CLICK_TIMEOUT_MS);
  holdTimeoutMs = prefs.getUShort("hld", DEFAULT_HOLD_TIMEOUT_MS);
  String stored = prefs.getString("name", DEFAULT_DEVICE_NAME);
  prefs.end();

  currentMode = (storedMode == 1) ? HOME_ASSISTANT_MODE : DESKTOP_MODE;

  stored.trim();
  if (stored.length() == 0 || stored.length() > MAX_DEVICE_NAME_LEN) {
    stored = DEFAULT_DEVICE_NAME;
  }
  deviceName = stored;
}

void saveSettings() {
  prefs.begin("mp", false);
  prefs.putUChar("mode", currentMode == HOME_ASSISTANT_MODE ? 1 : 0);
  prefs.putUShort("deb", debounceMs);
  prefs.putUShort("clk", clickTimeoutMs);
  prefs.putUShort("hld", holdTimeoutMs);
  prefs.putString("name", deviceName);
  prefs.end();
}

void setupBLE() {
  NimBLEDevice::init(deviceName.c_str());
  pServer = NimBLEDevice::createServer();
  pServer->setCallbacks(new ServerCallbacks());

  NimBLEService* svc = pServer->createService(SERVICE_UUID);
  pTx = svc->createCharacteristic(TX_CHAR_UUID,
                                  NIMBLE_PROPERTY::NOTIFY | NIMBLE_PROPERTY::READ);
  NimBLECharacteristic* pRx = svc->createCharacteristic(
      RX_CHAR_UUID, NIMBLE_PROPERTY::WRITE | NIMBLE_PROPERTY::WRITE_NR);
  pRx->setCallbacks(new CommandCallbacks());
  svc->start();

  // Expose the identity line as the TX characteristic's initial value so the
  // application can read it straight after connecting.
  String hello = helloLine();
  pTx->setValue((const uint8_t*)hello.c_str(), hello.length());

  NimBLEAdvertising* adv = NimBLEDevice::getAdvertising();
  adv->addServiceUUID(SERVICE_UUID);
  adv->enableScanResponse(true);
  NimBLEDevice::startAdvertising();
}

// Re-initialise BLE so a newly-set device name is advertised immediately.
// This drops any active connection (the name lives in the GAP layer).
void applyDeviceName() {
  NimBLEDevice::stopAdvertising();
  NimBLEDevice::deinit(true);
  pTx = nullptr;
  pServer = nullptr;
  setupBLE();
}

// Send the same event line to the application over Bluetooth (if a client is connected).
void bleSend(const String& s) {
  if (pTx != nullptr && pServer != nullptr &&
      pServer->getConnectedCount() > 0) {
    pTx->setValue((const uint8_t*)s.c_str(), s.length());
    pTx->notify();
  }
}

void setMode(MacroMode mode, bool persist) {
  currentMode = mode;
  digitalWrite(LED_PIN, (mode == DESKTOP_MODE) ? HIGH : LOW);
  if (persist) {
    saveSettings();
  }
}

void toggleMode() {
  setMode((currentMode == DESKTOP_MODE) ? HOME_ASSISTANT_MODE : DESKTOP_MODE, true);
  respond((currentMode == DESKTOP_MODE) ? "MODE: DESKTOP" : "MODE: HOME ASSISTANT");
}

// Start a non-blocking blink: the LED keeps blinking 80 ms on / 80 ms off while
// the main loop stays free to keep debouncing and dispatching button events.
void blinkLED(uint8_t count) {
  ledTogglesRemaining = count * 2;
  ledNextToggleAt = millis();
}

void updateLED() {
  if (ledTogglesRemaining == 0) {
    return;
  }
  unsigned long now = millis();
  if ((long)(now - ledNextToggleAt) < 0) {
    return;
  }
  bool base = (currentMode == DESKTOP_MODE);
  digitalWrite(LED_PIN, (ledTogglesRemaining % 2 == 0) ? !base : base);
  ledTogglesRemaining--;
  ledNextToggleAt = now + 80;
  if (ledTogglesRemaining == 0) {
    digitalWrite(LED_PIN, base);
  }
}

void handleButtonEvent(uint8_t buttonIndex, ButtonEvent event) {
  // 1. Mode button (Button 1): a single click toggles the layer.
  if (buttonIndex == MODE_BUTTON && event == SINGLE_CLICK) {
    toggleMode();
  }

  // 2. Emit the event line to both transports (identical format for the app).
  String modeText = (currentMode == DESKTOP_MODE) ? "DESKTOP" : "HA";
  String eventText = (event == SINGLE_CLICK) ? "SINGLE" : (event == DOUBLE_CLICK) ? "DOUBLE" : "HOLD";
  String line = String("[") + modeText + "] Button " + String(buttonIndex + 1) +
                " -> " + eventText + " (LED Feedback Processed)";
  Serial.println(line);
  bleSend(line);

  // 3. Global LED feedback: double = 2 blinks, hold = 3 blinks, single = none.
  if (event == DOUBLE_CLICK) {
    blinkLED(2);
  } else if (event == HOLD) {
    blinkLED(3);
  }
}

// ---------------------------------------------------------------------------
// Command protocol (USB serial + BLE RX characteristic)
//   WHO                 -> HELLO:OCTOPAD:<name>:v<version>
//   VERSION             -> VERSION:OCTOPAD:<version>
//   HELP                -> HELP:WHO,NAME,TIMING,MODE,SAVE,RESET,VERSION
//   NAME:<text>         -> NAME:OK:<text> | NAME:ERR:length
//   TIMING:<d>,<c>,<h>  -> TIMING:OK:<d>,<c>,<h> | TIMING:ERR
//   MODE:DESKTOP|HA     -> MODE: DESKTOP | MODE: HOME ASSISTANT
//   SAVE                -> SAVED
//   RESET               -> RESET:OK (timings + mode restored, name kept)
//   INFO                -> INFO:name=...,mode=...,debounce=...,click=...,hold=...
//   RESTART             -> REBOOTING (reboots the ESP32)
// ---------------------------------------------------------------------------
void processCommand(const String& l) {
  if (l.startsWith("WHO")) {
    respond(helloLine());
    return;
  }
  if (l.startsWith("VERSION")) {
    respond(String("VERSION:") + DEVICE_BRAND + ":" + FIRMWARE_VERSION);
    return;
  }
  if (l.startsWith("HELP")) {
    respond("HELP:WHO,NAME,TIMING,MODE,SAVE,RESET,INFO,RESTART,VERSION");
    return;
  }
  if (l.startsWith("INFO")) {
    String info = String("INFO:name=") + deviceName +
                  ",mode=" + (currentMode == DESKTOP_MODE ? "DESKTOP" : "HA") +
                  ",debounce=" + debounceMs + ",click=" + clickTimeoutMs +
                  ",hold=" + holdTimeoutMs + ",version=" + FIRMWARE_VERSION;
    respond(info);
    return;
  }
  if (l.startsWith("NAME:")) {
    String newName = l.substring(5);
    newName.trim();
    if (newName.length() > 0 && newName.length() <= MAX_DEVICE_NAME_LEN) {
      deviceName = newName;
      saveSettings();
      respond(String("NAME:OK:") + newName);
      applyDeviceName();
    } else {
      respond("NAME:ERR:length");
    }
    return;
  }
  if (l.startsWith("TIMING:")) {
    String rest = l.substring(7);
    int c1 = rest.indexOf(',');
    int c2 = rest.indexOf(',', c1 + 1);
    if (c1 > 0 && c2 > c1) {
      long d = strtol(rest.substring(0, c1).c_str(), nullptr, 10);
      long c = strtol(rest.substring(c1 + 1, c2).c_str(), nullptr, 10);
      long h = strtol(rest.substring(c2 + 1).c_str(), nullptr, 10);
      if (d >= 0 && d <= 200 && c >= 50 && c <= 2000 && h >= 100 && h <= 3000) {
        debounceMs = (uint16_t)d;
        clickTimeoutMs = (uint16_t)c;
        holdTimeoutMs = (uint16_t)h;
        saveSettings();
        respond(String("TIMING:OK:") + d + "," + c + "," + h);
        return;
      }
    }
    respond("TIMING:ERR");
    return;
  }
  if (l.startsWith("MODE:")) {
    String target = l.substring(5);
    target.trim();
    target.toUpperCase();
    if (target == "DESKTOP" || target == "HA" ||
        target == "HOME ASSISTANT" || target == "HOME_ASSISTANT") {
      bool wantHa = (target == "HA" || target == "HOME ASSISTANT" ||
                     target == "HOME_ASSISTANT");
      setMode(wantHa ? HOME_ASSISTANT_MODE : DESKTOP_MODE, true);
      respond(wantHa ? "MODE: HOME ASSISTANT" : "MODE: DESKTOP");
    } else {
      respond("MODE:ERR");
    }
    return;
  }
  if (l.startsWith("SAVE")) {
    saveSettings();
    respond("SAVED");
    return;
  }
  if (l.startsWith("RESET")) {
    debounceMs = DEFAULT_DEBOUNCE_MS;
    clickTimeoutMs = DEFAULT_CLICK_TIMEOUT_MS;
    holdTimeoutMs = DEFAULT_HOLD_TIMEOUT_MS;
    setMode(DESKTOP_MODE, false);
    saveSettings();
    respond("RESET:OK");
    return;
  }
  if (l.startsWith("RESTART")) {
    respond("REBOOTING");
    delay(100);
    ESP.restart();
    return;
  }
  respond("ERR:UNKNOWN");
}

void handleCmd() {
  if (!Serial.available()) {
    return;
  }
  String l = Serial.readStringUntil('\n');
  l.trim();
  if (l.length() > 0) {
    processCommand(l);
  }
}

void setup() {
  Serial.begin(115200);
  delay(400);

  pinMode(LED_PIN, OUTPUT);

  for (uint8_t i = 0; i < BUTTON_COUNT; i++) {
    pinMode(buttonPins[i], INPUT_PULLUP);
    bool initReading = digitalRead(buttonPins[i]);

    buttons[i].pin = buttonPins[i];
    buttons[i].lastRawState = initReading;
    buttons[i].stableState = initReading;
    buttons[i].lastDebounceTime = 0;
    buttons[i].clickCount = 0;
    buttons[i].lastReleaseTime = 0;
    buttons[i].lastPressTime = 0;
    buttons[i].holdReported = false;
    buttons[i].isPressed = false;
  }

  loadSettings();
  digitalWrite(LED_PIN, (currentMode == DESKTOP_MODE) ? HIGH : LOW);
  setupBLE();
  Serial.print("BLE ");
  Serial.print(deviceName);
  Serial.println(" Ready");
  Serial.println(helloLine());

  // Reset the board automatically if the main loop ever hangs.
  enableLoopWDT();
}

void loop() {
  handleCmd();
  updateLED();
  unsigned long now = millis();

  for (uint8_t i = 0; i < BUTTON_COUNT; i++) {
    bool rawReading = digitalRead(buttons[i].pin);

    // Debounce filter
    if (rawReading != buttons[i].lastRawState) {
      buttons[i].lastDebounceTime = now;
      buttons[i].lastRawState = rawReading;
    }

    if ((now - buttons[i].lastDebounceTime) >= debounceMs) {
      if (rawReading != buttons[i].stableState) {
        buttons[i].stableState = rawReading;

        if (buttons[i].stableState == LOW) {
          buttons[i].isPressed = true;
          buttons[i].lastPressTime = now;
        } else {
          buttons[i].isPressed = false;
          if (!buttons[i].holdReported) {
            buttons[i].clickCount++;
            buttons[i].lastReleaseTime = now;
          } else {
            buttons[i].holdReported = false;
          }
        }
      }
    }

    // HOLD evaluation
    if (buttons[i].isPressed && !buttons[i].holdReported) {
      if (now - buttons[i].lastPressTime >= holdTimeoutMs) {
        buttons[i].holdReported = true;
        buttons[i].clickCount = 0;
        handleButtonEvent(i, HOLD);
      }
    }

    // Single / double click evaluation
    if (!buttons[i].isPressed) {
      if (buttons[i].clickCount == 1 && (now - buttons[i].lastReleaseTime >= clickTimeoutMs)) {
        handleButtonEvent(i, SINGLE_CLICK);
        buttons[i].clickCount = 0;
      }
      else if (buttons[i].clickCount >= 2) {
        handleButtonEvent(i, DOUBLE_CLICK);
        buttons[i].clickCount = 0;
      }
    }
  }

  // Keep the task watchdog happy (enabled at the end of setup()).
  feedLoopWDT();
  delay(1);
}
