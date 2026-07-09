"""
gesture_hook.py — Precision Touchpad 3-finger swipe listener (Raw Input)

Uses the Raw Input API (RegisterRawInputDevices + RIDEV_INPUTSINK) so a plain,
unprivileged python.exe receives touchpad HID reports even while in the
background. Each WM_INPUT report is parsed with the HidP_* API to recover the
per-finger contact state (tip-down, X, Y). A GestureTracker turns that stream
into swipe / swipe+hold events.

Why not WM_POINTER + RegisterPointerInputTarget?
  RegisterPointerInputTarget requires UIAccess (signed binary in a trusted
  location). A normal script can't get global pointer input that way. Raw Input
  with RIDEV_INPUTSINK has no such requirement.

Run standalone to see what your touchpad reports:
    python gesture_hook.py            # prints swipes
    python gesture_hook.py --debug    # prints every frame's finger contacts
"""

import ctypes
import ctypes.wintypes as wt
import threading
import time
import math
import sys

user32   = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
hid      = ctypes.windll.hid

DEBUG = False

# ─── Constants ────────────────────────────────────────────────────────────────

WM_INPUT             = 0x00FF
RID_INPUT            = 0x10000003
RIDI_PREPARSEDDATA   = 0x20000005
RIM_TYPEHID          = 2
RIDEV_INPUTSINK      = 0x00000100
HWND_MESSAGE         = -3

# HID usage pages / usages
UP_GENERIC   = 0x01
U_X          = 0x30
U_Y          = 0x31
UP_DIGITIZER = 0x0D
U_TOUCHPAD   = 0x05
U_TIPSWITCH  = 0x42
U_CONTACTID  = 0x51
U_CONTACTCNT = 0x54

HIDP_INPUT          = 0
HIDP_STATUS_SUCCESS = 0x00110000

if ctypes.sizeof(ctypes.c_void_p) == 8:
    LRESULT = ctypes.c_longlong
    WPARAM  = ctypes.c_ulonglong
else:
    LRESULT = ctypes.c_long
    WPARAM  = ctypes.c_ulong

# ─── ctypes signatures ────────────────────────────────────────────────────────

user32.DefWindowProcW.restype  = LRESULT
user32.DefWindowProcW.argtypes = [wt.HWND, ctypes.c_uint, WPARAM, wt.LPARAM]

user32.RegisterClassExW.restype  = ctypes.c_uint16
user32.RegisterClassExW.argtypes = [ctypes.c_void_p]

user32.CreateWindowExW.restype  = ctypes.c_void_p
user32.CreateWindowExW.argtypes = [
    ctypes.c_ulong, ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_ulong,
    ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
    ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
]

user32.GetRawInputData.restype  = ctypes.c_uint
user32.GetRawInputData.argtypes = [
    ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p,
    ctypes.POINTER(ctypes.c_uint), ctypes.c_uint,
]

user32.GetRawInputDeviceInfoW.restype  = ctypes.c_uint
user32.GetRawInputDeviceInfoW.argtypes = [
    ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint),
]

WNDPROCTYPE = ctypes.WINFUNCTYPE(LRESULT, wt.HWND, ctypes.c_uint, WPARAM, wt.LPARAM)

# ─── Win32 / HID structures ───────────────────────────────────────────────────

class WNDCLASSEX(ctypes.Structure):
    _fields_ = [
        ("cbSize", ctypes.c_uint), ("style", ctypes.c_uint),
        ("lpfnWndProc", WNDPROCTYPE),
        ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
        ("hInstance", wt.HANDLE), ("hIcon", wt.HANDLE), ("hCursor", wt.HANDLE),
        ("hbrBackground", wt.HANDLE), ("lpszMenuName", wt.LPCWSTR),
        ("lpszClassName", wt.LPCWSTR), ("hIconSm", wt.HANDLE),
    ]

class RAWINPUTDEVICE(ctypes.Structure):
    _fields_ = [
        ("usUsagePage", ctypes.c_ushort), ("usUsage", ctypes.c_ushort),
        ("dwFlags", ctypes.c_ulong), ("hwndTarget", ctypes.c_void_p),
    ]

class RAWINPUTHEADER(ctypes.Structure):
    _fields_ = [
        ("dwType", ctypes.c_ulong), ("dwSize", ctypes.c_ulong),
        ("hDevice", ctypes.c_void_p), ("wParam", WPARAM),
    ]

class HIDP_CAPS(ctypes.Structure):
    _fields_ = [
        ("Usage", ctypes.c_ushort), ("UsagePage", ctypes.c_ushort),
        ("InputReportByteLength", ctypes.c_ushort),
        ("OutputReportByteLength", ctypes.c_ushort),
        ("FeatureReportByteLength", ctypes.c_ushort),
        ("Reserved", ctypes.c_ushort * 17),
        ("NumberLinkCollectionNodes", ctypes.c_ushort),
        ("NumberInputButtonCaps", ctypes.c_ushort),
        ("NumberInputValueCaps", ctypes.c_ushort),
        ("NumberInputDataIndices", ctypes.c_ushort),
        ("NumberOutputButtonCaps", ctypes.c_ushort),
        ("NumberOutputValueCaps", ctypes.c_ushort),
        ("NumberOutputDataIndices", ctypes.c_ushort),
        ("NumberFeatureButtonCaps", ctypes.c_ushort),
        ("NumberFeatureValueCaps", ctypes.c_ushort),
        ("NumberFeatureDataIndices", ctypes.c_ushort),
    ]

class _VC_RANGE(ctypes.Structure):
    _fields_ = [
        ("UsageMin", ctypes.c_ushort), ("UsageMax", ctypes.c_ushort),
        ("StringMin", ctypes.c_ushort), ("StringMax", ctypes.c_ushort),
        ("DesignatorMin", ctypes.c_ushort), ("DesignatorMax", ctypes.c_ushort),
        ("DataIndexMin", ctypes.c_ushort), ("DataIndexMax", ctypes.c_ushort),
    ]

class _VC_NOTRANGE(ctypes.Structure):
    _fields_ = [
        ("Usage", ctypes.c_ushort), ("Reserved1", ctypes.c_ushort),
        ("StringIndex", ctypes.c_ushort), ("Reserved2", ctypes.c_ushort),
        ("DesignatorIndex", ctypes.c_ushort), ("Reserved3", ctypes.c_ushort),
        ("DataIndex", ctypes.c_ushort), ("Reserved4", ctypes.c_ushort),
    ]

class _VC_UNION(ctypes.Union):
    _fields_ = [("Range", _VC_RANGE), ("NotRange", _VC_NOTRANGE)]

class HIDP_VALUE_CAPS(ctypes.Structure):
    _fields_ = [
        ("UsagePage", ctypes.c_ushort),
        ("ReportID", ctypes.c_ubyte), ("IsAlias", ctypes.c_ubyte),
        ("BitField", ctypes.c_ushort), ("LinkCollection", ctypes.c_ushort),
        ("LinkUsage", ctypes.c_ushort), ("LinkUsagePage", ctypes.c_ushort),
        ("IsRange", ctypes.c_ubyte), ("IsStringRange", ctypes.c_ubyte),
        ("IsDesignatorRange", ctypes.c_ubyte), ("IsAbsolute", ctypes.c_ubyte),
        ("HasNull", ctypes.c_ubyte), ("Reserved", ctypes.c_ubyte),
        ("BitSize", ctypes.c_ushort), ("ReportCount", ctypes.c_ushort),
        ("Reserved2", ctypes.c_ushort * 5),
        ("UnitsExp", ctypes.c_ulong), ("Units", ctypes.c_ulong),
        ("LogicalMin", ctypes.c_long), ("LogicalMax", ctypes.c_long),
        ("PhysicalMin", ctypes.c_long), ("PhysicalMax", ctypes.c_long),
        ("u", _VC_UNION),
    ]

hid.HidP_GetCaps.argtypes = [ctypes.c_void_p, ctypes.POINTER(HIDP_CAPS)]
hid.HidP_GetCaps.restype  = ctypes.c_long
hid.HidP_GetValueCaps.argtypes = [
    ctypes.c_int, ctypes.c_void_p, ctypes.POINTER(ctypes.c_ushort), ctypes.c_void_p,
]
hid.HidP_GetValueCaps.restype = ctypes.c_long
hid.HidP_GetUsageValue.argtypes = [
    ctypes.c_int, ctypes.c_ushort, ctypes.c_ushort, ctypes.c_ushort,
    ctypes.POINTER(ctypes.c_ulong), ctypes.c_void_p, ctypes.c_char_p, ctypes.c_ulong,
]
hid.HidP_GetUsageValue.restype = ctypes.c_long
hid.HidP_GetUsages.argtypes = [
    ctypes.c_int, ctypes.c_ushort, ctypes.c_ushort,
    ctypes.POINTER(ctypes.c_ushort), ctypes.POINTER(ctypes.c_ulong),
    ctypes.c_void_p, ctypes.c_char_p, ctypes.c_ulong,
]
hid.HidP_GetUsages.restype = ctypes.c_long


# ─── Per-device HID report layout (cached) ────────────────────────────────────

class DeviceLayout:
    """Parsed HID caps for one touchpad device: which link collections hold
    fingers, and where contact-count lives."""
    def __init__(self, preparsed):
        self.preparsed = preparsed
        self._buf = None        # holds preparsed-data buffer alive
        caps = HIDP_CAPS()
        if hid.HidP_GetCaps(preparsed, ctypes.byref(caps)) != HIDP_STATUS_SUCCESS:
            raise RuntimeError("HidP_GetCaps failed")
        self.input_len = caps.InputReportByteLength
        n = caps.NumberInputValueCaps
        arr = (HIDP_VALUE_CAPS * n)()
        count = ctypes.c_ushort(n)
        if hid.HidP_GetValueCaps(HIDP_INPUT, arr, ctypes.byref(count),
                                 preparsed) != HIDP_STATUS_SUCCESS:
            raise RuntimeError("HidP_GetValueCaps failed")

        self.finger_links = []        # link collections that expose an X value
        self.contactcount_link = None
        for i in range(count.value):
            vc = arr[i]
            usage = vc.u.NotRange.Usage if not vc.IsRange else vc.u.Range.UsageMin
            if vc.UsagePage == UP_GENERIC and usage == U_X:
                self.finger_links.append(vc.LinkCollection)
            elif vc.UsagePage == UP_DIGITIZER and usage == U_CONTACTCNT:
                self.contactcount_link = vc.LinkCollection
        # de-dup, preserve order
        seen = set()
        self.finger_links = [l for l in self.finger_links
                             if not (l in seen or seen.add(l))]

    def parse_report(self, report):
        """Return dict {contact_id: (x, y, tip_down)} for every contact slot
        present in this report (tip_down False = finger lifting/lifted)."""
        rep = ctypes.c_char_p(report)
        rlen = len(report)
        contacts = {}
        for link in self.finger_links:
            usages = (ctypes.c_ushort * 16)()
            ulen = ctypes.c_ulong(16)
            hid.HidP_GetUsages(HIDP_INPUT, UP_DIGITIZER, link, usages,
                               ctypes.byref(ulen), self.preparsed, rep, rlen)
            down = any(usages[i] == U_TIPSWITCH for i in range(ulen.value))
            x = ctypes.c_ulong(0); y = ctypes.c_ulong(0); cid = ctypes.c_ulong(0)
            hid.HidP_GetUsageValue(HIDP_INPUT, UP_GENERIC, link, U_X,
                                   ctypes.byref(x), self.preparsed, rep, rlen)
            hid.HidP_GetUsageValue(HIDP_INPUT, UP_GENERIC, link, U_Y,
                                   ctypes.byref(y), self.preparsed, rep, rlen)
            hid.HidP_GetUsageValue(HIDP_INPUT, UP_DIGITIZER, link, U_CONTACTID,
                                   ctypes.byref(cid), self.preparsed, rep, rlen)
            contacts[cid.value] = (x.value, y.value, down)
        return contacts


_layout_cache = {}   # hDevice -> DeviceLayout (or None if unparseable)

def _get_layout(hDevice):
    if hDevice in _layout_cache:
        return _layout_cache[hDevice]
    size = ctypes.c_uint(0)
    user32.GetRawInputDeviceInfoW(hDevice, RIDI_PREPARSEDDATA, None,
                                  ctypes.byref(size))
    if size.value == 0:
        _layout_cache[hDevice] = None
        return None
    buf = (ctypes.c_byte * size.value)()
    user32.GetRawInputDeviceInfoW(hDevice, RIDI_PREPARSEDDATA, buf,
                                  ctypes.byref(size))
    try:
        layout = DeviceLayout(ctypes.cast(buf, ctypes.c_void_p))
        layout._buf = buf   # keep preparsed buffer alive
        if DEBUG:
            print(f"[layout] dev={hDevice} finger_links={layout.finger_links} "
                  f"input_len={layout.input_len} "
                  f"contactcount_link={layout.contactcount_link}", flush=True)
        if not layout.finger_links:
            layout = None
    except Exception as e:
        if DEBUG:
            print("[layout] parse failed:", e, flush=True)
        layout = None
    _layout_cache[hDevice] = layout
    return layout


# ─── Gesture state machine ────────────────────────────────────────────────────

class GestureTracker:
    """3-finger gesture interpreter.

    While 3 fingers are down:
      • fingers converge toward the centroid (spread shrinks past PINCH_RATIO)
        -> fire the pinch shortcut, then lock the gesture until fingers lift
      • keep moving horizontally  -> switch app per SWIPE_THRESHOLD of travel
        (swipe again without lifting to keep stepping through apps)
      • hold roughly still for DWELL seconds -> open the picker; after that the
        picker itself reads live finger position to navigate, lift to select

    Pinch vs swipe separate naturally: a swipe moves the centroid horizontally
    while the spread stays constant; a pinch keeps the centroid still while the
    spread collapses. Whichever metric crosses its threshold first wins.
    """
    SWIPE_THRESHOLD = 200    # logical units of travel that triggers one switch
    DWELL           = 0.20   # seconds of stillness that opens the picker
    PREWARM         = 0.10   # stillness at which we pre-grab the backdrop
    MOVE_EPS        = 18     # per-sample travel under this counts as "still"
    MIN_FINGERS     = 3
    PINCH_RATIO     = 0.62   # spread <= base*this -> pinch (38% collapse)
    PINCH_MIN_BASE  = 120    # ignore tiny base spreads (noise / near-touching)
    SPREAD_EPS      = 12     # per-sample spread change that counts as "moving"

    def __init__(self, on_swipe, on_hold_swipe, on_arm=None, on_pinch=None):
        self.on_swipe      = on_swipe
        self.on_hold_swipe = on_hold_swipe
        self.on_arm        = on_arm     # fired when a hold looks imminent
        self.on_pinch      = on_pinch   # fired when 3 fingers pinch inward
        self._reset()

    def _reset(self):
        self._active     = False
        self._base_x     = 0.0   # reference for swipe-distance accumulation
        self._base_y     = 0.0
        self._last_x     = 0.0   # last position used for stillness detection
        self._last_y     = 0.0
        self._still_since = 0.0
        self._picker     = False  # picker already opened this gesture
        self._prewarmed  = False
        self._base_spread = 0.0   # finger spread when the gesture began
        self._last_spread = 0.0
        self._pinched    = False  # pinch already fired this gesture

    @staticmethod
    def _spread(contacts, avg_x, avg_y):
        """Mean distance of the fingers from their centroid."""
        return sum(math.hypot(x - avg_x, y - avg_y)
                   for x, y in contacts.values()) / len(contacts)

    def feed(self, contacts):
        """contacts: dict {contact_id: (x, y)} of fingers currently down."""
        if len(contacts) != self.MIN_FINGERS:   # exactly 3 fingers (not 4+)
            self._reset()
            return

        now   = time.time()
        avg_x = sum(x for x, _ in contacts.values()) / len(contacts)
        avg_y = sum(y for _, y in contacts.values()) / len(contacts)
        spread = self._spread(contacts, avg_x, avg_y)

        if not self._active:                 # fingers just reached 3
            self._active      = True
            self._base_x, self._base_y = avg_x, avg_y
            self._last_x, self._last_y = avg_x, avg_y
            self._base_spread = spread
            self._last_spread = spread
            self._still_since = now
            self._picker      = False
            self._pinched     = False
            # Prewarm immediately: capturing the windows now (in the background)
            # overlaps the finger travel, so a swipe's slide starts instantly.
            self._prewarmed   = True
            if self.on_arm:
                self.on_arm()
            return

        if self._picker or self._pinched:    # gesture already consumed
            return

        # pinch: fingers collapsed toward the centroid past the ratio. Checked
        # before swipe/hold so a symmetric pinch (centroid barely moves) can't be
        # mistaken for a hold, and locks the rest of the gesture once fired.
        if (self.on_pinch and self._base_spread >= self.PINCH_MIN_BASE
                and spread <= self._base_spread * self.PINCH_RATIO):
            self._pinched = True
            if DEBUG:
                print(f"[pinch] base={self._base_spread:.0f} -> {spread:.0f}",
                      flush=True)
            self.on_pinch(-1)
            return

        # stillness tracking — a shrinking spread also counts as motion so a
        # slow pinch doesn't trip the hold-picker before it completes.
        if (abs(avg_x - self._last_x) + abs(avg_y - self._last_y) > self.MOVE_EPS
                or abs(spread - self._last_spread) > self.SPREAD_EPS):
            self._last_x, self._last_y = avg_x, avg_y
            self._last_spread = spread
            self._still_since = now
            self._prewarmed = False          # moved again -> not a hold (yet)

        still = now - self._still_since

        # a hold looks imminent -> pre-grab the backdrop so the picker opens fast
        if not self._prewarmed and still >= self.PREWARM:
            self._prewarmed = True
            if self.on_arm:
                self.on_arm()

        # held still long enough -> open picker
        if still >= self.DWELL:
            self._picker = True
            self.on_hold_swipe(0)
            return

        # moved far enough horizontally -> switch (and re-arm for the next step)
        dx = avg_x - self._base_x
        dy = abs(avg_y - self._base_y)
        if abs(dx) >= self.SWIPE_THRESHOLD and dy < abs(dx) * 0.9:
            self.on_swipe(1 if dx > 0 else -1)
            self._base_x, self._base_y = avg_x, avg_y
            self._still_since = now          # don't instantly dwell after a switch


# ─── Raw Input message window ─────────────────────────────────────────────────

_tracker = None
_hwnd    = None

# Live finger tracking. This touchpad streams one contact per HID report, so we
# accumulate by contact-id and expire stale ones to know how many fingers are
# actually down right now.
CONTACT_TIMEOUT = 0.06          # seconds; lifted finger stops refreshing
_live = {}                      # cid -> (x, y, last_seen)
_last_finger_count = -1

# Public live snapshot other modules (the picker) can read each poll to drive
# selection with the fingers: number of fingers down + their average X.
LIVE = {"count": 0, "avg_x": 0.0, "ts": 0.0}

def _wnd_proc(hwnd, msg, wparam, lparam):
    if msg == WM_INPUT:
        _handle_raw_input(lparam)
    return user32.DefWindowProcW(hwnd, msg, wparam, lparam)

def _handle_raw_input(hRawInput):
    size = ctypes.c_uint(0)
    user32.GetRawInputData(hRawInput, RID_INPUT, None, ctypes.byref(size),
                           ctypes.sizeof(RAWINPUTHEADER))
    if size.value == 0:
        return
    buf = (ctypes.c_byte * size.value)()
    if user32.GetRawInputData(hRawInput, RID_INPUT, buf, ctypes.byref(size),
                              ctypes.sizeof(RAWINPUTHEADER)) != size.value:
        return

    header = ctypes.cast(buf, ctypes.POINTER(RAWINPUTHEADER)).contents
    if header.dwType != RIM_TYPEHID:
        return
    layout = _get_layout(header.hDevice)
    if layout is None:
        return

    # RAWHID = { DWORD dwSizeHid; DWORD dwCount; BYTE bRawData[]; }
    off = ctypes.sizeof(RAWINPUTHEADER)
    raw = bytes(buf)
    size_hid = int.from_bytes(raw[off:off+4], "little")
    count    = int.from_bytes(raw[off+4:off+8], "little")
    data_off = off + 8

    now = time.time()
    for i in range(count):
        report = raw[data_off + i*size_hid : data_off + (i+1)*size_hid]
        try:
            contacts = layout.parse_report(report)
        except Exception:
            continue
        for cid, (x, y, down) in contacts.items():
            if down:
                _live[cid] = (x, y, now)
            else:
                _live.pop(cid, None)   # finger lifted -> drop immediately

    # expire fingers that stopped refreshing (no lift report seen)
    for cid in [c for c, v in _live.items() if now - v[2] > CONTACT_TIMEOUT]:
        del _live[cid]

    snapshot = {cid: (v[0], v[1]) for cid, v in _live.items()}

    # publish live snapshot for finger-driven UI (picker navigation)
    LIVE["count"] = len(snapshot)
    if snapshot:
        LIVE["avg_x"] = sum(x for x, _ in snapshot.values()) / len(snapshot)
    LIVE["ts"] = now

    global _last_finger_count
    if DEBUG and len(snapshot) != _last_finger_count:
        _last_finger_count = len(snapshot)
        print(f"[fingers] {len(snapshot)} down: {snapshot}", flush=True)

    if _tracker:
        _tracker.feed(snapshot)


def _create_message_window():
    hinstance  = kernel32.GetModuleHandleW(None)
    class_name = "AppSwitcherGestureWnd"
    proc_cb    = WNDPROCTYPE(_wnd_proc)

    wc = WNDCLASSEX()
    wc.cbSize       = ctypes.sizeof(WNDCLASSEX)
    wc.lpfnWndProc  = proc_cb
    wc.hInstance    = hinstance
    wc.lpszClassName = class_name
    user32.RegisterClassExW(ctypes.byref(wc))   # 1410 = already registered, ok

    hwnd = user32.CreateWindowExW(
        0, class_name, "GestureWnd", 0, 0, 0, 0, 0,
        ctypes.c_void_p(HWND_MESSAGE),   # message-only window
        None, hinstance, None)
    if not hwnd:
        raise RuntimeError(f"CreateWindowEx failed: {kernel32.GetLastError()}")

    rid = RAWINPUTDEVICE()
    rid.usUsagePage = UP_DIGITIZER
    rid.usUsage     = U_TOUCHPAD
    rid.dwFlags     = RIDEV_INPUTSINK
    rid.hwndTarget  = hwnd
    if not user32.RegisterRawInputDevices(ctypes.byref(rid), 1,
                                          ctypes.sizeof(RAWINPUTDEVICE)):
        raise RuntimeError(
            f"RegisterRawInputDevices failed: {kernel32.GetLastError()}")

    global _hwnd
    _hwnd = hwnd
    return proc_cb   # caller must keep this alive


def run_message_loop():
    msg = wt.MSG()
    while True:
        r = user32.GetMessageW(ctypes.byref(msg), None, 0, 0)
        if r in (0, -1):
            break
        user32.TranslateMessage(ctypes.byref(msg))
        user32.DispatchMessageW(ctypes.byref(msg))


# ─── Public API ───────────────────────────────────────────────────────────────

def start(on_swipe, on_hold_swipe, on_arm=None, on_alttab=None, on_pinch=None):
    """
    on_swipe(direction)      — quick 3-finger swipe; direction 1=right, -1=left
    on_hold_swipe(direction) — held 3-finger swipe, same directions
    on_arm()                 — a hold looks imminent (use to pre-warm the picker)
    on_alttab(direction)     — Alt+Tab pressed (keyboard hook); replaces Win switch
    on_pinch(direction)      — 3 fingers pinched inward (fires the pinch shortcut)
    """
    global _tracker
    _tracker = GestureTracker(on_swipe, on_hold_swipe, on_arm, on_pinch)
    _cb = _create_message_window()   # keep reference alive
    if on_alttab is not None:
        try:
            import kbd_hook
            kbd_hook.install(on_alttab)   # LL keyboard hook on this thread
        except Exception as e:
            print("[kbd_hook] install failed:", e)
    print("[gesture_hook] Listening for 3-finger touchpad gestures...")
    run_message_loop()
    return _cb


if __name__ == "__main__":
    DEBUG = "--debug" in sys.argv

    def _swipe(d):
        print(f"SWIPE {'RIGHT' if d == 1 else 'LEFT'}")

    def _hold(d):
        print(f"HOLD+SWIPE {'RIGHT' if d == 1 else 'LEFT'}")

    def _pinch(d):
        print("PINCH IN")

    print("Move 3 fingers on the touchpad. Ctrl+C to quit.")
    if DEBUG:
        print("(debug: printing every report frame)")
    start(_swipe, _hold, on_pinch=_pinch)
