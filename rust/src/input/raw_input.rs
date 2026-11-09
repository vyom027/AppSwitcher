use crate::event::{AppEvent, WM_APP_EVENT};
use crate::input::gesture_tracker::GestureTracker;
use crate::input::hid_parser::{DeviceLayout, UP_DIGITIZER, U_TOUCHPAD};
use crate::input::keyboard_hook;
use crossbeam_channel::Sender;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEHID,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassExW,
    TranslateMessage, MSG, WINDOW_EX_STYLE, WM_INPUT, WNDCLASSEXW,
};

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct LiveFingerState {
    pub count: usize,
    pub avg_x: f64,
    pub timestamp: Instant,
}

static LIVE_STATE: LazyLock<RwLock<LiveFingerState>> = LazyLock::new(|| {
    RwLock::new(LiveFingerState {
        count: 0,
        avg_x: 0.0,
        timestamp: Instant::now(),
    })
});

pub fn get_live_state() -> LiveFingerState {
    *LIVE_STATE.read()
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static TRACKER: parking_lot::Mutex<Option<GestureTracker>> = parking_lot::Mutex::new(None);

static LAYOUT_CACHE: LazyLock<parking_lot::Mutex<HashMap<usize, Option<DeviceLayout>>>> =
    LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

static LIVE_CONTACTS: LazyLock<parking_lot::Mutex<HashMap<u32, (f64, f64, Instant)>>> =
    LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

const CONTACT_TIMEOUT: Duration = Duration::from_millis(60);

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if msg == WM_INPUT {
        handle_raw_input(HRAWINPUT(l_param.0 as *mut _));
    }
    DefWindowProcW(hwnd, msg, w_param, l_param)
}

unsafe fn handle_raw_input(h_raw_input: HRAWINPUT) {
    let mut size: u32 = 0;
    let header_size = size_of::<RAWINPUTHEADER>() as u32;

    GetRawInputData(h_raw_input, RID_INPUT, None, &mut size, header_size);
    if size == 0 {
        return;
    }

    let mut buf = vec![0u8; size as usize];
    if GetRawInputData(
        h_raw_input,
        RID_INPUT,
        Some(buf.as_mut_ptr() as *mut _),
        &mut size,
        header_size,
    ) != size
    {
        return;
    }

    let raw = &*(buf.as_ptr() as *const RAWINPUT);
    if raw.header.dwType != RIM_TYPEHID.0 {
        return;
    }

    let device_handle = raw.header.hDevice.0 as usize;
    let mut cache = LAYOUT_CACHE.lock();
    let layout_entry = cache.entry(device_handle).or_insert_with(|| {
        DeviceLayout::from_device_handle(raw.header.hDevice)
    });

    let layout = match layout_entry.as_ref() {
        Some(l) => l,
        None => return,
    };

    let rawhid = &raw.data.hid;
    let size_hid = rawhid.dwSizeHid as usize;
    let count = rawhid.dwCount as usize;

    let off = 32; // In 64-bit Windows, offset of bRawData in RAWINPUT is 32 bytes
    if off > buf.len() {
        return;
    }
    let raw_bytes = &buf[off..];

    let now = Instant::now();
    let mut live = LIVE_CONTACTS.lock();

    for i in 0..count {
        let start = i * size_hid;
        let end = start + size_hid;
        if end > raw_bytes.len() {
            break;
        }

        let report = &raw_bytes[start..end];
        let contacts = layout.parse_report(report);

        for (cid, (x, y, down)) in contacts {
            if down {
                live.insert(cid, (x, y, now));
            } else {
                live.remove(&cid);
            }
        }
    }

    // Prune stale contacts that lifted without report
    live.retain(|_, (_, _, last_seen)| now.duration_since(*last_seen) < CONTACT_TIMEOUT);

    let snapshot: HashMap<u32, (f64, f64)> = live
        .iter()
        .map(|(&cid, &(x, y, _))| (cid, (x, y)))
        .collect();

    let count = snapshot.len();
    let avg_x = if count > 0 {
        snapshot.values().map(|(x, _)| *x).sum::<f64>() / count as f64
    } else {
        0.0
    };

    *LIVE_STATE.write() = LiveFingerState {
        count,
        avg_x,
        timestamp: now,
    };

    if let Some(ref mut tracker) = *TRACKER.lock() {
        tracker.feed(&snapshot);
    }
}

pub fn run_input_thread(
    event_tx: Sender<AppEvent>,
    ui_hwnd_usize: usize,
    use_ctrl: bool,
    trigger_vk: u32,
) -> Result<(), windows::core::Error> {
    // 1. Setup gesture callbacks to post to UI thread
    let tx_swipe = event_tx.clone();
    let on_swipe = move |direction| {
        let _ = tx_swipe.send(AppEvent::Swipe(direction));
        unsafe {
            let _ = PostMessageW(HWND(ui_hwnd_usize as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
        }
    };

    let tx_hold = event_tx.clone();
    let on_hold = move |_dir| {
        let _ = tx_hold.send(AppEvent::Hold);
        unsafe {
            let _ = PostMessageW(HWND(ui_hwnd_usize as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
        }
    };

    let tx_pinch = event_tx.clone();
    let on_pinch = move || {
        let _ = tx_pinch.send(AppEvent::Pinch);
        unsafe {
            let _ = PostMessageW(HWND(ui_hwnd_usize as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
        }
    };

    let on_arm = || {
        crate::window::capture::prewarm_backdrop();
        let wins = crate::window::get_open_windows(&[], false);
        let mru = crate::window::sort_by_mru(wins);
        if !mru.is_empty() {
            let mut targets = vec![mru[0].hwnd];
            if mru.len() > 1 {
                targets.push(mru[1].hwnd);
                targets.push(mru[mru.len() - 1].hwnd);
            }
            crate::window::capture::prewarm_windows(&targets);
        }
    };

    *TRACKER.lock() = Some(GestureTracker::new(
        on_swipe,
        on_hold,
        Some(on_arm),
        Some(on_pinch),
    ));
    RUNNING.store(true, Ordering::SeqCst);

    unsafe {
        // 2. Install Low-Level Keyboard Hook on this thread
        if let Err(e) = keyboard_hook::install(event_tx.clone(), HWND(ui_hwnd_usize as *mut _), use_ctrl, trigger_vk) {
            eprintln!("[warn] Failed to install keyboard hook: {:?}", e);
        } else {
            println!("[input] Low-level keyboard hook active (Alt+Tab interception enabled)");
        }

        // 3. Create Message-Only Window for Raw Input
        let h_instance = GetModuleHandleW(None)?;
        let class_name = w!("AppSwitcherNativeGestureWnd");

        let mut wc = WNDCLASSEXW::default();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(window_proc);
        wc.hInstance = h_instance.into();
        wc.lpszClassName = class_name;

        let _ = RegisterClassExW(&wc);

        // HWND_MESSAGE (-3) creates a message-only window
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("AppSwitcherGestureTarget"),
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND(-3isize as *mut _),
            None,
            h_instance,
            None,
        )?;

        let rid = RAWINPUTDEVICE {
            usUsagePage: UP_DIGITIZER,
            usUsage: U_TOUCHPAD,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };

        if let Err(e) = RegisterRawInputDevices(&[rid], size_of::<RAWINPUTDEVICE>() as u32) {
            eprintln!("[warn] Precision Touchpad RawInput registration failed: {:?}", e);
        } else {
            println!("[input] Precision Touchpad RawInput listener active (RIDEV_INPUTSINK)");
        }

        // 4. Input Thread Message Loop (pumps both RawInput and WH_KEYBOARD_LL)
        let mut msg = MSG::default();
        while RUNNING.load(Ordering::Relaxed) {
            let res = GetMessageW(&mut msg, None, 0, 0);
            if res.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        keyboard_hook::uninstall();
    }

    Ok(())
}

pub fn stop_input_thread() {
    RUNNING.store(false, Ordering::SeqCst);
}
