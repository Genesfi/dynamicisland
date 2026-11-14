#![allow(dead_code)]

/// Spring dynamics implementation for organic, fluid iOS-like Dynamic Island animations.
#[derive(Debug, Clone)]
pub struct Spring {
    pub current: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl Spring {
    pub fn new(initial: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            current: initial,
            target: initial,
            velocity: 0.0,
            stiffness,
            damping,
            mass: 1.0,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn reset(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Advances the spring simulation by dt seconds (using semi-implicit Euler integration).
    pub fn update(&mut self, dt: f32) -> bool {
        let displacement = self.current - self.target;
        let spring_force = -self.stiffness * displacement;
        let damping_force = -self.damping * self.velocity;
        let acceleration = (spring_force + damping_force) / self.mass;

        self.velocity += acceleration * dt;
        self.current += self.velocity * dt;

        // Check if settled
        if (self.current - self.target).abs() < 0.25 && self.velocity.abs() < 0.5 {
            self.current = self.target;
            self.velocity = 0.0;
            false // Settled
        } else {
            true // Still moving
        }
    }

    pub fn is_settled(&self) -> bool {
        (self.current - self.target).abs() < 0.01 && self.velocity.abs() < 0.01
    }
}
