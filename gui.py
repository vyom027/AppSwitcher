"""Settings window + system tray for AppSwitcher (PySide6)."""
from PySide6 import QtCore, QtGui, QtWidgets
import switcher as S

_tray = None
_win = None


def _layout_names():
    import layouts
    return list(layouts.LAYOUTS.keys())


def _make_icon():
    """Generate a simple app-switcher tray icon (no asset file needed)."""
    pix = QtGui.QPixmap(64, 64); pix.fill(QtCore.Qt.transparent)
    p = QtGui.QPainter(pix); p.setRenderHint(QtGui.QPainter.Antialiasing)
    p.setBrush(QtGui.QColor(30, 34, 44)); p.setPen(QtCore.Qt.NoPen)
    p.drawRoundedRect(4, 4, 56, 56, 14, 14)
    p.setBrush(S.accent_qcolor())
    p.drawRoundedRect(14, 18, 24, 16, 4, 4)
    p.setBrush(QtGui.QColor(150, 160, 180))
    p.drawRoundedRect(28, 32, 24, 16, 4, 4)
    p.end()
    return QtGui.QIcon(pix)


class SettingsWindow(QtWidgets.QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("AppSwitcher — Settings")
        self.setWindowIcon(_make_icon())
        self.setMinimumWidth(380)
        form = QtWidgets.QFormLayout(self)
        form.setVerticalSpacing(12)

        self.anim = QtWidgets.QComboBox()
        self.anim.addItems(list(S.ANIMATIONS.keys()))
        self.anim.setCurrentText(S.SETTINGS.get("animation", "slide"))

        self.layout_cb = QtWidgets.QComboBox()
        self.layout_cb.addItems(_layout_names())
        self.layout_cb.setCurrentText(S.SETTINGS.get("layout", "dock"))

        self.dur = QtWidgets.QSlider(QtCore.Qt.Horizontal)
        self.dur.setRange(80, 600); self.dur.setValue(int(S.SETTINGS.get("duration", 0.22) * 1000))
        self.dur_l = QtWidgets.QLabel()

        self.mag = QtWidgets.QSlider(QtCore.Qt.Horizontal)
        self.mag.setRange(120, 260); self.mag.setValue(int(S.SETTINGS.get("dock_mag", 1.95) * 100))
        self.mag_l = QtWidgets.QLabel()

        self.sens = QtWidgets.QSlider(QtCore.Qt.Horizontal)
        self.sens.setRange(120, 400); self.sens.setValue(int(S.SETTINGS.get("sensitivity", 210)))
        self.sens_l = QtWidgets.QLabel()

        self.accent_btn = QtWidgets.QPushButton("Pick accent color")
        self.accent_btn.clicked.connect(self._pick_accent)

        self.autostart = QtWidgets.QCheckBox("Start with Windows")
        self.autostart.setChecked(_is_autostart())
        self.autostart.toggled.connect(self._toggle_autostart)

        prev_l = QtWidgets.QPushButton("Preview layout"); prev_l.clicked.connect(self._prev_layout)
        prev_a = QtWidgets.QPushButton("Preview animation"); prev_a.clicked.connect(self._prev_anim)
        save = QtWidgets.QPushButton("Save"); save.clicked.connect(self._save)

        form.addRow("Switch animation", self.anim)
        form.addRow("Picker layout", self.layout_cb)
        form.addRow("Animation speed", self._with_label(self.dur, self.dur_l))
        form.addRow("Dock magnify", self._with_label(self.mag, self.mag_l))
        form.addRow("Sensitivity", self._with_label(self.sens, self.sens_l))
        form.addRow("Accent", self.accent_btn)
        form.addRow("", self.autostart)
        row = QtWidgets.QHBoxLayout(); row.addWidget(prev_l); row.addWidget(prev_a)
        form.addRow(row)
        form.addRow(save)

        for w in (self.anim, self.layout_cb):
            w.currentTextChanged.connect(self._apply)
        for s in (self.dur, self.mag, self.sens):
            s.valueChanged.connect(self._apply)
        self._apply()

    def _with_label(self, slider, label):
        w = QtWidgets.QWidget(); h = QtWidgets.QHBoxLayout(w); h.setContentsMargins(0, 0, 0, 0)
        h.addWidget(slider); label.setMinimumWidth(48); h.addWidget(label)
        return w

    def _apply(self, *_):
        S.SETTINGS["animation"]   = self.anim.currentText()
        S.SETTINGS["layout"]      = self.layout_cb.currentText()
        S.SETTINGS["duration"]    = self.dur.value() / 1000
        S.SETTINGS["dock_mag"]    = self.mag.value() / 100
        S.SETTINGS["sensitivity"] = self.sens.value()
        self.dur_l.setText(f"{self.dur.value()} ms")
        self.mag_l.setText(f"{self.mag.value()/100:.2f}x")
        self.sens_l.setText(str(self.sens.value()))

    def _save(self):
        self._apply(); S.save_settings(); self.hide()

    def _pick_accent(self):
        c = QtWidgets.QColorDialog.getColor(S.accent_qcolor(), self, "Accent color")
        if c.isValid():
            S.SETTINGS["accent"] = [c.red(), c.green(), c.blue()]

    def _prev_layout(self):
        self._apply(); S.post(lambda: S._switcher.show_picker())

    def _prev_anim(self):
        self._apply(); S.post(lambda: S._switcher.preview_anim())

    def _toggle_autostart(self, on):
        try:
            import main
            (main.install_startup if on else main.uninstall_startup)()
        except Exception as e:
            print("[autostart]", e)


def _is_autostart():
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                            r"Software\Microsoft\Windows\CurrentVersion\Run") as k:
            winreg.QueryValueEx(k, "AppSwitcher")
        return True
    except Exception:
        return False


def show_settings():
    try:
        import qml_settings
        qml_settings.open_settings()
    except Exception as e:
        print("[qml settings] failed, using fallback:", e)
        global _win
        if _win is None:
            _win = SettingsWindow()
        _win.show(); _win.raise_(); _win.activateWindow()


def build_tray(app):
    global _tray
    _tray = QtWidgets.QSystemTrayIcon(_make_icon())
    menu = QtWidgets.QMenu()
    menu.addAction("Settings…").triggered.connect(show_settings)
    menu.addSeparator()
    menu.addAction("Quit").triggered.connect(app.quit)
    _tray.setContextMenu(menu)
    _tray.setToolTip("AppSwitcher — 3-finger app switching")
    _tray.activated.connect(
        lambda r: show_settings() if r == QtWidgets.QSystemTrayIcon.Trigger else None)
    _tray.show()
    return _tray
