pub mod gesture_tracker;
pub mod hid_parser;
pub mod keyboard_hook;
pub mod raw_input;

pub use raw_input::{get_live_state, run_input_thread, set_sensitivity, stop_input_thread};

use std::sync::atomic::{AtomicBool, Ordering};

static PAUSED: AtomicBool = AtomicBool::new(false);

/// Pause/resume all gesture and Alt+Tab handling (tray "Pause AppSwitcher").
pub fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::SeqCst);
}

pub fn is_paused() -> bool {
    PAUSED.load(Ordering::Relaxed)
}
