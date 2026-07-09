"""QML-based settings window (smooth, GPU, macOS/iOS-grade) for AppSwitcher."""
import os
import sys
from PySide6 import QtCore, QtGui, QtQml
import switcher as S

def _qml_dir():
    # frozen (PyInstaller) bundles data into _MEIPASS
    return getattr(sys, "_MEIPASS", os.path.dirname(os.path.abspath(__file__)))


def _three_finger_enabled():
    """True if Windows 3-finger swipes are still ON (user hasn't disabled)."""
    try:
        import winreg
        key = r"Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad"
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key) as k:
            v, _ = winreg.QueryValueEx(k, "ThreeFingerSlideEnabled")
            return v != 0
    except FileNotFoundError:
        return True       # key absent = default (enabled)
    except Exception:
        return True


# Qt.Key -> Windows vkCode for keys whose codes differ. Letters (Qt.Key_A..Z =
# 0x41..0x5A) and digits (0x30..0x39) already equal their vkCodes, so only the
# odd ones (Tab, arrows, punctuation, F-keys…) need an entry.
_QT_TO_VK = {
    0x01000001: 0x09,  # Tab
    0x01000000: 0x1B,  # Escape
    0x01000004: 0x0D,  # Return
    0x01000005: 0x0D,  # Enter (keypad)
    0x01000003: 0x08,  # Backspace
    0x01000007: 0x2E,  # Delete
    0x01000006: 0x2D,  # Insert
    0x01000010: 0x24,  # Home
    0x01000011: 0x23,  # End
    0x01000016: 0x21,  # PageUp
    0x01000017: 0x22,  # PageDown
    0x01000012: 0x25,  # Left
    0x01000013: 0x26,  # Up
    0x01000014: 0x27,  # Right
    0x01000015: 0x28,  # Down
    0x20:       0x20,  # Space
    0x2D: 0xBD, 0x3D: 0xBB,               # - =
    0x5B: 0xDB, 0x5D: 0xDD, 0x5C: 0xDC,   # [ ] \
    0x3B: 0xBA, 0x27: 0xDE,               # ; '
    0x2C: 0xBC, 0x2E: 0xBE, 0x2F: 0xBF,   # , . /
    0x60: 0xC0,                           # ` (backtick)
}
for _i in range(12):                       # F1..F12
    _QT_TO_VK[0x01000030 + _i] = 0x70 + _i

def _qt_key_to_vk(qt_key):
    if qt_key in _QT_TO_VK:
        return _QT_TO_VK[qt_key]
    if 0x30 <= qt_key <= 0x39 or 0x41 <= qt_key <= 0x5A:   # 0-9, A-Z
        return qt_key
    return 0


class Backend(QtCore.QObject):
    changed = QtCore.Signal()
    warnChanged = QtCore.Signal()
    blocklistChanged = QtCore.Signal()

    def __init__(self):
        super().__init__()
        self._warn = _three_finger_enabled()
        self._last_sig = None
        self._poll = QtCore.QTimer(self)
        self._poll.setInterval(1000)            # live-refresh the window list
        self._poll.timeout.connect(self._refresh_if_changed)

    def _refresh_if_changed(self):
        sig = tuple((w["title"], w["blocked"]) for w in self.openWindows)
        if sig != self._last_sig:
            self._last_sig = sig
            self.blocklistChanged.emit()

    @QtCore.Slot()
    def startWatch(self):
        self._last_sig = None
        self._refresh_if_changed()
        self._poll.start()

    @QtCore.Slot()
    def stopWatch(self):
        self._poll.stop()

    @QtCore.Slot()
    def refresh(self):
        self._last_sig = None
        self.blocklistChanged.emit()

    # blocklist -------------------------------------------------------------
    @QtCore.Property('QVariantList', notify=blocklistChanged)
    def openWindows(self):
        """[{title, blocked}] — every open window + any already-blocked title."""
        titles = S.list_open_titles()
        blocked = list(S.SETTINGS.get("blocklist", []))
        seen = set(titles)
        for t in blocked:                               # keep blocked-but-closed
            if t not in seen:
                titles.append(t); seen.add(t)
        return [{"title": t, "blocked": t in blocked} for t in titles]

    @QtCore.Slot(str, bool)
    def setBlocked(self, title, blocked):
        bl = list(S.SETTINGS.get("blocklist", []))
        if blocked and title not in bl:
            bl.append(title)
        elif not blocked:
            bl = [t for t in bl if t != title]
        S.SETTINGS["blocklist"] = bl
        S.save_settings()
        self.blocklistChanged.emit()

    # static lists ----------------------------------------------------------
    @QtCore.Property('QStringList', constant=True)
    def animations(self):
        return list(S.ANIMATIONS.keys())

    @QtCore.Property('QStringList', constant=True)
    def layoutsList(self):
        import layouts
        return list(layouts.LAYOUTS.keys())

    # settings properties ---------------------------------------------------
    @QtCore.Property(str, notify=changed)
    def animation(self):
        return S.SETTINGS.get("animation", "slide")
    @animation.setter
    def animation(self, v):
        S.SETTINGS["animation"] = v; self.changed.emit()

    @QtCore.Property(str, notify=changed)
    def layout(self):
        return S.SETTINGS.get("layout", "dock")
    @layout.setter
    def layout(self, v):
        S.SETTINGS["layout"] = v; self.changed.emit()

    @QtCore.Property(int, notify=changed)
    def duration(self):
        return int(S.SETTINGS.get("duration", 0.22) * 1000)
    @duration.setter
    def duration(self, v):
        S.SETTINGS["duration"] = v / 1000.0; self.changed.emit()

    @QtCore.Property(float, notify=changed)
    def dockMag(self):
        return float(S.SETTINGS.get("dock_mag", 1.95))
    @dockMag.setter
    def dockMag(self, v):
        S.SETTINGS["dock_mag"] = float(v); self.changed.emit()

    @QtCore.Property(int, notify=changed)
    def sensitivity(self):
        return int(S.SETTINGS.get("sensitivity", 210))
    @sensitivity.setter
    def sensitivity(self, v):
        S.SETTINGS["sensitivity"] = int(v); self.changed.emit()

    @QtCore.Property(str, notify=changed)
    def accent(self):
        r, g, b = S.SETTINGS.get("accent", [150, 205, 255])
        return f"#{r:02x}{g:02x}{b:02x}"
    @accent.setter
    def accent(self, v):
        c = QtGui.QColor(v)
        S.SETTINGS["accent"] = [c.red(), c.green(), c.blue()]; self.changed.emit()

    @QtCore.Property(bool, notify=changed)
    def autostart(self):
        import gui
        return gui._is_autostart()
    @autostart.setter
    def autostart(self, v):
        import main
        (main.install_startup if v else main.uninstall_startup)()
        self.changed.emit()

    @QtCore.Property(bool, notify=changed)
    def altTab(self):
        return bool(S.SETTINGS.get("alt_tab", True))
    @altTab.setter
    def altTab(self, v):
        S.SETTINGS["alt_tab"] = bool(v)
        try:
            import kbd_hook
            kbd_hook.enabled = bool(v)
        except Exception:
            pass
        self.changed.emit()

    # hotkey ----------------------------------------------------------------
    @QtCore.Property('QStringList', constant=True)
    def hotkeyMods(self):
        return ["alt", "ctrl"]

    @QtCore.Property('QStringList', constant=True)
    def hotkeyKeys(self):
        return list(S.HOTKEY_KEYS.keys())

    @QtCore.Property(str, notify=changed)
    def hotkeyMod(self):
        return str(S.SETTINGS.get("hotkey_mod", "alt"))
    @hotkeyMod.setter
    def hotkeyMod(self, v):
        S.SETTINGS["hotkey_mod"] = str(v)
        S.apply_hotkey(); self.changed.emit()

    @QtCore.Property(str, notify=changed)
    def hotkeyKey(self):
        return S._VK_TO_KEY.get(int(S.SETTINGS.get("hotkey_key", 0x09)), "Tab")
    @hotkeyKey.setter
    def hotkeyKey(self, v):
        S.SETTINGS["hotkey_key"] = int(S.HOTKEY_KEYS.get(str(v), 0x09))
        S.apply_hotkey(); self.changed.emit()

    # pinch shortcut -------------------------------------------------------
    @QtCore.Property(bool, notify=changed)
    def pinchEnabled(self):
        return bool(S.SETTINGS.get("pinch_enabled", True))
    @pinchEnabled.setter
    def pinchEnabled(self, v):
        S.SETTINGS["pinch_enabled"] = bool(v); S.save_settings(); self.changed.emit()

    @QtCore.Property(str, notify=changed)
    def pinchShortcut(self):
        return S.pinch_label()

    @QtCore.Slot(int, int)
    def captureChord(self, qt_key, qt_mods):
        """Record a key chord captured in the QML field. qt_key is a Qt.Key,
        qt_mods a Qt.KeyboardModifiers bitmask — mapped to Windows vkCodes."""
        vk = _qt_key_to_vk(qt_key)
        if not vk:
            return
        mods = []
        if qt_mods & 0x04000000: mods.append(0x11)   # Ctrl
        if qt_mods & 0x08000000: mods.append(0x12)   # Alt
        if qt_mods & 0x02000000: mods.append(0x10)   # Shift
        if qt_mods & 0x10000000: mods.append(0x5B)   # Win / Meta
        S.SETTINGS["pinch_mods"] = mods
        S.SETTINGS["pinch_vk"]   = vk
        S.save_settings(); self.changed.emit()

    @QtCore.Slot()
    def clearChord(self):
        S.SETTINGS["pinch_mods"] = []
        S.SETTINGS["pinch_vk"]   = 0
        S.save_settings(); self.changed.emit()

    @QtCore.Property(bool, notify=warnChanged)
    def threeFingerWarning(self):
        return self._warn and not S.SETTINGS.get("warn_dismissed", False)

    @QtCore.Slot()
    def dismissWarning(self):
        S.SETTINGS["warn_dismissed"] = True
        S.save_settings()
        self.warnChanged.emit()

    # actions ---------------------------------------------------------------
    @QtCore.Slot()
    def save(self):
        S.save_settings()

    @QtCore.Slot()
    def previewLayout(self):
        S.post(lambda: S._switcher.show_picker())

    @QtCore.Slot()
    def previewAnim(self):
        S.post(lambda: S._switcher.preview_anim())

    @QtCore.Slot()
    def recheckWarning(self):
        self._warn = _three_finger_enabled(); self.warnChanged.emit()

    @QtCore.Slot()
    def openTouchpadSettings(self):
        os.startfile("ms-settings:devices-touchpad")


_engine = None
_backend = None


def open_settings():
    global _engine, _backend
    if _engine is None:
        _backend = Backend()
        _engine = QtQml.QQmlApplicationEngine()
        _engine.rootContext().setContextProperty("backend", _backend)
        qml = os.path.join(_qml_dir(), "Settings.qml")
        _engine.load(QtCore.QUrl.fromLocalFile(qml))
    if _engine.rootObjects():
        _backend.recheckWarning()
        _backend.blocklistChanged.emit()      # refresh open-apps list each show
        win = _engine.rootObjects()[0]
        win.setProperty("visible", True)
        try:
            win.raise_(); win.requestActivate()
        except Exception:
            pass
    else:
        print("[qml] failed to load Settings.qml")
