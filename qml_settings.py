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


class Backend(QtCore.QObject):
    changed = QtCore.Signal()
    warnChanged = QtCore.Signal()
    blocklistChanged = QtCore.Signal()

    def __init__(self):
        super().__init__()
        self._warn = _three_finger_enabled()

    # blocklist -------------------------------------------------------------
    @QtCore.Property('QVariantList', notify=blocklistChanged)
    def openApps(self):
        """[{exe, name, blocked}] — currently-open apps + already-blocked ones."""
        exes = dict(S.list_open_exes())                 # exe -> sample title
        blocked = [e.lower() for e in S.SETTINGS.get("blocklist", [])]
        for e in blocked:
            exes.setdefault(e, e)
        out = []
        for exe in sorted(exes):
            base = exe[:-4] if exe.endswith(".exe") else exe
            name = (base[:1].upper() + base[1:]) if base else exe
            out.append({"exe": exe, "name": name, "blocked": exe in blocked})
        return out

    @QtCore.Slot(str, bool)
    def setBlocked(self, exe, blocked):
        exe = (exe or "").lower()
        bl = [e.lower() for e in S.SETTINGS.get("blocklist", [])]
        if blocked and exe not in bl:
            bl.append(exe)
        elif not blocked:
            bl = [e for e in bl if e != exe]
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
