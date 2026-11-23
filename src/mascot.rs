//! Mascota (Esme, el perrito, o el gato): ojitos que pestañean, miran hacia el cursor,
//! se duermen si no usás la PC y saludan al volver.

use std::time::{Duration, Instant};

pub const SLEEP_AFTER: Duration = Duration::from_secs(180);
/// Pestañeo en 4 cuadros clave por timer (no a la tasa de refresco): mismo efecto, ~1/4 de CPU.
const BLINK_KEYS: [f32; 4] = [0.45, 0.0, 0.5, 1.0];
pub const BLINK_STEP_MS: u32 = 45;
pub const GREET_LEN: Duration = Duration::from_millis(1800);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mood {
    Awake,
    Asleep,
    Greeting,
}

/// Lo que el renderer necesita para dibujar los ojos.
#[derive(Clone, Copy, Debug)]
pub struct Eyes {
    /// 1 = abiertos, 0 = cerrados.
    pub open: f32,
    pub mood: Mood,
    /// Hacia dónde miran (desplazamiento en DIPs, ±1,8).
    pub gaze: (f32, f32),
}

pub struct Mascot {
    pub mood: Mood,
    blink_step: Option<usize>,
    greet_until: Option<Instant>,
    seed: u32,
    gaze: (f32, f32),
}

impl Mascot {
    pub fn new() -> Self {
        let seed = std::process::id() ^ 0x9E37_79B9;
        Self { mood: Mood::Awake, blink_step: None, greet_until: None, seed, gaze: (0.0, 0.0) }
    }

    /// Milisegundos hasta el próximo pestañeo (aleatorio, 4–9 s: cada pestañeo cuesta unos frames).
    pub fn next_blink_ms(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        4000 + self.seed % 5000
    }

    /// Empieza un pestañeo. Devuelve true si hay que programar el próximo cuadro.
    pub fn blink(&mut self) -> bool {
        if self.mood == Mood::Awake {
            self.blink_step = Some(0);
            return true;
        }
        false
    }

    /// Avanza un cuadro del pestañeo. Devuelve true si quedan cuadros.
    pub fn blink_advance(&mut self) -> bool {
        match self.blink_step {
            Some(i) if i + 1 < BLINK_KEYS.len() => {
                self.blink_step = Some(i + 1);
                true
            }
            _ => {
                self.blink_step = None;
                false
            }
        }
    }

    /// Actualiza la mirada. Devuelve true si cambió lo suficiente como para redibujar.
    pub fn look(&mut self, g: (f32, f32)) -> bool {
        let changed = (g.0 - self.gaze.0).abs() > 0.25 || (g.1 - self.gaze.1).abs() > 0.25;
        if changed {
            self.gaze = g;
        }
        changed
    }

    pub fn sleep(&mut self) {
        self.mood = Mood::Asleep;
        self.blink_step = None;
    }

    pub fn greet(&mut self) {
        self.mood = Mood::Greeting;
        self.greet_until = Some(Instant::now() + GREET_LEN);
    }

    /// Llamar cuando vence el timer del saludo.
    pub fn end_greet(&mut self) {
        if self.mood == Mood::Greeting && self.greet_until.map(|t| Instant::now() >= t).unwrap_or(true) {
            self.mood = Mood::Awake;
            self.greet_until = None;
        }
    }

    pub fn eyes(&self) -> Eyes {
        let open = match (self.mood, self.blink_step) {
            (Mood::Asleep, _) => 0.0,
            (Mood::Awake, Some(i)) => BLINK_KEYS[i],
            _ => 1.0,
        };
        Eyes { open, mood: self.mood, gaze: if self.mood == Mood::Asleep { (0.0, 0.0) } else { self.gaze } }
    }
}
