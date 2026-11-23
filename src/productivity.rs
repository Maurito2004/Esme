//! Pomodoro y registro de agua (Fase 5). Todo local: %APPDATA%\Esme\agua.json.

use nanoserde::{DeJson, SerJson};
use std::time::{Duration, Instant};

pub const FOCUS: Duration = Duration::from_secs(25 * 60);
pub const BREAK: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Focus,
    Break,
}

/// Temporizador Pomodoro: 25 min de foco, 5 de descanso.
pub struct Pomodoro {
    pub mode: Mode,
    /// Cuándo termina si está corriendo.
    end_at: Option<Instant>,
    /// Lo que queda si está en pausa.
    left: Duration,
}

impl Pomodoro {
    pub fn new() -> Self {
        Self { mode: Mode::Focus, end_at: None, left: FOCUS }
    }

    pub fn running(&self) -> bool {
        self.end_at.is_some()
    }

    /// ¿Se usó desde que se abrió? (para mostrarlo en la píldora aunque esté en pausa).
    pub fn touched(&self) -> bool {
        self.running() || self.left != self.total()
    }

    pub fn total(&self) -> Duration {
        if self.mode == Mode::Focus { FOCUS } else { BREAK }
    }

    pub fn remaining(&self) -> Duration {
        match self.end_at {
            Some(t) => t.saturating_duration_since(Instant::now()),
            None => self.left,
        }
    }

    pub fn toggle(&mut self) {
        match self.end_at.take() {
            Some(t) => self.left = t.saturating_duration_since(Instant::now()),
            None => self.end_at = Some(Instant::now() + self.left),
        }
    }

    pub fn reset(&mut self) {
        self.mode = Mode::Focus;
        self.end_at = None;
        self.left = FOCUS;
    }

    /// Si terminó, pasa al siguiente modo (arrancando solo) y devuelve el modo que terminó.
    pub fn check_finished(&mut self) -> Option<Mode> {
        let t = self.end_at?;
        if Instant::now() < t {
            return None;
        }
        let done = self.mode;
        self.mode = if done == Mode::Focus { Mode::Break } else { Mode::Focus };
        self.left = self.total();
        self.end_at = Some(Instant::now() + self.left);
        Some(done)
    }

    /// "12:34"
    pub fn text(&self) -> String {
        let s = self.remaining().as_secs();
        format!("{:02}:{:02}", s / 60, s % 60)
    }

    /// Avance 0..1 dentro del período actual.
    pub fn progress(&self) -> f32 {
        1.0 - self.remaining().as_secs_f32() / self.total().as_secs_f32()
    }
}

#[derive(DeJson, SerJson, Default)]
pub struct Water {
    #[nserde(default)]
    pub date: String,
    #[nserde(default)]
    pub glasses: u32,
}

fn water_path() -> std::path::PathBuf {
    crate::config::data_dir().join("agua.json")
}

/// "2026-11-23" (fecha local).
pub fn today() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

impl Water {
    pub fn load() -> Self {
        let w: Water = std::fs::read_to_string(water_path())
            .ok()
            .and_then(|s| Water::deserialize_json(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default();
        w.for_today()
    }

    /// Los vasos se reinician cada día.
    pub fn for_today(self) -> Self {
        let d = today();
        if self.date == d { self } else { Water { date: d, glasses: 0 } }
    }

    pub fn add(&mut self, delta: i32) {
        let me = std::mem::take(self).for_today();
        *self = me;
        self.glasses = (self.glasses as i32 + delta).clamp(0, 99) as u32;
        let _ = std::fs::create_dir_all(crate::config::data_dir());
        let _ = std::fs::write(water_path(), self.serialize_json());
    }
}
