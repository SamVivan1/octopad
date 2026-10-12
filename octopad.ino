#include <NimBLEDevice.h>
#include <Preferences.h>

// ---------------------------------------------------------------------------
// USB HID keyboard usage IDs (standard HID keycodes).
// Earlier versions pulled these from <HijelHID_BLEKeyboard.h>; the library was
// later dropped in favour of NimBLE. The constants are kept because the config
// arrays below are still stored/sent for SET/SAVE compatibility, and they must
// match the keycodes produced by firmware_sync.py on the application side.
// ---------------------------------------------------------------------------
#define KEY_A 0x04
#define KEY_B 0x05
#define KEY_C 0x06
#define KEY_D 0x07
#define KEY_E 0x08
#define KEY_F 0x09
#define KEY_G 0x0A
#define KEY_H 0x0B
#define KEY_I 0x0C
#define KEY_J 0x0D
#define KEY_K 0x0E
#define KEY_L 0x0F
#define KEY_M 0x10
#define KEY_N 0x11
#define KEY_O 0x12
#define KEY_P 0x13
#define KEY_Q 0x14
#define KEY_R 0x15
#define KEY_S 0x16
#define KEY_T 0x17
#define KEY_U 0x18
#define KEY_V 0x19
#define KEY_W 0x1A
#define KEY_X 0x1B
#define KEY_Y 0x1C
#define KEY_Z 0x1D
#define KEY_1 0x1E
#define KEY_2 0x1F
#define KEY_3 0x20
#define KEY_4 0x21
#define KEY_5 0x22
#define KEY_6 0x23
#define KEY_7 0x24
#define KEY_8 0x25
#define KEY_9 0x26
#define KEY_0 0x27
#define KEY_F1  0x3A
#define KEY_F2  0x3B
#define KEY_F3  0x3C
#define KEY_F4  0x3D
#define KEY_F5  0x3E
#define KEY_F6  0x3F
#define KEY_F7  0x40
#define KEY_F8  0x41
#define KEY_F9  0x42
#define KEY_F10 0x43
#define KEY_F11 0x44
#define KEY_F12 0x45

// ---------------------------------------------------------------------------
// Layer BLE: custom service for sending button events to the application. All
// actions (shortcuts, text, media, Home Assistant, launch app, bash) are
// executed BY THE APPLICATION — whether over USB serial or over BLE — so that
// cable and Bluetooth modes stay consistent. The firmware only sends text lines
// identical to those the application used to parse from serial.
//
// The UUIDs below are unique to this project: the first four octets of the
// service UUID spell "octo" in ASCII, so the application can recognise the pad
// from its Bluetooth advertisement regardless of the user-configured name.
// ---------------------------------------------------------------------------
#define SERVICE_UUID "6f63746f-7061-4430-8001-000000000001"
#define TX_CHAR_UUID "6f63746f-7061-4430-8001-000000000002"
#define RX_CHAR_UUID "6f63746f-7061-4430-8001-000000000003"

// Identity / naming. The brand tag is what the application uses to confirm it
// is talking to a genuine OctoPad (over serial or Bluetooth); the device name
// is user-configurable and stored in NVS, so a DIY build can be called anything.
#define BRAND_TAG "OCTOPAD"
#define FIRMWARE_VERSION "2.5"
#define DEFAULT_DEVICE_NAME "OctoPad"
#define MAX_DEVICE_NAME_LEN 24

NimBLEServer* pServer = nullptr;
NimBLECharacteristic* pTx = nullptr;

Preferences prefs;
String deviceName = DEFAULT_DEVICE_NAME;

constexpr uint8_t BUTTON_COUNT = 8;
constexpr uint8_t MODE_BUTTON = 0; // B1 = GPIO20
constexpr uint8_t LED_PIN = 4;

// Accurate Timing Parameters for Human Fingers
constexpr unsigned long DEBOUNCE_MS = 25;     // Mechanical switch bounce filter
constexpr unsigned long CLICK_TIMEOUT = 250;  // Maximum interval BETWEEN TAPS (after release)
constexpr unsigned long HOLD_TIMEOUT = 450;   // Press duration for the HOLD (Long Press) function

const uint8_t buttonPins[BUTTON_COUNT] = {20, 9, 2, 1, 21, 10, 3, 0};

// --- DESKTOP MODE LAYER ---
// (Arrays are kept for SET/SAVE compatibility from the application; HID is no
//  longer sent — all actions are executed by the application parsing event lines.)
uint8_t cfg_desktopSingle[BUTTON_COUNT - 1] = {KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8};
uint8_t cfg_desktopDouble[BUTTON_COUNT - 1] = {KEY_Q, KEY_W, KEY_E, KEY_I, KEY_T, KEY_Y, KEY_U};
uint8_t cfg_desktopHold[BUTTON_COUNT - 1]   = {KEY_A, KEY_S, KEY_D, KEY_F, KEY_G, KEY_H, KEY_J};

// --- HOME ASSISTANT MODE LAYER ---
uint8_t cfg_haSingle[BUTTON_COUNT - 1]  = {KEY_F2, KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8};
uint8_t cfg_haDouble[BUTTON_COUNT - 1]  = {KEY_Z,  KEY_X,  KEY_C,  KEY_V,  KEY_B,  KEY_N,  KEY_P};
uint8_t cfg_haHold[BUTTON_COUNT - 1]    = {KEY_9,  KEY_0,  KEY_I,  KEY_O,  KEY_P,  KEY_K,  KEY_L};

enum MacroMode { DESKTOP_MODE, HOME_ASSISTANT_MODE };
MacroMode currentMode = DESKTOP_MODE;

enum ButtonEvent { NONE, SINGLE_CLICK, DOUBLE_CLICK, HOLD };

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

String helloLine() {
  return String("HELLO:") + BRAND_TAG + ":" + deviceName + ":v" + FIRMWARE_VERSION;
}

void loadDeviceName() {
  prefs.begin("mp", true);
  String stored = prefs.getString("name", DEFAULT_DEVICE_NAME);
  prefs.end();
  stored.trim();
  if (stored.length() == 0 || stored.length() > MAX_DEVICE_NAME_LEN) {
    stored = DEFAULT_DEVICE_NAME;
  }
  deviceName = stored;
}

void saveDeviceName(const String& name) {
  prefs.begin("mp", false);
  prefs.putString("name", name);
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

bool loadConfig() {
  prefs.begin("mp", false);
  if (!prefs.isKey("v")) {
    prefs.end();
    return false;
  }
  prefs.getBytes("d1", cfg_desktopSingle, 7);
  prefs.getBytes("d2", cfg_desktopDouble, 7);
  prefs.getBytes("dh", cfg_desktopHold, 7);
  prefs.getBytes("h1", cfg_haSingle, 7);
  prefs.getBytes("h2", cfg_haDouble, 7);
  prefs.getBytes("hh", cfg_haHold, 7);
  prefs.end();
  return true;
}

void saveConfig() {
  prefs.begin("mp", false);
  prefs.putUChar("v", 1);
  prefs.putBytes("d1", cfg_desktopSingle, 7);
  prefs.putBytes("d2", cfg_desktopDouble, 7);
  prefs.putBytes("dh", cfg_desktopHold, 7);
  prefs.putBytes("h1", cfg_haSingle, 7);
  prefs.putBytes("h2", cfg_haDouble, 7);
  prefs.putBytes("hh", cfg_haHold, 7);
  prefs.end();
}

void toggleMode() {
  if (currentMode == DESKTOP_MODE) {
    currentMode = HOME_ASSISTANT_MODE;
    Serial.println("MODE: HOME ASSISTANT");
    bleSend("MODE: HOME ASSISTANT");
    digitalWrite(LED_PIN, LOW);
  } else {
    currentMode = DESKTOP_MODE;
    Serial.println("MODE: DESKTOP");
    bleSend("MODE: DESKTOP");
    digitalWrite(LED_PIN, HIGH);
  }
}

// Dynamic LED Blink Function (Optimized to 80ms to reduce lag)
void blinkLED(uint8_t count) {
  bool baseState = digitalRead(LED_PIN);
  for (uint8_t i = 0; i < count; i++) {
    digitalWrite(LED_PIN, !baseState);
    delay(80);
    digitalWrite(LED_PIN, baseState);
    delay(80);
  }
}

void handleButtonEvent(uint8_t buttonIndex, ButtonEvent event) {
  // 1. Mode Button (B1): a single click toggles the mode; HOLD is now managed by
  //    the application (via event lines) just like the other buttons.
  if (buttonIndex == MODE_BUTTON && event == SINGLE_CLICK) {
    toggleMode();
  }

  // 2. Print Debug Log to Serial + send to BLE (identical format for
  //    the application to parse over either the USB serial or Bluetooth path).
  String modeText = (currentMode == DESKTOP_MODE) ? "DESKTOP" : "HA";
  String eventText = (event == SINGLE_CLICK) ? "SINGLE" : (event == DOUBLE_CLICK) ? "DOUBLE" : "HOLD";
  String line = String("[") + modeText + "] Button " + String(buttonIndex + 1) +
                " -> " + eventText + " (LED Feedback Processed)";
  Serial.println(line);
  bleSend(line);

  // 3. Global LED Feedback Rule for ANY Button (Including Button 1)
  if (event == SINGLE_CLICK) {
    // "Do nothing" -> LED does not blink, keeping the main light state.
  }
  else if (event == DOUBLE_CLICK) {
    blinkLED(2); // Blink 2x for every Double Click
  }
  else if (event == HOLD) {
    blinkLED(3); // Blink 3x for every Long Press
  }
}

void setup() {
  Serial.begin(115200);
  delay(400);

  pinMode(LED_PIN, OUTPUT);
  digitalWrite(LED_PIN, HIGH);

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

  loadConfig();
  loadDeviceName();
  setupBLE();
  Serial.print("BLE ");
  Serial.print(deviceName);
  Serial.println(" Ready");
  Serial.println(helloLine());
}

void handleCmd() {
  if (!Serial.available()) return;
  String l = Serial.readStringUntil('\n');
  l.trim();

  if (l.startsWith("SET ")) {
    // SET <key> <c0,c1,c2,c3,c4,c5,c6>
    String rest = l.substring(4);
    int sp = rest.indexOf(' ');
    if (sp > 0) {
      String key = rest.substring(0, sp);
      String csv = rest.substring(sp + 1);
      uint8_t* arr = nullptr;
      if (key == "d1") arr = cfg_desktopSingle;
      else if (key == "d2") arr = cfg_desktopDouble;
      else if (key == "dh") arr = cfg_desktopHold;
      else if (key == "h1") arr = cfg_haSingle;
      else if (key == "h2") arr = cfg_haDouble;
      else if (key == "hh") arr = cfg_haHold;
      if (arr) {
        int i = 0, s = 0;
        while (i < 7 && s < (int)csv.length()) {
          int e = csv.indexOf(',', s);
          String tok = (e < 0) ? csv.substring(s) : csv.substring(s, e);
          arr[i] = (uint8_t)strtol(tok.c_str(), nullptr, 10);
          i++;
          if (e < 0) break;
          s = e + 1;
        }
        Serial.println(String("SET:") + key + ":OK");
        return;
      }
    }
    Serial.println("SET:ERR");
    return;
  }
  if (l.startsWith("SAVE")) {
    saveConfig();
    Serial.println("SAVED");
    return;
  }
  if (l.startsWith("NAME:")) {
    // NAME:<new-name> — set + persist the device name, then restart BLE so
    // the new name is advertised immediately (this drops any BLE link).
    String newName = l.substring(5);
    newName.trim();
    if (newName.length() > 0 && newName.length() <= MAX_DEVICE_NAME_LEN) {
      deviceName = newName;
      saveDeviceName(newName);
      Serial.println(String("NAME:OK:") + newName);
      applyDeviceName();
      Serial.println(helloLine());
    } else {
      Serial.println("NAME:ERR:length");
    }
    return;
  }
  if (l.startsWith("WHO")) {
    Serial.println(helloLine());
    return;
  }
  if (l.startsWith("RESET")) {
    // keep defaults; save
    saveConfig();
    Serial.println("RESET+SAVED");
    return;
  }
}

void loop() {
  handleCmd();
  unsigned long now = millis();

  for (uint8_t i = 0; i < BUTTON_COUNT; i++) {
    bool rawReading = digitalRead(buttons[i].pin);

    // Debounce Filter
    if (rawReading != buttons[i].lastRawState) {
      buttons[i].lastDebounceTime = now;
      buttons[i].lastRawState = rawReading;
    }

    if ((now - buttons[i].lastDebounceTime) >= DEBOUNCE_MS) {
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

    // HOLD Evaluation Logic
    if (buttons[i].isPressed && !buttons[i].holdReported) {
      if (now - buttons[i].lastPressTime >= HOLD_TIMEOUT) {
        buttons[i].holdReported = true;
        buttons[i].clickCount = 0;
        handleButtonEvent(i, HOLD);
      }
    }

    // Click Evaluation Logic (Single / Double)
    if (!buttons[i].isPressed) {
      if (buttons[i].clickCount == 1 && (now - buttons[i].lastReleaseTime >= CLICK_TIMEOUT)) {
        handleButtonEvent(i, SINGLE_CLICK);
        buttons[i].clickCount = 0;
      }
      else if (buttons[i].clickCount >= 2) {
        handleButtonEvent(i, DOUBLE_CLICK);
        buttons[i].clickCount = 0;
      }
    }
  }
  delay(1);
}