//! Pomodoro y registro de agua (Fase 5). Todo local: %APPDATA%\Esme\agua.json.

use nanoserde::{DeJson, SerJson};
use std::time::{Duration, Instant};


#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Focus,
    Break,
}

/// Temporizador Pomodoro: foco y descanso configurables (por defecto 25 / 5 min).
pub struct Pomodoro {
    pub mode: Mode,
    focus: Duration,
    brk: Duration,
    /// Cuándo termina si está corriendo.
    end_at: Option<Instant>,
    /// Lo que queda si está en pausa.
    left: Duration,
}

impl Pomodoro {
    pub fn new(focus_min: u32, break_min: u32) -> Self {
        let focus = Duration::from_secs(focus_min as u64 * 60);
        Self { mode: Mode::Focus, focus, brk: Duration::from_secs(break_min as u64 * 60), end_at: None, left: focus }
    }

    /// Cambia las duraciones. Si no se estaba usando, arranca de cero con la nueva.
    pub fn set_durations(&mut self, focus_min: u32, break_min: u32) {
        let untouched = !self.touched();
        self.focus = Duration::from_secs(focus_min as u64 * 60);
        self.brk = Duration::from_secs(break_min as u64 * 60);
        if untouched {
            self.left = self.total();
        }
    }

    pub fn minutes(&self, m: Mode) -> u64 {
        (if m == Mode::Focus { self.focus } else { self.brk }).as_secs() / 60
    }

    pub fn running(&self) -> bool {
        self.end_at.is_some()
    }

    /// ¿Se usó desde que se abrió? (para mostrarlo en la píldora aunque esté en pausa).
    pub fn touched(&self) -> bool {
        self.running() || self.left != self.total()
    }

    pub fn total(&self) -> Duration {
        if self.mode == Mode::Focus { self.focus } else { self.brk }
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
        self.left = self.focus;
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
    /// Vasos registrados por hora del día (0..23). Compatible con archivos viejos gracias a `default`.
    #[nserde(default)]
    pub hours: Vec<u32>,
}

fn water_path() -> std::path::PathBuf {
    crate::config::data_dir().join("agua.json")
}

/// "2026-11-23" (fecha local).
pub fn today() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

/// Hora local actual (0..23).
pub fn current_hour() -> usize {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (t.wHour as usize).min(23)
}

impl Water {
    pub fn load() -> Self {
        let w: Water = crate::config::read_text(&water_path())
            .and_then(|s| Water::deserialize_json(&s).ok())
            .unwrap_or_default();
        w.for_today()
    }

    /// Los vasos se reinician cada día.
    pub fn for_today(mut self) -> Self {
        let d = today();
        if self.date == d {
            self.hours.resize(24, 0);
            self
        } else {
            Water { date: d, glasses: 0, hours: vec![0; 24] }
        }
    }

    /// Devuelve un arreglo fijo de 24 horas (0..23) para dibujar las barritas.
    pub fn hourly(&self) -> [u32; 24] {
        let mut out = [0u32; 24];
        for (dst, src) in out.iter_mut().zip(self.hours.iter()) {
            *dst = *src;
        }
        out
    }

    pub fn add(&mut self, delta: i32) {
        let me = std::mem::take(self).for_today();
        *self = me;
        let prev = self.glasses;
        self.glasses = (self.glasses as i32 + delta).clamp(0, 99) as u32;
        let h = current_hour();
        if delta > 0 {
            self.hours[h] = self.hours[h].saturating_add(delta as u32);
        } else if delta < 0 && self.glasses < prev {
            let mut rem = prev - self.glasses;
            for step in 0..24 {
                if rem == 0 {
                    break;
                }
                let idx = (h + 24 - step) % 24;
                if self.hours[idx] > 0 {
                    let take = self.hours[idx].min(rem);
                    self.hours[idx] -= take;
                    rem -= take;
                }
            }
        }
        let _ = std::fs::create_dir_all(crate::config::data_dir());
        let _ = std::fs::write(water_path(), self.serialize_json());
    }
}
