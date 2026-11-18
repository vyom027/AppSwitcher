# AppSwitcher

AppSwitcher is a native Rust app switcher for Windows 11 precision touchpads. It uses Direct2D for window previews and animations, and runs from a single EXE without a console window.

**[Download the latest Windows EXE](https://github.com/vyom027/AppSwitcher/releases/latest)**

## Try it and share feedback

The Rust version is available to try now. Download the EXE, use it with your normal apps, and tell us what feels good or needs work.

- [Review AppSwitcher 2.0 and share your experience](https://github.com/vyom027/AppSwitcher/discussions/1)
- [Report a bug](https://github.com/vyom027/AppSwitcher/issues/new/choose)
- [Fork the code](https://github.com/vyom027/AppSwitcher/fork) and send a pull request; see [CONTRIBUTING.md](CONTRIBUTING.md)

For useful feedback, mention your Windows version, touchpad model, picker layout, and the steps that reproduce any issue. A short screen recording helps with animation and gesture problems.

## Use it

1. Download the Rust EXE from Releases and place it in a permanent folder.
2. In Windows, open **Settings → Bluetooth & devices → Touchpad → Three-finger gestures** and set **Swipes** to **Nothing**. This lets AppSwitcher receive the gesture.
3. Close any older Python AppSwitcher instance from its tray icon, then run the Rust EXE.
4. Open the tray icon's **Settings** to choose the animation, layout, shortcuts, sensitivity, accent color, and windows to hide from the picker. Enable **Start with Windows** there if you want it.

The EXE contains its icon and the Microsoft C runtime. No Python installation or companion files are needed. Keep the EXE in its final location before enabling **Start with Windows**, since that setting records its path.

| Input | Action |
| --- | --- |
| Three-finger swipe | Switch to another app with an animation |
| Three-finger hold | Open the interactive app picker |
| Move fingers in the picker | Change the highlighted app |
| Lift fingers | Select the highlighted app |
| Alt+Tab or configured shortcut | Open the keyboard picker |
| Alt+` | Cycle between windows of the current app |
| Three-finger pinch | Send the configured shortcut |

Inside the picker:

| Input | Action |
| --- | --- |
| Arrow keys, Tab, mouse wheel | Move the highlight |
| Hover with the mouse | Highlight the card under the cursor |
| Home / End | Jump to the first or last window |
| 1–9 | Switch to that window immediately |
| Q, W, Delete, or middle-click | Close that window; the picker stays open |
| Enter or click | Switch to the highlighted window |
| Esc or click outside the cards | Cancel |

The picker opens on the monitor with the mouse cursor. Right-click the tray icon to open the switcher, pause gestures and Alt+Tab, or open Settings. Transitions play even when **Animation effects** is off in Windows settings. To have AppSwitcher follow that Windows setting instead, set `"respect_system_animations": true` in `settings.json`.

The settings include Slide, Duo Fold, Fade, Zoom, Cube, Flip, Push, and Stack animations. Picker layouts are Dock, Row, Hero, Exposé Cloud, and Spotlight. Settings are saved to `%APPDATA%\AppSwitcher\settings.json` on each PC.

## Build from source

Install the current Rust toolchain and Windows SDK, then run:

```powershell
cd rust
cargo build --release
```

The result is `rust\target\release\appswitcher.exe`. The build embeds the icon and links the C runtime statically. The project is currently built for 64-bit Windows using the MSVC Rust target.

## Python version

The earlier Python implementation remains in this repository for reference. Its setup and installer instructions are in [PYTHON_LEGACY.md](PYTHON_LEGACY.md). Run only one implementation at a time because both can listen for the same gestures and shortcut.

## License

MIT. See [LICENSE](LICENSE).
