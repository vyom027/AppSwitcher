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

def main():
    if "--install" in sys.argv:
        install_startup()
        return
    if "--uninstall" in sys.argv:
        uninstall_startup()
        return

    check_deps()

    # Import after deps check
    import switcher
    import gesture_hook

    if "--debug" in sys.argv:
        gesture_hook.DEBUG = True
        switcher.DEBUG = True
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
        )
    threading.Thread(target=listen, daemon=True).start()

    # Open the settings window on a MANUAL launch (double-click / after install),
    # but NOT when Windows starts it automatically (--startup).
    manual = "--startup" not in sys.argv

    try:
        switcher.run_ui(open_settings=manual)   # owns Qt; blocks until exit
    except KeyboardInterrupt:
        print("\n[exit] AppSwitcher stopped.")

if __name__ == "__main__":
    main()
