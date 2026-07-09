"""
main.py — AppSwitcher entry point
Run: python main.py
     python main.py --install   (add to Windows startup)
     python main.py --uninstall
"""

import sys
import os
import winreg
import subprocess

def _startup_command():
    # --startup => launched by Windows autostart: run gestures only, no GUI.
    if getattr(sys, "frozen", False):
        return f'"{sys.executable}" --startup'
    return f'"{sys.executable}" "{os.path.abspath(__file__)}" --startup'

def install_startup():
    key_path = r"Software\Microsoft\Windows\CurrentVersion\Run"
    value = _startup_command()
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key_path, 0, winreg.KEY_SET_VALUE) as k:
        winreg.SetValueEx(k, "AppSwitcher", 0, winreg.REG_SZ, value)
    print(f"[install] Added to startup:\n  {value}")

def uninstall_startup():
    key_path = r"Software\Microsoft\Windows\CurrentVersion\Run"
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key_path, 0, winreg.KEY_SET_VALUE) as k:
            winreg.DeleteValue(k, "AppSwitcher")
        print("[uninstall] Removed from startup.")
    except FileNotFoundError:
        print("[uninstall] Not found in startup.")

def check_deps():
    if getattr(sys, "frozen", False):
        return                    # bundled exe ships its deps
    missing = []
    for pkg in ["win32gui", "PIL", "PySide6"]:
        try:
            __import__(pkg)
        except ImportError:
            missing.append(pkg)
    if missing:
        pip_names = {"win32gui": "pywin32", "PIL": "Pillow", "PySide6": "PySide6"}
        pkgs = " ".join(pip_names.get(m, m) for m in missing)
        print(f"[setup] Missing packages: {pkgs}")
        print(f"[setup] Installing...")
        subprocess.check_call([sys.executable, "-m", "pip", "install"] + pkgs.split())
        print("[setup] Done. Restart main.py.")
        sys.exit(0)

_MUTEX_HANDLE = None

def acquire_single_instance(name="AppSwitcher_Running_Mutex"):
    """Create a named mutex; return False if another instance already holds it.

    The installer's AppMutex uses this same name to detect the running app and
    close/restart it during an in-place update — so you never have to uninstall
    first. It also stops a duplicate background instance (e.g. the installer's
    'launch now' firing after Restart Manager already relaunched us)."""
    global _MUTEX_HANDLE
    try:
        import ctypes
        k = ctypes.windll.kernel32
        k.CreateMutexW.restype = ctypes.c_void_p
        h = k.CreateMutexW(None, False, name)
        if not h:
            return True                      # can't create -> don't block startup
        _MUTEX_HANDLE = h                    # hold for process lifetime
        return k.GetLastError() != 183       # 183 = ERROR_ALREADY_EXISTS
    except Exception:
        return True

_SHOW_SETTINGS_EVENT = "AppSwitcher_ShowSettings_Event"

def _event_handle():
    import ctypes
    k = ctypes.windll.kernel32
    k.CreateEventW.restype  = ctypes.c_void_p
    k.CreateEventW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_wchar_p]
    # auto-reset, initially unset; same name returns a handle to the existing one
    return k, k.CreateEventW(None, False, False, _SHOW_SETTINGS_EVENT)

def signal_show_settings():
    """2nd instance -> wake the already-running instance to open Settings."""
    try:
        k, h = _event_handle()
        if h:
            k.SetEvent.argtypes = [ctypes.c_void_p]
            k.SetEvent(h)
    except Exception:
        pass

def _watch_show_settings():
    """1st instance: block on the event; each signal opens the Settings window
    on the Qt thread. Lets a later double-click focus this instance instead of
    silently doing nothing."""
    try:
        import time
        k, h = _event_handle()
        if not h:
            return
        k.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        while True:
            if k.WaitForSingleObject(h, 0xFFFFFFFF) != 0:   # 0 = WAIT_OBJECT_0
                continue
            try:
                import switcher, gui
                for _ in range(50):                          # wait for Qt bridge
                    if getattr(switcher, "_bridge", None) is not None:
                        switcher.post(gui.show_settings)
                        break
                    time.sleep(0.05)
            except Exception:
                pass
    except Exception:
        pass

def main():
    if "--install" in sys.argv:
        install_startup()
        return
    if "--uninstall" in sys.argv:
        uninstall_startup()
        return

    check_deps()

    if not acquire_single_instance():
        # Already running: ask that instance to surface its Settings, then exit.
        signal_show_settings()
        print("[exit] AppSwitcher is already running — opened its Settings.")
        return

    # Import after deps check
    import switcher
    import gesture_hook

    if "--debug" in sys.argv:
        gesture_hook.DEBUG = True
        switcher.DEBUG = True
        try:
            import kbd_hook; kbd_hook.DEBUG = True
        except Exception:
            pass
        print("[debug] verbose tracing ON")

    print("=" * 50)
    print("  AppSwitcher running")
    print("  3-finger swipe (keep moving) -> switch apps")
    print("  3-finger hold still ~0.2s    -> app picker (move to pick, lift)")
    print("  Ctrl+C to quit")
    print("=" * 50)

    import threading

    # The gesture listener has its own Win32 message loop, so it runs on a
    # background thread. All tkinter / overlay work happens on the main thread
    # via switcher.run_ui(); gesture callbacks marshal onto it through a queue.
    def listen():
        gesture_hook.start(
            on_swipe      = switcher.handle_swipe,
            on_hold_swipe = switcher.handle_hold_swipe,
            on_arm        = switcher.handle_arm,
            on_alttab     = switcher.handle_alttab,
            on_pinch      = switcher.handle_pinch,
        )
    threading.Thread(target=listen, daemon=True).start()
    # Listen for a later launch asking us to show Settings (focus-existing).
    threading.Thread(target=_watch_show_settings, daemon=True).start()

    # Open the settings window on a MANUAL launch (double-click / after install),
    # but NOT when Windows starts it automatically (--startup).
    manual = "--startup" not in sys.argv

    try:
        switcher.run_ui(open_settings=manual)   # owns Qt; blocks until exit
    except KeyboardInterrupt:
        print("\n[exit] AppSwitcher stopped.")

if __name__ == "__main__":
    main()
