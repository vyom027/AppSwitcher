use std::collections::HashMap;
use std::time::Instant;

pub const SWIPE_THRESHOLD: f64 = 200.0;
pub const DWELL_SECS: f64 = 0.20;
pub const PREWARM_SECS: f64 = 0.10;
pub const MOVE_EPS: f64 = 18.0;
pub const SPREAD_EPS: f64 = 12.0;
pub const PINCH_RATIO: f64 = 0.62;
pub const PINCH_MIN_BASE: f64 = 120.0;
pub const MIN_FINGERS: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct Contact {
    pub x: f64,
    pub y: f64,
    pub tip_down: bool,
}

pub struct GestureTracker {
    active: bool,
    base_x: f64,
    base_y: f64,
    last_x: f64,
    last_y: f64,
    still_since: Instant,
    picker_fired: bool,
    prewarmed: bool,
    base_spread: f64,
    last_spread: f64,
    pinched: bool,

    on_swipe: Box<dyn Fn(i32) + Send + Sync>,
    on_hold_swipe: Box<dyn Fn(i32) + Send + Sync>,
    on_arm: Option<Box<dyn Fn() + Send + Sync>>,
    on_pinch: Option<Box<dyn Fn() + Send + Sync>>,
}

impl GestureTracker {
    pub fn new<S, H, A, P>(on_swipe: S, on_hold_swipe: H, on_arm: Option<A>, on_pinch: Option<P>) -> Self
    where
        S: Fn(i32) + Send + Sync + 'static,
        H: Fn(i32) + Send + Sync + 'static,
        A: Fn() + Send + Sync + 'static,
        P: Fn() + Send + Sync + 'static,
    {
        Self {
            active: false,
            base_x: 0.0,
            base_y: 0.0,
            last_x: 0.0,
            last_y: 0.0,
            still_since: Instant::now(),
            picker_fired: false,
            prewarmed: false,
            base_spread: 0.0,
            last_spread: 0.0,
            pinched: false,
            on_swipe: Box::new(on_swipe),
            on_hold_swipe: Box::new(on_hold_swipe),
            on_arm: on_arm.map(|a| Box::new(a) as Box<dyn Fn() + Send + Sync>),
            on_pinch: on_pinch.map(|p| Box::new(p) as Box<dyn Fn() + Send + Sync>),
        }
    }

    fn reset(&mut self) {
        self.active = false;
        self.base_x = 0.0;
        self.base_y = 0.0;
        self.last_x = 0.0;
        self.last_y = 0.0;
        self.still_since = Instant::now();
        self.picker_fired = false;
        self.prewarmed = false;
        self.base_spread = 0.0;
        self.last_spread = 0.0;
        self.pinched = false;
    }

    fn calculate_spread(contacts: &HashMap<u32, (f64, f64)>, avg_x: f64, avg_y: f64) -> f64 {
        if contacts.is_empty() {
            return 0.0;
        }
        let sum: f64 = contacts
            .values()
            .map(|(x, y)| ((x - avg_x).powi(2) + (y - avg_y).powi(2)).sqrt())
            .sum();
        sum / contacts.len() as f64
    }

    pub fn feed(&mut self, contacts: &HashMap<u32, (f64, f64)>) {
        if contacts.len() != MIN_FINGERS {
            self.reset();
            return;
        }

        let count = contacts.len() as f64;
        let avg_x: f64 = contacts.values().map(|(x, _)| *x).sum::<f64>() / count;
        let avg_y: f64 = contacts.values().map(|(_, y)| *y).sum::<f64>() / count;
        let spread = Self::calculate_spread(contacts, avg_x, avg_y);
        let now = Instant::now();

        if !self.active {
            self.active = true;
            self.base_x = avg_x;
            self.base_y = avg_y;
            self.last_x = avg_x;
            self.last_y = avg_y;
            self.base_spread = spread;
            self.last_spread = spread;
            self.still_since = now;
            self.picker_fired = false;
            self.pinched = false;
            self.prewarmed = true;

            if let Some(ref on_arm) = self.on_arm {
                on_arm();
            }
            return;
        }

        if self.picker_fired || self.pinched {
            return;
        }

        // Pinch check: fingers collapsed inward toward centroid
        if self.base_spread >= PINCH_MIN_BASE && spread <= self.base_spread * PINCH_RATIO {
            self.pinched = true;
            if let Some(ref on_pinch) = self.on_pinch {
                on_pinch();
            }
            return;
        }

        // Stillness tracking
        if (avg_x - self.last_x).abs() + (avg_y - self.last_y).abs() > MOVE_EPS
            || (spread - self.last_spread).abs() > SPREAD_EPS
        {
            self.last_x = avg_x;
            self.last_y = avg_y;
            self.last_spread = spread;
            self.still_since = now;
            self.prewarmed = false;
        }

        let still = now.duration_since(self.still_since).as_secs_f64();

        // Imminent hold -> arm / prewarm
        if !self.prewarmed && still >= PREWARM_SECS {
            self.prewarmed = true;
            if let Some(ref on_arm) = self.on_arm {
                on_arm();
            }
        }

        // Dwell elapsed -> open picker
        if still >= DWELL_SECS {
            self.picker_fired = true;
            (self.on_hold_swipe)(0);
            return;
        }

        // Swipe travel check
        let dx = avg_x - self.base_x;
        let dy = (avg_y - self.base_y).abs();
        if dx.abs() >= SWIPE_THRESHOLD && dy < dx.abs() * 0.9 {
            let dir = if dx > 0.0 { 1 } else { -1 };
            (self.on_swipe)(dir);
            self.base_x = avg_x;
            self.base_y = avg_y;
            self.still_since = now;
        }
    }
}

