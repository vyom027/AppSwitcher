#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Instant;

pub const DEFAULT_SWIPE_THRESHOLD: f64 = 42.0;
pub const DWELL_SECS: f64 = 0.35;
pub const PREWARM_SECS: f64 = 0.12;
pub const MOVE_EPS: f64 = 14.0;
pub const SPREAD_EPS: f64 = 10.0;
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
    threshold: f64,
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
    swiped: bool,
    last_swipe: Instant,
    /// Set when a swipe fires; cleared only once the fingers lift (fewer than
    /// two contacts). One gesture = one switch, however far the fingers travel,
    /// and a contact briefly dropping out mid-swipe can't re-arm it.
    swipe_latched: bool,

    on_swipe: Box<dyn Fn(i32) + Send + Sync>,
    on_hold_swipe: Box<dyn Fn(i32) + Send + Sync>,
    on_arm: Option<Box<dyn Fn() + Send + Sync>>,
    on_pinch: Option<Box<dyn Fn() + Send + Sync>>,
}

impl GestureTracker {
    pub fn new<S, H, A, P>(
        threshold: f64,
        on_swipe: S,
        on_hold_swipe: H,
        on_arm: Option<A>,
        on_pinch: Option<P>,
    ) -> Self
    where
        S: Fn(i32) + Send + Sync + 'static,
        H: Fn(i32) + Send + Sync + 'static,
        A: Fn() + Send + Sync + 'static,
        P: Fn() + Send + Sync + 'static,
    {
        Self {
            threshold: Self::sanitize_threshold(threshold),
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
            swiped: false,
            last_swipe: Instant::now(),
            swipe_latched: false,
            on_swipe: Box::new(on_swipe),
            on_hold_swipe: Box::new(on_hold_swipe),
            on_arm: on_arm.map(|a| Box::new(a) as Box<dyn Fn() + Send + Sync>),
            on_pinch: on_pinch.map(|p| Box::new(p) as Box<dyn Fn() + Send + Sync>),
        }
    }

    fn sanitize_threshold(threshold: f64) -> f64 {
        if threshold.is_finite() && threshold > 0.0 {
            threshold
        } else {
            DEFAULT_SWIPE_THRESHOLD
        }
    }

    /// Update the swipe travel threshold without restarting the input thread.
    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = Self::sanitize_threshold(threshold);
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
        self.swiped = false;
        self.last_swipe = Instant::now();
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
        if contacts.len() < 2 {
            self.swipe_latched = false;
        }
        if contacts.len() != MIN_FINGERS {
            self.reset();
            return;
        }
        if self.swipe_latched {
            // Already switched for this gesture; ignore the rest of it
            // (including the stillness dwell that would open the picker).
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

        // Swipe travel check: moving past the threshold switches once; the
        // latch above ignores the rest of the gesture until the fingers lift.
        let dx = avg_x - self.base_x;
        let dy = (avg_y - self.base_y).abs();

        if dx.abs() >= self.threshold
            && dy < dx.abs() * 1.6
            && now.duration_since(self.last_swipe).as_millis() >= 95
        {
            let dir = if dx > 0.0 { 1 } else { -1 };
            self.last_swipe = now;
            self.base_x = avg_x;
            self.base_y = avg_y;
            self.still_since = now;
            self.swipe_latched = true;
            (self.on_swipe)(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Arc;

    fn three_fingers(x: f64) -> HashMap<u32, (f64, f64)> {
        // Spread wide enough that the pinch detector never triggers.
        HashMap::from([(1, (x, 100.0)), (2, (x + 150.0, 100.0)), (3, (x + 300.0, 100.0))])
    }

    fn tracker(count: Arc<AtomicI32>) -> GestureTracker {
        GestureTracker::new(
            24.0,
            move |dir| {
                count.fetch_add(dir, Ordering::SeqCst);
            },
            |_| {},
            None::<fn()>,
            None::<fn()>,
        )
    }

    /// Feed a swipe of `distance` units in 4-unit steps, ~8ms apart.
    fn swipe(t: &mut GestureTracker, from: f64, distance: f64) {
        let steps = (distance.abs() / 4.0) as i32;
        let step = distance.signum() * 4.0;
        for i in 0..=steps {
            t.feed(&three_fingers(from + i as f64 * step));
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }

    #[test]
    fn full_length_swipe_switches_once() {
        let count = Arc::new(AtomicI32::new(0));
        let mut t = tracker(count.clone());
        swipe(&mut t, 0.0, 400.0); // ~16x the threshold
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lifting_fingers_rearms_the_next_swipe() {
        let count = Arc::new(AtomicI32::new(0));
        let mut t = tracker(count.clone());
        swipe(&mut t, 0.0, 200.0);
        t.feed(&HashMap::new()); // fingers lifted
        swipe(&mut t, 0.0, -200.0);
        // Both swipes fired: +1 then -1.
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(t.swipe_latched);
    }

    #[test]
    fn brief_contact_dropout_does_not_rearm() {
        let count = Arc::new(AtomicI32::new(0));
        let mut t = tracker(count.clone());
        swipe(&mut t, 0.0, 100.0);
        // One finger drops out for a report, then returns mid-swipe.
        let mut two = three_fingers(100.0);
        two.remove(&3);
        t.feed(&two);
        swipe(&mut t, 100.0, 200.0);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
