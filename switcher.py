"""
AppSwitcher - Mac-style gesture app switching for Windows 11
Renderer: PySide6 (Qt) QGraphicsView — GPU-composited overlay, smooth 60fps.
The gesture layer (gesture_hook.py) and all Win32/PIL helpers are unchanged.
"""

import os
import sys
import time
import math
import threading
import ctypes
import ctypes.wintypes

import win32gui
import win32ui
import win32con
import win32process
from PIL import Image, ImageGrab, ImageDraw, ImageFont, ImageFilter, ImageChops

os.environ.setdefault("QT_ENABLE_HIGHDPI_SCALING", "0")
os.environ.setdefault("QT_SCALE_FACTOR", "1")
from PySide6 import QtCore, QtGui, QtWidgets

# ─── Constants ───────────────────────────────────────────────────────────────
DEBUG           = False
FPS             = 60
FRAME_MS        = int(1000 / FPS)
ANIM_DURATION   = 0.22
PICKER_STEP     = 210   # touchpad logical units of finger travel per app step

# ─── Settings (persisted to %APPDATA%\AppSwitcher\settings.json) ─────────────
import json
_CFG_DIR  = os.path.join(os.environ.get("APPDATA", os.path.expanduser("~")), "AppSwitcher")
_CFG_PATH = os.path.join(_CFG_DIR, "settings.json")
SETTINGS  = {
    "animation":  "slide",    # see ANIMATIONS registry
    "layout":     "dock",     # see layouts.LAYOUTS registry
    "duration":   0.22,       # switch animation seconds
    "dock_mag":   1.95,       # dock magnification
    "sensitivity": 210,       # PICKER_STEP (lower = less finger travel per app)
    "accent":     [150, 205, 255],
    "warn_dismissed": False,  # user dismissed the 3-finger warning
}

def load_settings():
    try:
        with open(_CFG_PATH, "r", encoding="utf-8") as f:
            SETTINGS.update(json.load(f))
    except Exception:
        pass

def save_settings():
    try:
        os.makedirs(_CFG_DIR, exist_ok=True)
        with open(_CFG_PATH, "w", encoding="utf-8") as f:
            json.dump(SETTINGS, f, indent=2)
    except Exception as e:
        print("[settings] save failed:", e)

def accent_qcolor():
    from PySide6 import QtGui as _qg
    r, g, b = SETTINGS.get("accent", [150, 205, 255])
    return _qg.QColor(int(r), int(g), int(b))

# ─── ctypes signatures (avoid 64-bit handle truncation) ──────────────────────
_u32 = ctypes.windll.user32
_u32.GetForegroundWindow.restype = ctypes.c_void_p
_u32.SetForegroundWindow.argtypes = [ctypes.c_void_p]
_u32.SwitchToThisWindow.argtypes = [ctypes.c_void_p, ctypes.c_bool]
_u32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
_u32.GetWindowThreadProcessId.restype = ctypes.c_ulong
_u32.AttachThreadInput.argtypes = [ctypes.c_ulong, ctypes.c_ulong, ctypes.c_bool]
_u32.GetAsyncKeyState.restype = ctypes.c_short
_u32.GetAsyncKeyState.argtypes = [ctypes.c_int]
_u32.GetCursorPos.argtypes = [ctypes.POINTER(ctypes.wintypes.POINT)]

# ─── Win32 Helpers ───────────────────────────────────────────────────────────

_GA_ROOTOWNER  = 3
_DWMWA_CLOAKED = 14
_u32_w = ctypes.windll.user32
_u32_w.GetLastActivePopup.restype  = ctypes.c_void_p
_u32_w.GetLastActivePopup.argtypes = [ctypes.c_void_p]

def _is_cloaked(hwnd):
    val = ctypes.c_int(0)
    try:
        ctypes.windll.dwmapi.DwmGetWindowAttribute(
            ctypes.c_void_p(hwnd), _DWMWA_CLOAKED,
            ctypes.byref(val), ctypes.sizeof(val))
    except Exception:
        return False
    return val.value != 0

def _is_alt_tab_window(hwnd):
    """True only for the windows Alt-Tab / the taskbar show: visible, titled,
    top-level, not a tool window, not a cloaked/ghost UWP shell."""
    if not win32gui.IsWindowVisible(hwnd):
        return False
    title = win32gui.GetWindowText(hwnd)
    if not title or title in ("Program Manager", "Windows Input Experience"):
        return False
    ex = win32gui.GetWindowLong(hwnd, win32con.GWL_EXSTYLE)
    if ex & win32con.WS_EX_TOOLWINDOW:
        return False
    root = win32gui.GetAncestor(hwnd, _GA_ROOTOWNER)
    if _u32_w.GetLastActivePopup(root) != hwnd:
        return False
    if _is_cloaked(hwnd):
        return False
    return True

def get_open_windows():
    windows = []
    def cb(hwnd, _):
        if _is_alt_tab_window(hwnd):
            windows.append((hwnd, win32gui.GetWindowText(hwnd)))
        return True
    win32gui.EnumWindows(cb, None)
    return windows

class _MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", ctypes.c_long), ("dy", ctypes.c_long),
                ("mouseData", ctypes.c_ulong), ("dwFlags", ctypes.c_ulong),
                ("time", ctypes.c_ulong), ("dwExtraInfo", ctypes.c_void_p)]

class _INPUT(ctypes.Structure):
    class _U(ctypes.Union):
        _fields_ = [("mi", _MOUSEINPUT)]
    _anonymous_ = ("u",)
    _fields_ = [("type", ctypes.c_ulong), ("u", _U)]

_u32.SendInput.argtypes = [ctypes.c_uint, ctypes.POINTER(_INPUT), ctypes.c_int]

def nudge_cursor():
    """Clear the 'working in background' cursor Windows shows after a background
    process calls SetForegroundWindow. It only clears on REAL input, so we
    inject a net-zero mouse move via SendInput (SetCursorPos doesn't count)."""
    try:
        MOVE = 0x0001   # MOUSEEVENTF_MOVE (relative)
        arr = (_INPUT * 2)()
        for k, dx in ((0, 1), (1, -1)):
            arr[k].type = 0          # INPUT_MOUSE
            arr[k].mi = _MOUSEINPUT(dx, 0, 0, MOVE, 0, None)
        _u32.SendInput(2, arr, ctypes.sizeof(_INPUT))
    except Exception:
        pass

def bring_to_front(hwnd):
    """Force a window to the foreground despite Windows' foreground lock.

    Uses AttachThreadInput + SetForegroundWindow (does NOT show the "working in
    background" wait cursor). SwitchToThisWindow is only a fallback because it
    simulates Alt-Tab and triggers that spinning cursor. Never raises.
    """
    user32 = ctypes.windll.user32
    try:
        if win32gui.IsIconic(hwnd):
            win32gui.ShowWindow(hwnd, win32con.SW_RESTORE)
    except Exception:
        pass

    fg = user32.GetForegroundWindow()
    cur_thread = ctypes.windll.kernel32.GetCurrentThreadId()
    fg_thread = user32.GetWindowThreadProcessId(fg, None)
    attached = False
    try:
        if fg_thread and fg_thread != cur_thread:
            attached = bool(user32.AttachThreadInput(fg_thread, cur_thread, True))
        win32gui.BringWindowToTop(hwnd)
        win32gui.SetForegroundWindow(hwnd)
    except Exception:
        pass
    finally:
        if attached:
            try:
                user32.AttachThreadInput(fg_thread, cur_thread, False)
            except Exception:
                pass

    # Fallback only if the foreground didn't actually change (avoids the
    # Alt-Tab spinner in the common case).
    try:
        if user32.GetForegroundWindow() != hwnd:
            user32.SwitchToThisWindow(hwnd, True)
    except Exception:
        pass

def capture_window_image(hwnd):
    """Capture a window's pixels via PrintWindow — works even when occluded."""
    hdc = mfc = save = bmp = None
    try:
        l, t, r, b = win32gui.GetWindowRect(hwnd)
        w, h = r - l, b - t
        if w <= 0 or h <= 0:
            return None
        hdc  = win32gui.GetWindowDC(hwnd)
        mfc  = win32ui.CreateDCFromHandle(hdc)
        save = mfc.CreateCompatibleDC()
        bmp  = win32ui.CreateBitmap()
        bmp.CreateCompatibleBitmap(mfc, w, h)
        save.SelectObject(bmp)
        ctypes.windll.user32.PrintWindow(hwnd, save.GetSafeHdc(), 2)
        info = bmp.GetInfo()
        bits = bmp.GetBitmapBits(True)
        return Image.frombuffer("RGB", (info["bmWidth"], info["bmHeight"]),
                                bits, "raw", "BGRX", 0, 1)
    except Exception:
        return None
    finally:
        try:
            if bmp: win32gui.DeleteObject(bmp.GetHandle())
        except Exception: pass
        try:
            if save: save.DeleteDC()
        except Exception: pass
        try:
            if mfc: mfc.DeleteDC()
        except Exception: pass
        try:
            if hdc: win32gui.ReleaseDC(hwnd, hdc)
        except Exception: pass

def screen_size():
    user32 = ctypes.windll.user32
    user32.SetProcessDPIAware()
    return user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)

def work_area():
    """Desktop area excluding any reserved taskbar/appbar (e.g. YASB)."""
    try:
        rect = ctypes.wintypes.RECT()
        if ctypes.windll.user32.SystemParametersInfoW(0x0030, 0,
                                                      ctypes.byref(rect), 0):
            w, h = rect.right - rect.left, rect.bottom - rect.top
            if w > 0 and h > 0:
                return rect.left, rect.top, w, h
    except Exception:
        pass
    sw, sh = screen_size()
    return 0, 0, sw, sh

# ─── PIL helpers (frosted backdrop, rounded cards, reflection) ───────────────

def _rounded_rgba(img, radius):
    img = img.convert("RGBA")
    mask = Image.new("L", img.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, img.size[0] - 1, img.size[1] - 1], radius=radius, fill=255)
    img.putalpha(mask)
    return img

def _frosted_backdrop(sw, sh):
    """Desktop, blurred (on a 1/4-scale copy) + dimmed — the frosted glass."""
    try:
        small = ImageGrab.grab(all_screens=False).resize((sw // 4, sh // 4))
        small = small.filter(ImageFilter.GaussianBlur(6))
        bg = small.resize((sw, sh))
    except Exception:
        bg = Image.new("RGB", (sw, sh), (20, 22, 28))
    return Image.blend(bg, Image.new("RGB", (sw, sh), (12, 14, 20)), 0.45)

_bg_cache = {"img": None, "ts": 0.0}

def prewarm_backdrop():
    sw, sh = screen_size()
    _bg_cache["img"] = _frosted_backdrop(sw, sh)
    _bg_cache["ts"]  = time.time()

def get_backdrop(sw, sh):
    img = _bg_cache["img"]
    if img is not None and img.size == (sw, sh) and time.time() - _bg_cache["ts"] < 1.5:
        return img
    prewarm_backdrop()
    return _bg_cache["img"]

def _reflection(card, frac=0.5, top_alpha=120):
    rh = max(1, int(card.height * frac))
    refl = card.transpose(Image.FLIP_TOP_BOTTOM).crop((0, 0, card.width, rh))
    grad = Image.new("L", (1, rh))
    for y in range(rh):
        grad.putpixel((0, y), int(top_alpha * (1 - y / rh)))
    grad = grad.resize((card.width, rh))
    refl.putalpha(ImageChops.multiply(refl.getchannel("A"), grad))
    return refl


# ─── Qt plumbing ─────────────────────────────────────────────────────────────

_app    = None
_bridge = None
_overlays = set()        # keep overlays alive while shown

def _pil2pix(im):
    """PIL Image -> QPixmap."""
    im = im.convert("RGBA")
    data = im.tobytes("raw", "RGBA")
    qim = QtGui.QImage(data, im.width, im.height, QtGui.QImage.Format.Format_RGBA8888)
    return QtGui.QPixmap.fromImage(qim.copy())   # copy: own the buffer

class _Bridge(QtCore.QObject):
    """Marshal callables from the gesture thread onto the Qt UI thread."""
    do = QtCore.Signal(object)
    def __init__(self):
        super().__init__()
        self.do.connect(self._run, QtCore.Qt.QueuedConnection)
    @QtCore.Slot(object)
    def _run(self, fn):
        try:
            fn()
        except Exception as e:
            print("[ui] task error:", e)

def post(fn):
    """Queue fn to run on the UI thread (thread-safe)."""
    _bridge.do.emit(fn)

def run_ui(open_settings=False):
    """Create the QApplication and run its event loop on the main thread."""
    global _app, _bridge
    _app = QtWidgets.QApplication.instance() or QtWidgets.QApplication(sys.argv)
    _app.setQuitOnLastWindowClosed(False)   # tray app: closing settings != quit
    _bridge = _Bridge()
    load_settings()
    try:
        import gui
        gui.build_tray(_app)
        if open_settings:                    # manual launch -> show settings
            QtCore.QTimer.singleShot(250, gui.show_settings)
    except Exception as e:
        print("[tray] failed:", e)
    _app.exec()


# ─── Overlay (frameless translucent topmost QGraphicsView) ───────────────────

class QtOverlay(QtWidgets.QGraphicsView):
    def __init__(self, sw, sh, clickable=False, x=0, y=0, activate=False):
        super().__init__()
        self.sw, self.sh = sw, sh
        self._closed = False
        self.scene = QtWidgets.QGraphicsScene(0, 0, sw, sh, self)
        self.setScene(self.scene)
        self.setWindowFlags(QtCore.Qt.FramelessWindowHint |
                            QtCore.Qt.WindowStaysOnTopHint |
                            QtCore.Qt.Tool)
        self.setAttribute(QtCore.Qt.WA_TranslucentBackground, True)
        self._activate = activate
        if not activate:
            self.setAttribute(QtCore.Qt.WA_ShowWithoutActivating, True)
        self.setHorizontalScrollBarPolicy(QtCore.Qt.ScrollBarAlwaysOff)
        self.setVerticalScrollBarPolicy(QtCore.Qt.ScrollBarAlwaysOff)
        self.setFrameShape(QtWidgets.QFrame.NoFrame)
        self.setStyleSheet("background: transparent; border: 0px;")
        self.setRenderHints(QtGui.QPainter.SmoothPixmapTransform |
                            QtGui.QPainter.Antialiasing)
        self.setGeometry(x, y, sw, sh)
        self.setSceneRect(0, 0, sw, sh)
        self._clickthrough = not clickable
        self.setWindowOpacity(0.0)

    def reveal(self, opacity=1.0):
        self.show()
        self.raise_()
        try:
            hwnd = int(self.winId())
            ex = win32gui.GetWindowLong(hwnd, win32con.GWL_EXSTYLE)
            if not self._activate:
                ex |= win32con.WS_EX_NOACTIVATE
            if self._clickthrough:
                ex |= win32con.WS_EX_TRANSPARENT
            win32gui.SetWindowLong(hwnd, win32con.GWL_EXSTYLE, ex)
            if self._activate:
                # become foreground so a later SetForegroundWindow(target) is a
                # clean foreground->foreground handoff (no "background" spinner).
                self.activateWindow()
                win32gui.SetForegroundWindow(hwnd)
        except Exception:
            pass
        self.setWindowOpacity(opacity)

    def destroy_overlay(self):
        if self._closed:
            return
        self._closed = True
        _overlays.discard(self)
        try:
            self.hide()
            self.scene.clear()
            self.deleteLater()
        except Exception:
            pass


# ─── Switch animation registry ───────────────────────────────────────────────
# Each is frame(t, pc, pn, sw, sh, d): t in [0,1], pc=current pixmap item,
# pn=next pixmap item, d=direction (+1/-1). Each call fully sets item state.

def _yrot(item, sw, sh, angle):
    tr = QtGui.QTransform()
    tr.translate(sw / 2, sh / 2)
    tr.rotate(angle, QtCore.Qt.YAxis)
    tr.translate(-sw / 2, -sh / 2)
    item.setTransform(tr)

def _a_slide(t, pc, pn, sw, sh, d):
    pc.setPos(d * t * sw, 0); pn.setPos(-d * (1 - t) * sw, 0)

def _a_fade(t, pc, pn, sw, sh, d):
    pc.setPos(0, 0); pn.setPos(0, 0); pc.setOpacity(1 - t); pn.setOpacity(t)

def _a_push(t, pc, pn, sw, sh, d):          # next slides over a static current
    pc.setPos(0, 0); pn.setPos(-d * (1 - t) * sw, 0)

def _a_zoom(t, pc, pn, sw, sh, d):
    pc.setPos(0, 0); pn.setPos(0, 0)
    pc.setOpacity(1 - t); pn.setOpacity(t)
    pc.setScale(1 + 0.18 * t); pn.setScale(0.82 + 0.18 * t)

def _a_cube(t, pc, pn, sw, sh, d):
    pc.setPos(0, 0); pn.setPos(0, 0)
    _yrot(pc, sw, sh, -d * 90 * t); _yrot(pn, sw, sh, d * 90 * (1 - t))
    pc.setOpacity(1 - t); pn.setOpacity(t)

def _a_flip(t, pc, pn, sw, sh, d):
    pc.setPos(0, 0); pn.setPos(0, 0)
    if t < 0.5:
        _yrot(pc, sw, sh, -d * 90 * (t / 0.5)); pc.setOpacity(1); pn.setOpacity(0)
    else:
        _yrot(pn, sw, sh, d * 90 * (1 - (t - 0.5) / 0.5)); pn.setOpacity(1); pc.setOpacity(0)

def _a_stack(t, pc, pn, sw, sh, d):         # next rises up over current
    pc.setPos(0, 0); pn.setPos(0, (1 - t) * sh); pc.setOpacity(1 - 0.35 * t)

ANIMATIONS = {
    "slide": _a_slide, "fade": _a_fade, "push": _a_push, "zoom": _a_zoom,
    "cube": _a_cube, "flip": _a_flip, "stack": _a_stack,
}


# ─── Animator (slide switch + dock picker) ───────────────────────────────────

class SlideAnimator:
    def __init__(self):
        self.sw, self.sh = screen_size()

    # ---- slide transition between two apps -----------------------------------
    def slide_transition(self, direction, current_hwnd, next_hwnd, on_done=None,
                         commit=True):
        wx, wy, sw, sh = work_area()
        cur = capture_window_image(current_hwnd) or Image.new("RGB", (sw, sh), (24, 24, 28))
        nxt = capture_window_image(next_hwnd) or Image.new("RGB", (sw, sh), (24, 24, 28))
        cur = cur.resize((sw, sh), Image.BILINEAR)
        nxt = nxt.resize((sw, sh), Image.BILINEAR)

        ov = QtOverlay(sw, sh, clickable=False, x=wx, y=wy, activate=True)
        _overlays.add(ov)
        pc = ov.scene.addPixmap(_pil2pix(cur))
        pn = ov.scene.addPixmap(_pil2pix(nxt))
        pc.setTransformOriginPoint(sw / 2, sh / 2)
        pn.setTransformOriginPoint(sw / 2, sh / 2)
        frame = ANIMATIONS.get(SETTINGS["animation"], _a_slide)
        frame(0.0, pc, pn, sw, sh, direction)     # initial state
        ov.reveal(1.0)

        done = {"f": False}
        def finish():
            if done["f"]:
                return
            done["f"] = True
            if commit:
                bring_to_front(next_hwnd)    # switch behind the last frame
                QtCore.QTimer.singleShot(160, nudge_cursor)
                QtCore.QTimer.singleShot(380, nudge_cursor)
            ov.destroy_overlay()
            if on_done:
                on_done()

        dur = int(float(SETTINGS.get("duration", ANIM_DURATION)) * 1000)
        anim = QtCore.QVariantAnimation(ov)
        anim.setStartValue(0.0)
        anim.setEndValue(1.0)
        anim.setDuration(max(80, dur))
        anim.setEasingCurve(QtCore.QEasingCurve.OutCubic)
        anim.valueChanged.connect(lambda t: frame(t, pc, pn, sw, sh, direction))
        anim.finished.connect(finish)
        ov._anim = anim
        anim.start()
        QtCore.QTimer.singleShot(max(80, dur) + 800, finish)

    # ---- picker (layout chosen from settings) --------------------------------
    def show_picker(self, windows, current_idx, on_result):
        import layouts
        n = len(windows)
        if n == 0:
            on_result(None)
            return
        sw, sh = self.sw, self.sh
        accent = accent_qcolor()
        step = float(SETTINGS.get("sensitivity", PICKER_STEP)) or PICKER_STEP

        imgs = []
        for hwnd, _ in windows:
            im = capture_window_image(hwnd)
            if not (im and im.width > 0 and im.height > 0):
                im = Image.new("RGB", (160, 100), (40, 42, 50))
            imgs.append(im)

        ov = QtOverlay(sw, sh, clickable=True)
        _overlays.add(ov)
        scene = ov.scene
        scene.addPixmap(_pil2pix(get_backdrop(sw, sh))).setZValue(0)
        hint = scene.addText("move fingers to pick   ·   lift to select   ·   Esc cancel",
                             QtGui.QFont("Segoe UI", 10))
        hint.setDefaultTextColor(QtGui.QColor(225, 230, 240)); hint.setZValue(8)
        hint.setPos((sw - hint.boundingRect().width()) / 2, 14)

        LayoutCls = layouts.LAYOUTS.get(SETTINGS.get("layout", "dock"), layouts.DockLayout)
        try:
            layout = LayoutCls(scene, imgs, windows, sw, sh, accent)
            layout.build()
        except Exception as e:
            print("[layout] build failed, fallback to dock:", e)
            layout = layouts.DockLayout(scene, imgs, windows, sw, sh, accent)
            layout.build()

        state = {"focus": float(current_idx), "selected": current_idx, "done": False}
        layout.update(state["focus"], state["selected"])

        timer = QtCore.QTimer(ov)
        def finish(idx):
            if state["done"]:
                return
            state["done"] = True
            timer.stop(); ov.destroy_overlay(); on_result(idx)

        import gesture_hook as _gh
        VK = {"esc": 0x1B, "left": 0x25, "right": 0x27, "enter": 0x0D, "lmb": 0x01}
        edge = {"down": False}
        fnav = {"base_x": None, "base_focus": float(current_idx), "armed": False}
        def held(vk):
            return _u32.GetAsyncKeyState(vk) & 0x8000

        def poll():
            if state["done"] or ov._closed:
                return
            live = _gh.LIVE
            if live["count"] >= 2:
                fnav["armed"] = True
                if fnav["base_x"] is None:
                    fnav["base_x"] = live["avg_x"]; fnav["base_focus"] = state["focus"]
                raw = fnav["base_focus"] + (live["avg_x"] - fnav["base_x"]) / step
                focus = max(0.0, min(n - 1, raw))
                if abs(focus - state["focus"]) > 0.002:
                    state["focus"] = focus
                    state["selected"] = int(round(focus))
                    layout.update(focus, state["selected"])
            elif live["count"] == 0 and fnav["armed"]:
                finish(state["selected"]); return
            esc, left, right, enter, lmb = (held(VK["esc"]), held(VK["left"]),
                                            held(VK["right"]), held(VK["enter"]), held(VK["lmb"]))
            any_down = esc or left or right or enter or lmb
            if any_down and not edge["down"]:
                edge["down"] = True
                if esc:
                    finish(None); return
                if left:
                    state["selected"] = (state["selected"] - 1) % n
                    state["focus"] = float(state["selected"]); layout.update(state["focus"], state["selected"])
                elif right:
                    state["selected"] = (state["selected"] + 1) % n
                    state["focus"] = float(state["selected"]); layout.update(state["focus"], state["selected"])
                elif enter:
                    finish(state["selected"]); return
                elif lmb:
                    pt = ctypes.wintypes.POINT(); _u32.GetCursorPos(ctypes.byref(pt))
                    hit = layout.hit_test(pt.x, pt.y)
                    finish(hit if hit is not None else state["selected"]); return
            elif not any_down:
                edge["down"] = False

        ov.reveal(0.0)
        fade = QtCore.QPropertyAnimation(ov, b"windowOpacity", ov)
        fade.setStartValue(0.0); fade.setEndValue(1.0); fade.setDuration(130)
        fade.setEasingCurve(QtCore.QEasingCurve.OutCubic)
        ov._fade = fade; fade.start()
        timer.timeout.connect(poll); timer.start(12)
        QtCore.QTimer.singleShot(12000, lambda: finish(None))


# ─── App Switcher Core ────────────────────────────────────────────────────────

class AppSwitcher:
    def __init__(self):
        self.animator = SlideAnimator()
        self.windows  = []
        self.current  = 0
        self._lock    = threading.Lock()
        self._busy    = False   # one overlay at a time

    def refresh_windows(self):
        wins = get_open_windows()
        try:
            fg  = win32gui.GetForegroundWindow()
            idx = next(i for i, (h, _) in enumerate(wins) if h == fg)
            wins = [wins[idx]] + wins[:idx] + wins[idx+1:]
        except Exception: pass
        with self._lock:
            self.windows = wins
            self.current = 0

    def _acquire(self):
        with self._lock:
            if self._busy:
                return False
            self._busy = True
            return True

    def _release(self):
        with self._lock:
            self._busy = False

    def switch(self, direction):
        if not self._acquire():
            return
        try:
            self.refresh_windows()
            with self._lock:
                wins = list(self.windows); cur = self.current
            if len(wins) < 2:
                self._release(); return
            nxt = (cur + direction) % len(wins)
            with self._lock:
                self.current = nxt
            if DEBUG:
                print(f"[switch] {len(wins)} apps, "
                      f"'{wins[cur][1][:20]}' -> '{wins[nxt][1][:20]}'", flush=True)
            self.animator.slide_transition(direction, wins[cur][0], wins[nxt][0],
                                           on_done=self._release)
        except Exception as e:
            print("[switch] error:", e)
            self._release()

    def show_picker(self):
        if not self._acquire():
            return
        try:
            self.refresh_windows()
            with self._lock:
                wins = list(self.windows); cur = self.current   # foreground at 0
            def on_result(idx):
                if idx is None or len(wins) < 2 or idx == cur:
                    if idx is not None:
                        with self._lock:
                            self.current = idx
                        bring_to_front(wins[idx][0])
                    self._release(); return
                with self._lock:
                    self.current = idx
                direction = 1 if idx > cur else -1
                self.animator.slide_transition(direction, wins[cur][0],
                                               wins[idx][0], on_done=self._release)
            self.animator.show_picker(wins, cur, on_result)
        except Exception as e:
            print("[picker] error:", e)
            self._release()

    def preview_anim(self):
        # run the current switch animation WITHOUT actually switching (settings)
        if not self._acquire():
            return
        try:
            self.refresh_windows()
            with self._lock:
                wins = list(self.windows)
            if len(wins) < 2:
                self._release(); return
            self.animator.slide_transition(1, wins[0][0], wins[1][0],
                                           on_done=self._release, commit=False)
        except Exception as e:
            print("[preview] error:", e)
            self._release()


_switcher = AppSwitcher()

# Gesture callbacks fire on the background listener thread; marshal onto the UI
# thread, where all Qt lives.
def handle_swipe(direction):
    post(lambda: _switcher.switch(direction))

def handle_hold_swipe(direction):
    post(lambda: _switcher.show_picker())

def handle_arm():
    post(prewarm_backdrop)

if __name__ == "__main__":
    print("AppSwitcher loaded. Run main.py to start.")
