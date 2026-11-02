#include <NimBLEDevice.h>
#include <NimBLEHIDDevice.h>
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
#define FIRMWARE_VERSION "2.7.0"
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
// Which transport carries button events. AUTO = both (the app arbitrates),
// USB/BLE = only that transport (the app still gets replies on both).
enum LinkPolicy { LINK_AUTO, LINK_USB, LINK_BLE };

// Runtime-adjustable timings (persisted in NVS).
uint16_t debounceMs = DEFAULT_DEBOUNCE_MS;
uint16_t clickTimeoutMs = DEFAULT_CLICK_TIMEOUT_MS;
uint16_t holdTimeoutMs = DEFAULT_HOLD_TIMEOUT_MS;

MacroMode currentMode = DESKTOP_MODE;
LinkPolicy currentLink = LINK_AUTO;

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

// --- Standalone HID mode ---------------------------------------------------
// The pad can work as a plain BLE keyboard when the application is not
// running. The app pushes one compact action per trigger slot; if no app is
// connected the firmware replays it itself. Actions that need the app (Home
// Assistant, long/non-ASCII text, launch-app) are refused with a 5-blink LED.
#define ACTION_TEXT_MAX 64
#define APP_PING_TIMEOUT_MS 8000

enum StoredActionType : uint8_t {
  SA_NONE = 0,
  SA_TEXT = 1,
  SA_SHORTCUT = 2,
  SA_VOL_UP = 3,
  SA_VOL_DOWN = 4,
  SA_VOL_MUTE = 5,
  SA_SCREENSHOT = 6,
  SA_LOCK = 7,
  SA_TERMINAL = 8,
  SA_APP_ONLY = 9,
  SA_HOME_ASSISTANT = 10,
  SA_MEDIA_PLAY = 11,
  SA_MEDIA_NEXT = 12,
  SA_MEDIA_PREV = 13
};

struct StoredAction {
  uint8_t type;
  uint8_t mods;
  uint8_t key;
  uint8_t len;
  char text[ACTION_TEXT_MAX];
};

NimBLEHIDDevice* pHid = nullptr;
NimBLECharacteristic* pHidKeyboard = nullptr;
NimBLECharacteristic* pHidConsumer = nullptr;

volatile bool appViaBle = false;      // the app subscribed to the TX characteristic
volatile uint16_t appBleHandle = 0xFFFF;
volatile bool hidHostReady = false;   // a host subscribed to the HID reports
unsigned long lastAppPing = 0;

// Two copies: the app fills staging and only ACTSAVE commits it, so an
// interrupted sync never leaves half a keymap on the pad.
StoredAction activeActions[2][BUTTON_COUNT][3];
StoredAction stagingActions[2][BUTTON_COUNT][3];

// Keyboard (report ID 1) + consumer control (report ID 2).
const uint8_t HID_REPORT_MAP[] = {
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x85, 0x01,
    0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00,
    0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02,
    0x95, 0x01, 0x75, 0x08, 0x81, 0x01,
    0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0x65,
    0x05, 0x07, 0x19, 0x00, 0x29, 0x65, 0x81, 0x00,
    0xC0,
    0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x02,
    0x15, 0x00, 0x26, 0xFF, 0x03, 0x19, 0x00, 0x2A,
    0xFF, 0x03, 0x75, 0x10, 0x95, 0x01, 0x81, 0x00,
    0xC0};

bool appPresent();
void loadActions();
void saveActions();
void runLocalAction(uint8_t mode, uint8_t button, uint8_t trigger);

class ServerCallbacks : public NimBLEServerCallbacks {
  void onConnect(NimBLEServer* server, NimBLEConnInfo& connInfo) override {
    // Greet the application with the identity line (best effort: a client that
    // has not subscribed yet will instead read the TX value on connect).
    bleSend(helloLine());
    // Keep advertising while connected: the OS may hold the HID link while the
    // desktop app still needs to find and attach to the custom service.
    NimBLEDevice::startAdvertising();
  }
  void onDisconnect(NimBLEServer* server, NimBLEConnInfo& connInfo, int reason) override {
    appViaBle = false;
    appBleHandle = 0xFFFF;
    hidHostReady = false;
    NimBLEDevice::startAdvertising();
  }
};

// The application marks itself present by subscribing to the TX notifications;
// the OS HID host marks itself present by subscribing to the keyboard report.
class AppLinkCallbacks : public NimBLECharacteristicCallbacks {
  void onSubscribe(NimBLECharacteristic* characteristic, NimBLEConnInfo& connInfo,
                   uint16_t subValue) override {
    if (subValue != 0) {
      appViaBle = true;
      appBleHandle = connInfo.getConnHandle();
    } else if (appBleHandle == connInfo.getConnHandle()) {
      appViaBle = false;
      appBleHandle = 0xFFFF;
    }
  }
};

class HidReportCallbacks : public NimBLECharacteristicCallbacks {
  void onSubscribe(NimBLECharacteristic* characteristic, NimBLEConnInfo& connInfo,
                   uint16_t subValue) override {
    hidHostReady = subValue != 0;
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
  uint8_t storedLink = prefs.getUChar("link", 0);
  debounceMs = prefs.getUShort("deb", DEFAULT_DEBOUNCE_MS);
  clickTimeoutMs = prefs.getUShort("clk", DEFAULT_CLICK_TIMEOUT_MS);
  holdTimeoutMs = prefs.getUShort("hld", DEFAULT_HOLD_TIMEOUT_MS);
  String stored = prefs.getString("name", DEFAULT_DEVICE_NAME);
  prefs.end();

  currentMode = (storedMode == 1) ? HOME_ASSISTANT_MODE : DESKTOP_MODE;
  currentLink = (storedLink == 1) ? LINK_USB
                 : (storedLink == 2) ? LINK_BLE : LINK_AUTO;

  stored.trim();
  if (stored.length() == 0 || stored.length() > MAX_DEVICE_NAME_LEN) {
    stored = DEFAULT_DEVICE_NAME;
  }
  deviceName = stored;
}

void saveSettings() {
  prefs.begin("mp", false);
  prefs.putUChar("mode", currentMode == HOME_ASSISTANT_MODE ? 1 : 0);
  prefs.putUChar("link", currentLink == LINK_USB ? 1 : currentLink == LINK_BLE ? 2 : 0);
  prefs.putUShort("deb", debounceMs);
  prefs.putUShort("clk", clickTimeoutMs);
  prefs.putUShort("hld", holdTimeoutMs);
  prefs.putString("name", deviceName);
  prefs.end();
}

void loadActions() {
  prefs.begin("mp", true);
  size_t stored = prefs.getBytesLength("acts");
  if (stored == sizeof(activeActions)) {
    prefs.getBytes("acts", activeActions, stored);
  } else {
    memset(activeActions, 0, sizeof(activeActions));
  }
  prefs.end();
  memcpy(stagingActions, activeActions, sizeof(activeActions));
}

void saveActions() {
  prefs.begin("mp", false);
  prefs.putBytes("acts", activeActions, sizeof(activeActions));
  prefs.end();
}

void setupBLE() {
  NimBLEDevice::init(deviceName.c_str());
  NimBLEDevice::setSecurityAuth(true, false, true);
  NimBLEDevice::setSecurityIOCap(BLE_HS_IO_NO_INPUT_OUTPUT);
  pServer = NimBLEDevice::createServer();
  pServer->setCallbacks(new ServerCallbacks());

  NimBLEService* svc = pServer->createService(SERVICE_UUID);
  pTx = svc->createCharacteristic(TX_CHAR_UUID,
                                  NIMBLE_PROPERTY::NOTIFY | NIMBLE_PROPERTY::READ);
  pTx->setCallbacks(new AppLinkCallbacks());
  NimBLECharacteristic* pRx = svc->createCharacteristic(
      RX_CHAR_UUID, NIMBLE_PROPERTY::WRITE | NIMBLE_PROPERTY::WRITE_NR);
  pRx->setCallbacks(new CommandCallbacks());
  svc->start();

  // Expose the identity line as the TX characteristic's initial value so the
  // application can read it straight after connecting.
  String hello = helloLine();
  pTx->setValue((const uint8_t*)hello.c_str(), hello.length());

  // HID keyboard + consumer control (report IDs 1 and 2). The host pairs with
  // the pad; the custom service stays available to the app on the same radio.
  pHid = new NimBLEHIDDevice(pServer);
  pHid->setManufacturer("OctoPad");
  pHid->setPnp(0x02, 0x1915, 0x5201, 0x0100);
  pHid->setHidInfo(0x00, 0x02);
  pHid->setReportMap((uint8_t*)HID_REPORT_MAP, sizeof(HID_REPORT_MAP));
  pHidKeyboard = pHid->getInputReport(1);
  pHidConsumer = pHid->getInputReport(2);
  pHidKeyboard->setCallbacks(new HidReportCallbacks());
  pHid->setBatteryLevel(100);

  NimBLEAdvertising* adv = NimBLEDevice::getAdvertising();
  adv->addServiceUUID(SERVICE_UUID);
  adv->addServiceUUID(pHid->getHidService()->getUUID());
  adv->setAppearance(HID_KEYBOARD);
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
  pHid = nullptr;
  pHidKeyboard = nullptr;
  pHidConsumer = nullptr;
  appViaBle = false;
  appBleHandle = 0xFFFF;
  hidHostReady = false;
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

bool appPresent() {
  if (appViaBle) {
    return true;
  }
#if ARDUINO_USB_CDC_ON_BOOT
  // Native USB CDC: the cable being open is a reliable sign the app is there.
  if ((bool)Serial) {
    return true;
  }
#endif
  return lastAppPing != 0 && (millis() - lastAppPing) < APP_PING_TIMEOUT_MS;
}

// ASCII -> USB HID keyboard usage (US layout). Returns false for characters
// the firmware cannot type.
bool charToHid(char c, uint8_t& mods, uint8_t& key) {
  mods = 0;
  if (c >= 'a' && c <= 'z') { key = 0x04 + (c - 'a'); return true; }
  if (c >= 'A' && c <= 'Z') { mods = 0x02; key = 0x04 + (c - 'A'); return true; }
  if (c >= '1' && c <= '9') { key = 0x1E + (c - '1'); return true; }
  if (c == '0') { key = 0x27; return true; }
  if (c == ' ') { key = 0x2C; return true; }
  if (c == '\n') { key = 0x28; return true; }
  if (c == '\t') { key = 0x2B; return true; }
  switch (c) {
    case '-': key = 0x2D; return true;
    case '=': key = 0x2E; return true;
    case '[': key = 0x2F; return true;
    case ']': key = 0x30; return true;
    case '\\': key = 0x31; return true;
    case ';': key = 0x33; return true;
    case '\'': key = 0x34; return true;
    case '`': key = 0x35; return true;
    case ',': key = 0x36; return true;
    case '.': key = 0x37; return true;
    case '/': key = 0x38; return true;
    case '!': mods = 0x02; key = 0x1E; return true;
    case '@': mods = 0x02; key = 0x1F; return true;
    case '#': mods = 0x02; key = 0x20; return true;
    case '$': mods = 0x02; key = 0x21; return true;
    case '%': mods = 0x02; key = 0x22; return true;
    case '^': mods = 0x02; key = 0x23; return true;
    case '&': mods = 0x02; key = 0x24; return true;
    case '*': mods = 0x02; key = 0x25; return true;
    case '(': mods = 0x02; key = 0x26; return true;
    case ')': mods = 0x02; key = 0x27; return true;
    case '_': mods = 0x02; key = 0x2D; return true;
    case '+': mods = 0x02; key = 0x2E; return true;
    case '{': mods = 0x02; key = 0x2F; return true;
    case '}': mods = 0x02; key = 0x30; return true;
    case '|': mods = 0x02; key = 0x31; return true;
    case ':': mods = 0x02; key = 0x33; return true;
    case '"': mods = 0x02; key = 0x34; return true;
    case '~': mods = 0x02; key = 0x35; return true;
    case '<': mods = 0x02; key = 0x36; return true;
    case '>': mods = 0x02; key = 0x37; return true;
    case '?': mods = 0x02; key = 0x38; return true;
  }
  return false;
}

void sendHidKey(uint8_t mods, uint8_t key) {
  if (pHidKeyboard == nullptr || !hidHostReady) {
    return;
  }
  uint8_t report[8] = {mods, 0, key, 0, 0, 0, 0, 0};
  pHidKeyboard->setValue(report, sizeof(report));
  pHidKeyboard->notify();
  delay(10);
  uint8_t release[8] = {0, 0, 0, 0, 0, 0, 0, 0};
  pHidKeyboard->setValue(release, sizeof(release));
  pHidKeyboard->notify();
  delay(10);
}

void sendConsumer(uint16_t usage) {
  if (pHidConsumer == nullptr || !hidHostReady) {
    return;
  }
  uint8_t report[2] = {(uint8_t)(usage & 0xFF), (uint8_t)(usage >> 8)};
  pHidConsumer->setValue(report, sizeof(report));
  pHidConsumer->notify();
  delay(20);
  uint8_t release[2] = {0, 0};
  pHidConsumer->setValue(release, sizeof(release));
  pHidConsumer->notify();
  delay(20);
}

void typeText(const char* text, uint8_t len) {
  for (uint8_t i = 0; i < len; i++) {
    uint8_t mods = 0;
    uint8_t key = 0;
    if (charToHid(text[i], mods, key)) {
      sendHidKey(mods, key);
    }
  }
}

void runLocalAction(uint8_t mode, uint8_t button, uint8_t trigger) {
  if (mode > 1 || button >= BUTTON_COUNT || trigger > 2) {
    return;
  }
  StoredAction& action = activeActions[mode][button][trigger];
  if (action.type == SA_NONE) {
    return;
  }
  // These only make sense with the app running; warn instead of doing nothing.
  if (action.type == SA_APP_ONLY || action.type == SA_HOME_ASSISTANT ||
      !hidHostReady) {
    blinkLED(5);
    return;
  }
  switch (action.type) {
    case SA_TEXT: typeText(action.text, action.len); break;
    case SA_SHORTCUT: sendHidKey(action.mods, action.key); break;
    case SA_VOL_UP: sendConsumer(0x00E9); break;
    case SA_VOL_DOWN: sendConsumer(0x00EA); break;
    case SA_VOL_MUTE: sendConsumer(0x00E2); break;
    case SA_SCREENSHOT: sendHidKey(0x02, 0x46); break;          // Shift + PrtSc
    case SA_LOCK: sendHidKey(0x08, 0x0F); break;                // GUI + L
    case SA_TERMINAL: sendHidKey(0x01 | 0x04, 0x17); break;     // Ctrl + Alt + T
    case SA_MEDIA_PLAY: sendConsumer(0x00CD); break;
    case SA_MEDIA_NEXT: sendConsumer(0x00B5); break;
    case SA_MEDIA_PREV: sendConsumer(0x00B6); break;
    default: blinkLED(5); break;
  }
}

const char* linkName() {
  return currentLink == LINK_USB ? "USB" : currentLink == LINK_BLE ? "BLE" : "AUTO";
}

void handleButtonEvent(uint8_t buttonIndex, ButtonEvent event) {
  // 1. Mode button (Button 1): a single click toggles the layer.
  if (buttonIndex == MODE_BUTTON && event == SINGLE_CLICK) {
    toggleMode();
  }

  // 2. With the app connected, forward the event to it. Otherwise replay the
  //    action table stored in NVS over BLE HID (standalone mode).
  if (!appPresent()) {
    runLocalAction(currentMode == DESKTOP_MODE ? 0 : 1, buttonIndex,
                   (uint8_t)(event - 1));
  } else {
    String modeText = (currentMode == DESKTOP_MODE) ? "DESKTOP" : "HA";
    String eventText = (event == SINGLE_CLICK) ? "SINGLE" : (event == DOUBLE_CLICK) ? "DOUBLE" : "HOLD";
    String line = String("[") + modeText + "] Button " + String(buttonIndex + 1) +
                  " -> " + eventText + " (LED Feedback Processed)";
    if (currentLink != LINK_BLE) {
      Serial.println(line);
    }
    if (currentLink != LINK_USB) {
      bleSend(line);
    }
  }

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
//   HELP                -> HELP:WHO,NAME,TIMING,MODE,LINK,SAVE,RESET,VERSION
//   NAME:<text>         -> NAME:OK:<text> | NAME:ERR:length
//   TIMING:<d>,<c>,<h>  -> TIMING:OK:<d>,<c>,<h> | TIMING:ERR
//   MODE:DESKTOP|HA     -> MODE: DESKTOP | MODE: HOME ASSISTANT
//   LINK:AUTO|USB|BLE   -> LINK:OK:<mode> | LINK:ERR
//   ACT:<m>,<b>,<t>,<type>,<mods>,<key>,<hex>  (staged, no reply)
//   ACTCLR              -> clear the staged action table (no reply)
//   ACTSAVE             -> ACTSAVED (commit staging to NVS)
//   PING                -> keeps the app marked present (no reply)
//   SAVE                -> SAVED
//   RESET               -> RESET:OK (timings + mode + link restored, name kept)
//   INFO                -> INFO:name=...,mode=...,link=...,debounce=...,click=...,hold=...
//   RESTART             -> REBOOTING (reboots the ESP32)
// ---------------------------------------------------------------------------
int hexDigit(char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  return -1;
}

uint16_t activeActionCount() {
  uint16_t count = 0;
  for (uint8_t mode = 0; mode < 2; mode++) {
    for (uint8_t button = 0; button < BUTTON_COUNT; button++) {
      for (uint8_t trigger = 0; trigger < 3; trigger++) {
        if (activeActions[mode][button][trigger].type != SA_NONE) {
          count++;
        }
      }
    }
  }
  return count;
}

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
    respond("HELP:WHO,NAME,TIMING,MODE,LINK,ACT,ACTCLR,ACTSAVE,PING,SAVE,RESET,INFO,RESTART,VERSION");
    return;
  }
  if (l.startsWith("INFO")) {
    String info = String("INFO:name=") + deviceName +
                  ",mode=" + (currentMode == DESKTOP_MODE ? "DESKTOP" : "HA") +
                  ",link=" + linkName() +
                  ",app=" + (appPresent() ? "1" : "0") +
                  ",hid=" + (hidHostReady ? "1" : "0") +
                  ",actions=" + activeActionCount() +
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
  if (l.startsWith("LINK:")) {    String target = l.substring(5);
    target.trim();
    target.toUpperCase();
    if (target == "AUTO") {
      currentLink = LINK_AUTO;
    } else if (target == "USB" || target == "WIRED") {
      currentLink = LINK_USB;
    } else if (target == "BLE" || target == "BLUETOOTH") {
      currentLink = LINK_BLE;
    } else {
      respond("LINK:ERR");
      return;
    }
    saveSettings();
    respond(String("LINK:OK:") + linkName());
    return;
  }
  if (l.startsWith("ACT:")) {
    String rest = l.substring(4);
    uint8_t fields[6] = {0, 0, 0, 0, 0, 0};
    int index = 0;
    bool ok = true;
    for (uint8_t i = 0; i < 6; i++) {
      int comma = rest.indexOf(',', index);
      if (comma < 0) { ok = false; break; }
      String part = rest.substring(index, comma);
      part.trim();
      long value = part.toInt();
      if (value < 0 || value > 255) { ok = false; break; }
      fields[i] = (uint8_t)value;
      index = comma + 1;
    }
    if (!ok || fields[0] > 1 || fields[1] >= BUTTON_COUNT || fields[2] > 2) {
      respond("ACT:ERR");
      return;
    }
    String hex = rest.substring(index);
    hex.trim();
    if (hex.length() % 2 != 0 || hex.length() > ACTION_TEXT_MAX * 2) {
      respond("ACT:ERR");
      return;
    }
    StoredAction& action = stagingActions[fields[0]][fields[1]][fields[2]];
    memset(&action, 0, sizeof(action));
    action.type = fields[3];
    action.mods = fields[4];
    action.key = fields[5];
    for (uint16_t i = 0; i + 1 < hex.length(); i += 2) {
      int hi = hexDigit(hex[i]);
      int lo = hexDigit(hex[i + 1]);
      if (hi < 0 || lo < 0) {
        respond("ACT:ERR");
        return;
      }
      action.text[action.len++] = (char)((hi << 4) | lo);
    }
    return;
  }
  if (l.startsWith("ACTCLR")) {
    memset(stagingActions, 0, sizeof(stagingActions));
    return;
  }
  if (l.startsWith("ACTSAVE")) {
    memcpy(activeActions, stagingActions, sizeof(activeActions));
    saveActions();
    respond("ACTSAVED");
    return;
  }
  if (l.startsWith("PING")) {
    lastAppPing = millis();
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
    currentLink = LINK_AUTO;
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
  loadActions();
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
