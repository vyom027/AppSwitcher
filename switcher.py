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

# Set DPI awareness to per-monitor-v2 BEFORE Qt loads (matches Qt's default, so
# Qt doesn't fail with "SetProcessDpiAwarenessContext: Access is denied"). Use
# physical pixels everywhere (overlay math relies on GetSystemMetrics).
try:
    _setctx = ctypes.windll.user32.SetProcessDpiAwarenessContext
    _setctx.argtypes = [ctypes.c_void_p]
    _setctx(ctypes.c_void_p(-4))     # DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
except Exception:
    try:
        ctypes.windll.user32.SetProcessDPIAware()
    except Exception:
        pass

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
    "alt_tab":    True,       # replace Windows Alt+Tab with this switcher
    "blocklist":  [],         # window titles to hide from the switcher
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
    blocked = set(SETTINGS.get("blocklist", []))
    if blocked:
        windows = [(h, t) for (h, t) in windows if t not in blocked]
    return windows

def list_open_titles():
    """Titles of every currently-open alt-tab window, IGNORING the blocklist —
    feeds the Settings 'hide window' editor (one entry per window title)."""
    titles, seen = [], set()
    def cb(hwnd, _):
        if _is_alt_tab_window(hwnd):
            t = win32gui.GetWindowText(hwnd)
            if t and t not in seen:
                seen.add(t); titles.append(t)
        return True
    win32gui.EnumWindows(cb, None)
    return titles

# ─── MRU (most-recently-used) ordering ────────────────────────────────────────
# A background thread watches the foreground window; the switch/picker order then
# follows real focus recency (like native Alt+Tab) — so one swipe lands on your
# last app, and the picker lists apps newest-first.
_mru = []                        # hwnds, most-recent first
_mru_lock = threading.Lock()

def _mru_note(hwnd):
    if not hwnd:
        return
    with _mru_lock:
        if hwnd in _mru:
            _mru.remove(hwnd)
        _mru.insert(0, hwnd)
        del _mru[64:]            # cap

def _mru_rank(hwnd):
    with _mru_lock:
        try:
            return _mru.index(hwnd)
        except ValueError:
            return 1 << 30        # never-focused -> last

def mru_order(windows):
    """Stable-sort (hwnd, title) pairs by focus recency, newest first."""
    return sorted(windows, key=lambda w: _mru_rank(w[0]))

def _mru_watch():
    last = None
    while True:
        try:
            fg = win32gui.GetForegroundWindow()
            if fg and fg != last and _is_alt_tab_window(fg):
                _mru_note(fg); last = fg
        except Exception:
            pass
        time.sleep(0.25)

_mru_started = [False]
def start_mru_watch():
    if _mru_started[0]:
        return
    _mru_started[0] = True
    threading.Thread(target=_mru_watch, daemon=True).start()

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
    user32 = ctypes.windll.user32   # DPI awareness already set at import
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

_bg_cache = {"img": None, "ts": 0.0, "render": {}}
_bg_lock  = threading.Lock()       # serialize ImageGrab: concurrent GDI screen
                                   # grabs (UI + prewarm threads) can hang

def prewarm_backdrop():
    sw, sh = screen_size()
    with _bg_lock:                             # serialize the ImageGrab vs others
        _bg_cache["img"] = _frosted_backdrop(sw, sh)
        _bg_cache["ts"]  = time.time()
        _bg_cache["render"] = {}               # invalidate render-sized copies

def get_backdrop(sw, sh):
    with _bg_lock:
        img = _bg_cache["img"]
        fresh = img is not None and img.size == (sw, sh) and time.time() - _bg_cache["ts"] < 1.5
    if fresh:
        return img
    prewarm_backdrop()
    return _bg_cache["img"]

def _render_backdrop(rw, rh):
    """A copy of the frosted backdrop at render size — NEVER triggers ImageGrab
    (compose runs on the UI thread and from prewarm threads; grabbing there risks
    a GDI hang). Falls back to a solid fill until prewarm_backdrop has run."""
    with _bg_lock:
        base = _bg_cache["img"]
        rcache = _bg_cache["render"]
        r = rcache.get((rw, rh))
        if r is None and base is not None:
            r = base.resize((rw, rh), Image.BILINEAR).convert("RGBA")
            rcache[(rw, rh)] = r
    if r is None:
        r = Image.new("RGBA", (rw, rh), (16, 18, 24, 255))
    return r.copy()                            # caller pastes onto it

# ─── Slide frame cache ────────────────────────────────────────────────────────
# The swipe freeze was NOT the window capture — it was building two full-screen
# QPixmaps from PIL on the UI thread every switch (~50ms typical, spiking to
# ~1s under GC pressure). So we split the work:
#   • BACKGROUND (prewarm, fired when 3 fingers land): capture + resize +
#     convert to raw RGBA bytes, keyed by hwnd. All the heavy CPU/IO lives here.
#   • UI THREAD (slide): turn ready bytes into a QPixmap once, cache the QPixmap,
#     and reuse it across switches until the capture goes stale.
# Result: a warm swipe builds in a few ms instead of 50–950ms.
_frame_cache  = {}              # hwnd -> {"bytes","w","h","ts","pix"}
_frame_lock   = threading.Lock()
_slide_active = [False]         # True while a slide animates: defer heavy
                                # pixmap pre-builds so they don't stall frames
CAP_TTL       = 2.0             # seconds a cached frame stays fresh
RENDER_MAX   = 1920            # cap the long edge of slide frames; the view
                               # stretches them back to full screen. On a 4K
                               # panel this is 4x fewer pixels to build/composite
                               # — and the downscale is invisible mid-motion.

def _render_size(sw, sh):
    s = min(1.0, RENDER_MAX / float(max(sw, sh)))
    return max(1, int(sw * s)), max(1, int(sh * s))

def _grab_screen_rect(hwnd):
    """Instant capture of a *visible* window straight off the composited screen
    (no PrintWindow repaint). Only valid for the foreground / unoccluded window."""
    try:
        l, t, r, b = win32gui.GetWindowRect(hwnd)
        if r - l <= 0 or b - t <= 0:
            return None
        with _bg_lock:                           # no concurrent GDI screen grabs
            return ImageGrab.grab(bbox=(l, t, r, b), all_screens=True)
    except Exception:
        return None

_OWN_PID = os.getpid()

def _is_own_window(hwnd):
    """True if hwnd belongs to THIS process (e.g. the Settings window)."""
    try:
        pid = ctypes.c_ulong(0)
        _u32.GetWindowThreadProcessId(ctypes.c_void_p(int(hwnd)), ctypes.byref(pid))
        return pid.value == _OWN_PID
    except Exception:
        return False

def _safe_capture(hwnd):
    """capture_window_image, but screen-grab our OWN windows. PrintWindow sends a
    synchronous WM_PRINT to the target; doing that to a window in our own process
    deadlocks/hangs against our UI thread — so never PrintWindow ourselves."""
    if _is_own_window(hwnd):
        return _grab_screen_rect(hwnd)
    return capture_window_image(hwnd)

def _bytes_to_pixmap(data, w, h):
    qim = QtGui.QImage(data, w, h, QtGui.QImage.Format.Format_RGBA8888)
    return QtGui.QPixmap.fromImage(qim)          # fromImage copies; no extra .copy()

def _compose_bytes(cap, hwnd, rw, rh):
    """Full-frame (rw x rh) RGBA bytes: the window image `cap` drawn at its REAL
    on-screen rect over the frosted backdrop. So a non-maximized window keeps its
    actual size/position in the slide instead of being stretched to fill."""
    wx, wy, sw, sh = work_area()
    sx, sy = rw / float(sw), rh / float(sh)
    canvas = _render_backdrop(rw, rh)            # cached; never grabs the screen
    try:
        l, t, r, b = win32gui.GetWindowRect(hwnd)
        dw = max(1, int(round((r - l) * sx))); dh = max(1, int(round((b - t) * sy)))
        dx = int(round((l - wx) * sx));         dy = int(round((t - wy) * sy))
        win = cap.convert("RGBA").resize((dw, dh), Image.BILINEAR)
        canvas.paste(win, (dx, dy), win)         # paste clips off-screen overflow
    except Exception:
        canvas.paste(cap.convert("RGBA").resize((rw, rh), Image.BILINEAR), (0, 0))
    return canvas.tobytes("raw", "RGBA")

def _frame_bytes(hwnd, w, h, fast=False):
    """Capture hwnd and return a composed full-frame's RGBA bytes, or None.
    fast=True screen-grabs the foreground window instead of PrintWindow."""
    cap = None
    if fast:
        try:
            if hwnd == _u32.GetForegroundWindow() and not win32gui.IsIconic(hwnd):
                cap = _grab_screen_rect(hwnd)
        except Exception:
            cap = None
    if cap is None:
        cap = _safe_capture(hwnd)
    if cap is None:
        return None
    return _compose_bytes(cap, hwnd, w, h)

def _cached_pixmap(hwnd, w, h):
    """Return a fresh cached QPixmap for hwnd at (w, h), or None — never builds
    or captures. Lets the slide prefer a warm pixmap over re-converting a PIL."""
    now = time.time()
    with _frame_lock:
        e = _frame_cache.get(hwnd)
        if (e and now - e["ts"] < CAP_TTL and e["w"] == w and e["h"] == h
                and e.get("pix") is not None):
            return e["pix"]
    return None

def _cache_pixmap_from_pil(hwnd, pil, w, h):
    """UI-thread: build a slide pixmap from an already-captured PIL frame
    (composed at the window's real rect) and cache it — warms the cache from the
    picker's own captures."""
    try:
        pix = _bytes_to_pixmap(_compose_bytes(pil, hwnd, w, h), w, h)
        with _frame_lock:
            _frame_cache[hwnd] = {"bytes": None, "w": w, "h": h,
                                  "ts": time.time(), "pix": pix}
    except Exception:
        pass

def _get_pixmap(hwnd, w, h):
    """UI-thread only: QPixmap for hwnd at (w, h). Reuses the prewarmed bytes /
    cached pixmap; only falls back to an inline capture on a cold miss."""
    now = time.time()
    with _frame_lock:
        e = _frame_cache.get(hwnd)
        fresh = bool(e and now - e["ts"] < CAP_TTL and e["w"] == w and e["h"] == h)
        if fresh and e.get("pix") is not None:
            return e["pix"]                  # hot: zero rebuild
        data = e["bytes"] if fresh else None
        ts   = e["ts"] if fresh else now
    if data is None:                         # cold: capture inline (slow path)
        data = _frame_bytes(hwnd, w, h, fast=True)
        if data is None:
            return None
        ts = now
    pix = _bytes_to_pixmap(data, w, h)
    with _frame_lock:                        # cache pixmap, drop the big bytes buf
        _frame_cache[hwnd] = {"bytes": None, "w": w, "h": h, "ts": ts, "pix": pix}
    return pix

def _prewarm_one(hwnd, w, h):
    data = _frame_bytes(hwnd, w, h)          # background: PrintWindow (occluded ok)
    if data is None:
        return
    with _frame_lock:
        _frame_cache[hwnd] = {"bytes": data, "w": w, "h": h,
                              "ts": time.time(), "pix": None}
    # Build the QPixmap NOW on the UI thread (idle during finger-down), so the
    # actual switch hits the cached-pixmap path (~ms) instead of paying the
    # full-screen fromImage + GC (which spiked to ~1s) on the hot path. But never
    # while a slide is animating — that heavy build would stall its frames; defer
    # until the animation is done (the pixmap is only needed next swipe anyway).
    def _warm():
        if _slide_active[0]:
            QtCore.QTimer.singleShot(60, _warm)
            return
        _get_pixmap(hwnd, w, h)
    post(_warm)

def prewarm_windows():
    """Pre-capture the foreground window + its two swipe-reachable neighbors so a
    swipe's slide starts without any synchronous capture/convert. Runs in PARALLEL
    (fire-and-forget). Background-thread safe; cheap on repeat (TTL-gated)."""
    try:
        wx, wy, sw, sh = work_area()
        w, h = _render_size(sw, sh)
        wins = get_open_windows()
        if not wins:
            return
        try:
            fg  = win32gui.GetForegroundWindow()
            idx = next(i for i, (hh, _) in enumerate(wins) if hh == fg)
            wins = [wins[idx]] + wins[:idx] + wins[idx+1:]
        except Exception:
            pass
        targets = {wins[0][0]}                # foreground + next / prev
        if len(wins) > 1:
            targets.add(wins[1][0]); targets.add(wins[-1][0])
        now = time.time()
        with _frame_lock:                     # prune stale entries
            for hh in [hh for hh, v in _frame_cache.items() if now - v["ts"] > 10.0]:
                del _frame_cache[hh]
            skip = {hh for hh in targets
                    if (v := _frame_cache.get(hh)) and now - v["ts"] < CAP_TTL}
        for hh in targets - skip:
            threading.Thread(target=_prewarm_one, args=(hh, w, h), daemon=True).start()
    except Exception:
        pass

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
_alttab_ctl = [None]     # set to the stepper fn while an Alt+Tab picker is open

def _pil2pix(im):
    """PIL Image -> QPixmap."""
    im = im.convert("RGBA")
    data = im.tobytes("raw", "RGBA")
    qim = QtGui.QImage(data, im.width, im.height, QtGui.QImage.Format.Format_RGBA8888)
    # fromImage already copies the pixels into the pixmap, and `data` is alive
    # for this whole call — so the old qim.copy() was a wasted full-image copy.
    return QtGui.QPixmap.fromImage(qim)

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
    start_mru_watch()                       # track focus recency for MRU order
    try:
        import kbd_hook
        kbd_hook.enabled = bool(SETTINGS.get("alt_tab", True))
    except Exception:
        pass
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
                         commit=True, cur_img=None, nxt_img=None):
        # cur_img/nxt_img: reuse already-captured images (e.g. from the picker)
        # to skip the ~150ms re-capture so the animation starts instantly.
        wx, wy, sw, sh = work_area()
        rw, rh = _render_size(sw, sh)             # reduced render resolution
        _fallback = lambda: _pil2pix(Image.new("RGB", (rw, rh), (24, 24, 28)))

        def _pix(hwnd, passed_img):
            # Prefer a warm cached pixmap (3-finger prewarm OR picker pre-cache).
            p = _cached_pixmap(hwnd, rw, rh)
            if p is not None:
                return p
            # else reuse the picker's PIL frame (no re-capture), or capture cold.
            if passed_img is not None:
                return _bytes_to_pixmap(_compose_bytes(passed_img, hwnd, rw, rh), rw, rh)
            return _get_pixmap(hwnd, rw, rh) or _fallback()

        _t1 = time.time()
        cpix = _pix(current_hwnd, cur_img)
        npix = _pix(next_hwnd, nxt_img)
        ov = QtOverlay(sw, sh, clickable=False, x=wx, y=wy, activate=True)
        _overlays.add(ov)
        if (rw, rh) != (sw, sh):                  # render small, stretch to fill
            ov.scene.setSceneRect(0, 0, rw, rh)
            ov.setSceneRect(0, 0, rw, rh)
            ov.resetTransform(); ov.scale(sw / rw, sh / rh)
        pc = ov.scene.addPixmap(cpix)
        pn = ov.scene.addPixmap(npix)
        pc.setTransformOriginPoint(rw / 2, rh / 2)
        pn.setTransformOriginPoint(rw / 2, rh / 2)
        frame = ANIMATIONS.get(SETTINGS["animation"], _a_slide)
        frame(0.0, pc, pn, rw, rh, direction)     # initial state
        _slide_active[0] = True                   # defer pre-builds until done
        _t2 = time.time()
        ov.reveal(1.0)
        if DEBUG:
            print(f"[slide] build={int((_t2-_t1)*1000)}ms "
                  f"reveal={int((time.time()-_t2)*1000)}ms", flush=True)
        _frame0 = {"t": time.time()}

        done = {"f": False}
        def finish():
            if done["f"]:
                return
            done["f"] = True
            _slide_active[0] = False
            if commit:
                bring_to_front(next_hwnd)    # switch behind the last frame
                QtCore.QTimer.singleShot(160, nudge_cursor)
                QtCore.QTimer.singleShot(380, nudge_cursor)
                # foreground changed -> rewarm so a follow-up swipe is instant
                threading.Thread(target=prewarm_windows, daemon=True).start()
            ov.destroy_overlay()
            if on_done:
                on_done()

        dur = int(float(SETTINGS.get("duration", ANIM_DURATION)) * 1000)
        anim = QtCore.QVariantAnimation(ov)
        anim.setStartValue(0.0)
        anim.setEndValue(1.0)
        anim.setDuration(max(80, dur))
        anim.setEasingCurve(QtCore.QEasingCurve.OutCubic)
        def _on_val(t):
            if DEBUG and _frame0["t"] is not None:
                print(f"[slide] first frame +{int((time.time()-_frame0['t'])*1000)}ms", flush=True)
                _frame0["t"] = None
            frame(t, pc, pn, rw, rh, direction)
        anim.valueChanged.connect(_on_val)
        anim.finished.connect(finish)
        ov._anim = anim
        anim.start()
        QtCore.QTimer.singleShot(max(80, dur) + 800, finish)

    # ---- picker (layout chosen from settings) --------------------------------
    def show_picker(self, windows, current_idx, on_result, mode="finger"):
        import layouts
        n = len(windows)
        if n == 0:
            on_result(None)
            return
        sw, sh = self.sw, self.sh
        accent = accent_qcolor()
        step = float(SETTINGS.get("sensitivity", PICKER_STEP)) or PICKER_STEP

        # Capture every window's thumbnail off the UI thread, THEN build the picker
        # on it. Two hazards this avoids:
        #   • PrintWindow of our OWN windows (Settings) deadlocks the UI thread
        #     (handled by _safe_capture -> screen grab);
        #   • PrintWindow of an UNRESPONSIVE window blocks forever — so we capture
        #     on daemon threads with a deadline and placeholder anything not ready,
        #     letting a hung capture leak harmlessly instead of freezing the picker.
        _ph = lambda: Image.new("RGB", (160, 100), (40, 42, 50))
        def _cap(hwnd):
            im = _safe_capture(hwnd)
            if not (im and im.width > 0 and im.height > 0):
                im = _ph()
            return im

        def _build(imgs):
            ov = QtOverlay(sw, sh, clickable=True)
            _overlays.add(ov)
            scene = ov.scene
            scene.addPixmap(_pil2pix(get_backdrop(sw, sh))).setZValue(0)
            hint_txt = ("hold Alt · Tab to cycle · release Alt to switch · Esc cancel"
                        if mode == "alttab"
                        else "move fingers to pick   ·   lift to select   ·   Esc cancel")
            hint = scene.addText(hint_txt, QtGui.QFont("Segoe UI", 10))
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

            # Warm the slide cache from the frames we already captured, so the slide
            # after selection is instant — matters most for keyboard Alt+Tab, which
            # gets no gesture prewarm. Built lazily (one per event-loop turn) so it
            # never delays the picker fade-in; reuses imgs, no re-capture.
            _, _, _fsw, _fsh = work_area()
            _rw, _rh = _render_size(_fsw, _fsh)
            def _precache(i=0):
                if ov._closed or i >= len(windows):
                    return
                _cache_pixmap_from_pil(windows[i][0], imgs[i], _rw, _rh)
                QtCore.QTimer.singleShot(0, lambda: _precache(i + 1))
            QtCore.QTimer.singleShot(0, _precache)

            # Semi-live thumbnails: re-capture the windows in the background every
            # ~0.4s while the picker is open and swap the images in place. Keeps
            # every layout (dock magnify, reflection, etc.); hung captures leak
            # harmlessly (daemon + deadline) and just keep the previous frame.
            def _live_loop():
                while not ov._closed and not state["done"]:
                    time.sleep(0.4)
                    if ov._closed or state["done"]:
                        break
                    fresh = [None] * len(windows)
                    cths = [threading.Thread(
                        target=lambda i=i: fresh.__setitem__(i, _safe_capture(windows[i][0])),
                        daemon=True) for i in range(len(windows))]
                    for ct in cths:
                        ct.start()
                    dl = time.time() + 0.6
                    for ct in cths:
                        ct.join(timeout=max(0.0, dl - time.time()))
                    if ov._closed or state["done"]:
                        break
                    def _apply(f=fresh):
                        if ov._closed or state["done"]:
                            return
                        changed = False
                        for i, im in enumerate(f):
                            if im is not None and im.width > 0 and im.height > 0:
                                imgs[i] = im
                                layout.refresh_image(i, im)
                                changed = True
                        if changed:
                            layout.update(state["focus"], state["selected"])
                    post(_apply)
            threading.Thread(target=_live_loop, daemon=True).start()

            timer = QtCore.QTimer(ov)
            def finish(idx):
                if state["done"]:
                    return
                state["done"] = True
                _alttab_ctl[0] = None
                timer.stop(); ov.destroy_overlay(); on_result(idx, imgs)

            import gesture_hook as _gh
            VK = {"esc": 0x1B, "left": 0x25, "right": 0x27, "enter": 0x0D,
                  "lmb": 0x01, "alt": 0x12}
            edge = {"down": False}
            fnav = {"base_x": None, "base_focus": float(current_idx), "armed": False}
            alt_state = {"down": True}        # Alt is held when alttab picker opens
            def held(vk):
                return _u32.GetAsyncKeyState(vk) & 0x8000

            # alttab mode: external stepper advances selection on each Alt+Tab
            if mode == "alttab":
                def _step(direction):
                    if state["done"]:
                        return
                    state["selected"] = (state["selected"] + direction) % n
                    state["focus"] = float(state["selected"])
                    layout.update(state["focus"], state["selected"])
                _alttab_ctl[0] = _step

            def poll():
                if state["done"] or ov._closed:
                    return
                if mode == "alttab":
                    a = held(VK["alt"])
                    if not a and alt_state["down"]:      # Alt released -> commit
                        finish(state["selected"]); return
                    alt_state["down"] = a
                    if held(VK["esc"]):
                        finish(None); return
                    if held(VK["lmb"]):
                        pt = ctypes.wintypes.POINT(); _u32.GetCursorPos(ctypes.byref(pt))
                        hit = layout.hit_test(pt.x, pt.y)
                        if hit is not None:
                            finish(hit); return
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

        def _capture_then_build():
            _tcap = time.time()
            hwnds = [h for h, _ in windows]
            imgs = [None] * n
            def _worker(i):
                imgs[i] = _cap(hwnds[i])
            ths = [threading.Thread(target=_worker, args=(i,), daemon=True)
                   for i in range(n)]
            for t in ths:
                t.start()
            deadline = time.time() + 1.0          # cap the wait; placeholder the rest
            for t in ths:
                t.join(timeout=max(0.0, deadline - time.time()))
            ready = sum(im is not None for im in imgs)
            imgs = [im if im is not None else _ph() for im in imgs]
            if DEBUG:
                print(f"[picker] captured {ready}/{n} windows in "
                      f"{int((time.time()-_tcap)*1000)}ms (rest placeholdered)", flush=True)
            post(lambda: _build(imgs))
        threading.Thread(target=_capture_then_build, daemon=True).start()


# ─── App Switcher Core ────────────────────────────────────────────────────────

class AppSwitcher:
    def __init__(self):
        self.animator = SlideAnimator()
        self.windows  = []
        self.current  = 0
        self._lock    = threading.Lock()
        self._busy    = False   # one overlay at a time
        self._pending_dir = None  # an Alt+Tab that arrived mid-animation, queued

    def refresh_windows(self):
        wins = mru_order(get_open_windows())     # newest-focused first
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
        pend = None
        with self._lock:
            self._busy = False
            if self._pending_dir is not None:
                pend = self._pending_dir
                self._pending_dir = None
        # A second Alt+Tab landed while we were busy animating the last switch.
        # The picker is already gone, so just slide one more step (Alt is almost
        # always released by now) instead of dropping the press.
        if pend is not None:
            post(lambda d=pend: self.switch(d))

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
            def on_result(idx, imgs=None):
                if idx is None or len(wins) < 2 or idx == cur:
                    if idx is not None:
                        with self._lock:
                            self.current = idx
                        bring_to_front(wins[idx][0])
                    self._release(); return
                with self._lock:
                    self.current = idx
                direction = 1 if idx > cur else -1
                ci = imgs[cur] if imgs else None
                ni = imgs[idx] if imgs else None
                self.animator.slide_transition(direction, wins[cur][0], wins[idx][0],
                                               on_done=self._release,
                                               cur_img=ci, nxt_img=ni)
            self.animator.show_picker(wins, cur, on_result)
        except Exception as e:
            print("[picker] error:", e)
            self._release()

    def alttab(self, direction):
        # If an Alt+Tab picker is already open, just advance its selection.
        if _alttab_ctl[0] is not None:
            _alttab_ctl[0](direction)
            return
        if not self._acquire():
            # Busy animating the previous switch — queue this press so it fires
            # the moment the lock frees, instead of being dropped (the cause of
            # "sometimes Alt+Tab does nothing, I have to press twice").
            with self._lock:
                self._pending_dir = direction
            if DEBUG:
                print(f"[alttab] busy -> queued dir={direction}", flush=True)
            return
        try:
            self.refresh_windows()
            with self._lock:
                wins = list(self.windows); cur = self.current   # foreground at 0
            if len(wins) < 2:
                self._release(); return
            start = (cur + direction) % len(wins)   # first Tab highlights next app
            def on_result(idx, imgs=None):
                if idx is None or idx == cur:
                    self._release(); return
                with self._lock:
                    self.current = idx
                d = 1 if idx >= cur else -1
                ci = imgs[cur] if imgs else None     # reuse picker captures =>
                ni = imgs[idx] if imgs else None     # animation starts instantly
                self.animator.slide_transition(d, wins[cur][0], wins[idx][0],
                                               on_done=self._release,
                                               cur_img=ci, nxt_img=ni)
            self.animator.show_picker(wins, start, on_result, mode="alttab")
        except Exception as e:
            print("[alttab] error:", e)
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
def _post_timed(tag, fn):
    """post(fn) but log how long it waited in the UI-thread queue before running
    — reveals lag from a clogged event loop that build= timing can't see."""
    t = time.time()
    def run():
        if DEBUG:
            print(f"[queue] {tag} waited {int((time.time()-t)*1000)}ms", flush=True)
        fn()
    post(run)

def handle_swipe(direction):
    _post_timed("swipe", lambda: _switcher.switch(direction))

def handle_hold_swipe(direction):
    _post_timed("hold", lambda: _switcher.show_picker())

def handle_arm():
    # Off the UI thread: PrintWindow/ImageGrab don't need Qt, and we must not
    # freeze the event loop while pre-capturing.
    def _warm():
        prewarm_backdrop()
        prewarm_windows()
    threading.Thread(target=_warm, daemon=True).start()

def handle_alttab(direction):
    # fired from the keyboard hook (background thread) on Alt+Tab
    _post_timed("alttab", lambda: _switcher.alttab(direction))

if __name__ == "__main__":
    print("AppSwitcher loaded. Run main.py to start.")
