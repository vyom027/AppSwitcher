"""
Low-level keyboard hook to replace Windows' Alt+Tab.

When enabled, Alt+Tab (and Alt+Shift+Tab) are SWALLOWED so Windows' own switcher
never fires, and a callback is invoked to drive AppSwitcher's picker instead.
Install from a thread that runs a message loop (the gesture thread does).
"""
import ctypes
import ctypes.wintypes as wt

user32 = ctypes.windll.user32

WH_KEYBOARD_LL = 13
WM_KEYDOWN, WM_KEYUP       = 0x0100, 0x0101
WM_SYSKEYDOWN, WM_SYSKEYUP = 0x0104, 0x0105
VK_TAB, VK_MENU, VK_LMENU, VK_RMENU = 0x09, 0x12, 0xA4, 0xA5
VK_SHIFT, VK_ESCAPE = 0x10, 0x1B

LRESULT  = ctypes.c_ssize_t
HOOKPROC = ctypes.WINFUNCTYPE(LRESULT, ctypes.c_int, wt.WPARAM, wt.LPARAM)

class KBDLLHOOKSTRUCT(ctypes.Structure):
    _fields_ = [("vkCode", wt.DWORD), ("scanCode", wt.DWORD),
                ("flags", wt.DWORD), ("time", wt.DWORD),
                ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]

user32.CallNextHookEx.restype  = LRESULT
user32.CallNextHookEx.argtypes = [wt.HHOOK, ctypes.c_int, wt.WPARAM, wt.LPARAM]
user32.SetWindowsHookExW.restype  = wt.HHOOK
user32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOKPROC, ctypes.c_void_p, wt.DWORD]
user32.UnhookWindowsHookEx.argtypes = [wt.HHOOK]
ctypes.windll.kernel32.GetModuleHandleW.restype = ctypes.c_void_p

_hook = None
_proc = None
_on_alttab = None
enabled = True
DEBUG = False
_alt = False


def _foreground_is_fullscreen():
    """True if the foreground window covers the WHOLE monitor (likely a
    fullscreen game) — then we pass Alt+Tab through to Windows."""
    try:
        h = user32.GetForegroundWindow()
        if not h:
            return False
        r = wt.RECT()
        user32.GetWindowRect(wt.HWND(h), ctypes.byref(r))
        mw = user32.GetSystemMetrics(0); mh = user32.GetSystemMetrics(1)
        return (r.left <= 0 and r.top <= 0 and
                (r.right - r.left) >= mw and (r.bottom - r.top) >= mh)
    except Exception:
        return False


def _callback(nCode, wParam, lParam):
    global _alt
    if nCode == 0 and enabled:
        kb = ctypes.cast(lParam, ctypes.POINTER(KBDLLHOOKSTRUCT)).contents
        vk = kb.vkCode
        down = wParam == WM_KEYDOWN or wParam == WM_SYSKEYDOWN
        if vk in (VK_MENU, VK_LMENU, VK_RMENU):
            _alt = down
        elif vk == VK_TAB and down and _alt:
            if not _foreground_is_fullscreen():
                shift = user32.GetAsyncKeyState(VK_SHIFT) & 0x8000
                if DEBUG:
                    print(f"[kbd_hook] Alt+Tab swallowed dir={-1 if shift else 1}", flush=True)
                # Ctrl tap so the swallowed Tab doesn't leave Alt "alone" ->
                # avoids the focused app's menu bar activating on Alt release.
                try:
                    user32.keybd_event(0x11, 0, 0, 0)   # Ctrl down
                    user32.keybd_event(0x11, 0, 2, 0)   # Ctrl up
                except Exception:
                    pass
                try:
                    if _on_alttab:
                        _on_alttab(-1 if shift else 1)
                except Exception:
                    pass
                return 1            # swallow -> Windows Alt+Tab never fires
    return user32.CallNextHookEx(None, nCode, wParam, lParam)


def install(on_alttab):
    """Call on a thread that pumps messages (e.g. the gesture thread)."""
    global _hook, _proc, _on_alttab
    _on_alttab = on_alttab
    _proc = HOOKPROC(_callback)
    hmod = ctypes.windll.kernel32.GetModuleHandleW(None)
    _hook = user32.SetWindowsHookExW(WH_KEYBOARD_LL, _proc, hmod, 0)
    return _hook


def uninstall():
    global _hook
    if _hook:
        user32.UnhookWindowsHookEx(_hook)
        _hook = None
