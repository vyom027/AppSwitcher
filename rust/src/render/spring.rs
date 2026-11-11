#![allow(dead_code)]

#[derive(Debug, Clone, Copy)]
pub struct SpringConfig {
    pub tension: f64,  // Stiffness (k)
    pub friction: f64, // Damping (c)
    pub mass: f64,     // Mass (m)
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self {
            tension: 280.0,
            friction: 26.0,
            mass: 1.0,
        }
    }
}

impl SpringConfig {
    pub const BOUNCY: Self = Self {
        tension: 300.0,
        friction: 18.0,
        mass: 1.0,
    };

    pub const SNAPPY: Self = Self {
        tension: 340.0,
        friction: 28.0,
        mass: 0.9,
    };

    pub const SMOOTH: Self = Self {
        tension: 220.0,
        friction: 24.0,
        mass: 1.0,
    };
}

#[derive(Debug, Clone)]
pub struct Spring {
    pub value: f64,
    pub velocity: f64,
    pub target: f64,
    pub config: SpringConfig,
}

impl Spring {
    pub fn new(initial_value: f64, config: SpringConfig) -> Self {
        Self {
            value: initial_value,
            velocity: 0.0,
            target: initial_value,
            config,
        }
    }

    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    pub fn snap_to(&mut self, value: f64) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    fn acceleration(&self, x: f64, v: f64) -> f64 {
        let spring_force = -self.config.tension * (x - self.target);
        let damping_force = -self.config.friction * v;
        (spring_force + damping_force) / self.config.mass
    }

    /// Advance simulation using Runge-Kutta 4th order (RK4) integration
    pub fn update(&mut self, dt: f64) {
        let dt_step = dt.min(0.064); // Guard against huge frame drops

        let x0 = self.value;
        let v0 = self.velocity;

        // k1
        let a1 = self.acceleration(x0, v0);
        let dx1 = v0;

        // k2
        let x2 = x0 + dx1 * (0.5 * dt_step);
        let v2 = v0 + a1 * (0.5 * dt_step);
        let a2 = self.acceleration(x2, v2);
        let dx2 = v2;

        // k3
        let x3 = x0 + dx2 * (0.5 * dt_step);
        let v3 = v0 + a2 * (0.5 * dt_step);
        let a3 = self.acceleration(x3, v3);
        let dx3 = v3;

        // k4
        let x4 = x0 + dx3 * dt_step;
        let v4 = v0 + a3 * dt_step;
        let a4 = self.acceleration(x4, v4);
        let dx4 = v4;

        let d_pos = (dx1 + 2.0 * dx2 + 2.0 * dx3 + dx4) / 6.0;
        let d_vel = (a1 + 2.0 * a2 + 2.0 * a3 + a4) / 6.0;

        self.value += d_pos * dt_step;
        self.velocity += d_vel * dt_step;

        // Check if settled
        if (self.value - self.target).abs() < 0.001 && self.velocity.abs() < 0.005 {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }

    pub fn is_at_rest(&self) -> bool {
        (self.value - self.target).abs() < 0.001 && self.velocity.abs() < 0.005
    }
}
