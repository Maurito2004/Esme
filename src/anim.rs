//! Resorte físico (spring) para las transiciones del notch.

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    stiffness: f32,
    damping: f32,
    eps: f32,
}

impl Spring {
    /// `bounce` 0.0 = amortiguamiento crítico, valores mayores = más rebote.
    pub fn new(value: f32, stiffness: f32, bounce: f32) -> Self {
        let critical = 2.0 * stiffness.sqrt();
        Self { value, velocity: 0.0, target: value, stiffness, damping: critical * (1.0 - bounce), eps: 0.0005 }
    }

    /// Tolerancia para considerar la animación terminada (en las unidades del valor).
    pub fn with_eps(mut self, eps: f32) -> Self {
        self.eps = eps;
        self
    }

    pub fn set_target(&mut self, t: f32) {
        self.target = t;
    }

    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < self.eps && self.velocity.abs() < self.eps * 10.0
    }

    /// Integra `dt` segundos con sub-pasos fijos (estable aunque haya frames lentos).
    pub fn step(&mut self, dt: f32) {
        let mut remaining = dt.min(0.1);
        const H: f32 = 1.0 / 240.0;
        while remaining > 0.0 {
            let h = remaining.min(H);
            let force = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            remaining -= h;
        }
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }
}

pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}
