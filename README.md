# AppSwitcher

Mac-style 3-finger gesture app switching for Windows 11. GPU-composited overlay
(PySide6/Qt). Configurable animations + picker layouts, settings GUI in the tray.

## What it does

| Gesture | Action |
|---|---|
| 3-finger swipe (keep moving) | Switch apps with a transition |
| 3-finger hold still ~0.2s | Open the app picker |
| Picker: move fingers | Slide the highlight |
| Picker: lift fingers | Switch to the highlighted app |
| Picker: ← → / Enter / Esc / click | Navigate / select / cancel |

**7 switch animations:** Slide, Fade, Push, Zoom, Cube, Flip, Stack.
**7 picker layouts:** Dock, Grid, Coverflow, Fan, Hero+Filmstrip, Wallet, Row.

Pick them in the **tray icon → Settings** (also tune speed, dock magnify,
sensitivity, accent color; live Preview). Saved to
`%APPDATA%\AppSwitcher\settings.json`.

> Works on **Windows precision touchpads** only (raw HID parsing). No admin needed.

---

## Setup

### 1. Install Python 3.10+
https://python.org/downloads — check "Add to PATH"

### 2. Install dependencies
```
pip install pywin32 Pillow PySide6
```
(Renderer is PySide6/Qt — GPU-composited overlay. PIL is used for thumbnail
capture/processing, pywin32 for window control + touchpad raw input.)

### 3. One-time: disable Windows 3-finger gestures
Settings → Bluetooth & devices → Touchpad → Three-finger gestures
→ Set "Swipes" to **Nothing**

This frees up 3-finger swipe for AppSwitcher.

### 4. Run
```
python main.py
```

### 5. (Optional) Start with Windows
Toggle "Start with Windows" in the tray Settings, or:
```
python main.py --install     # add to startup
python main.py --uninstall   # remove
```

---

## Build a shareable installer

Bundles Python + all deps into one `.exe` + an installer (Start Menu shortcut,
autostart toggle, uninstaller). Needs:
- `pip install pyinstaller`
- [Inno Setup 6](https://jrsoftware.org/isdl.php) (free)

Then:
```
build.bat
```
Output: `Output\AppSwitcher-Setup.exe` (share this). Without Inno Setup you still
get the portable app in `dist\AppSwitcher\` — zip and share that.

---

## Files

```
appswitcher/
├── main.py          ← entry point, run this
├── gesture_hook.py  ← touchpad Raw Input (WM_INPUT) listener + HID parse
└── switcher.py      ← animation engine + window switching
```

## Verify gestures (if nothing happens)

The listener reads raw touchpad HID reports. To confirm your touchpad is
parsed correctly:
```
python gesture_hook.py --debug
```
Put 3 fingers on the touchpad and swipe. You should see `[frame] fingers=3 ...`
lines and a `SWIPE LEFT/RIGHT`. If `fingers` never reaches 3, Windows is still
eating the gesture — finish step 3 above. If fingers show but no SWIPE fires,
the swipe distance threshold needs tuning (`SWIPE_THRESHOLD` in gesture_hook.py,
in touchpad logical units).

---

## Tuning

Gesture constants live in `gesture_hook.py` (class `GestureTracker`):

| Constant | Default | What it does |
|---|---|---|
| `HOLD_THRESHOLD` | 0.25s | How long to hold before picker shows |
| `SWIPE_THRESHOLD` | 600 | Min swipe distance, in touchpad logical units |
| `MIN_FINGERS` | 3 | Fingers required to trigger |

Animation constants live in `switcher.py`:

| Constant | Default | What it does |
|---|---|---|
| `ANIM_DURATION` | 0.22s | Slide animation length |
| `FPS` | 60 | Animation framerate |

---

## Troubleshooting

**Gestures not detected**
- Make sure you disabled Win11 3-finger swipe in Settings
- Run as Administrator if needed

**Window switch works but no animation**
- `pip install pygame` and `pip install Pillow`

**Picker is laggy**
- Normal on first open (capturing thumbnails). Subsequent opens faster.
