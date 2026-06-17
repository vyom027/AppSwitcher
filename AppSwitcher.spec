# PyInstaller spec — build a windowed one-folder app.
#   pyinstaller --noconfirm AppSwitcher.spec
block_cipher = None

a = Analysis(
    ['main.py'],
    pathex=['.'],
    binaries=[],
    datas=[('icon.ico', '.'), ('Settings.qml', '.')],
    hiddenimports=[
        'switcher', 'layouts', 'gui', 'gesture_hook', 'qml_settings',
        'win32gui', 'win32ui', 'win32con', 'win32process',
        'PySide6.QtQml', 'PySide6.QtQuick', 'PySide6.QtQuickControls2',
    ],
    hookspath=[],
    runtime_hooks=[],
    excludes=['tkinter'],
    cipher=block_cipher,
)
pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz, a.scripts, [],
    exclude_binaries=True,
    name='AppSwitcher',
    console=False,            # no console window (tray app)
    icon='icon.ico',
)
coll = COLLECT(
    exe, a.binaries, a.zipfiles, a.datas,
    name='AppSwitcher',
)
