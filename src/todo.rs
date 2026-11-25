//! To-Do rápido en el panel "Hoy" (v1.0 — Bloque 1).
//! Persistencia local en `%APPDATA%\Esme\todo.json` con `nanoserde`.
//! Soporta hora opcional `HH:MM` (al principio o al final, con o sin `@`) para avisar en la píldora
//! cuando vence aprovechando el timer de minuto `T_CLOCK` (cero timers extra en reposo).

use nanoserde::{DeJson, SerJson};
use std::path::PathBuf;

pub const MAX_ITEMS: usize = 30;

#[derive(DeJson, SerJson, Clone, Debug, Default, PartialEq, Eq)]
pub struct TodoItem {
    #[nserde(default)]
    pub text: String,
    #[nserde(default)]
    pub done: bool,
    /// Hora opcional en formato "HH:MM" (24 h) para mostrar aviso en la píldora.
    #[nserde(default)]
    pub due: String,
    /// Fecha local "YYYY-MM-DD" en que ya se disparó el aviso (evita repetirlo el mismo día).
    #[nserde(default)]
    pub alerted_day: String,
}

#[derive(DeJson, SerJson, Default, Debug)]
pub struct TodoList {
    #[nserde(default)]
    pub items: Vec<TodoItem>,
}

fn path() -> PathBuf {
    crate::config::data_dir().join("todo.json")
}

impl TodoList {
    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| TodoList::deserialize_json(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(crate::config::data_dir());
        let _ = std::fs::write(path(), self.serialize_json());
    }

    /// Agrega una tarea desde texto libre (ej. `"18:30 Llamar"`, `"Comprar pan @20:00"`, `"Revisar PR"`).
    /// Devuelve `true` si se agregó.
    pub fn add_quick(&mut self, raw: &str) -> bool {
        let Some((text, due)) = parse_quick_input(raw) else {
            return false;
        };
        self.items.insert(
            0,
            TodoItem {
                text,
                done: false,
                due: due.unwrap_or_default(),
                alerted_day: String::new(),
            },
        );
        self.items.truncate(MAX_ITEMS);
        self.save();
        true
    }

    /// Marca o desmarca una tarea como hecha.
    pub fn toggle(&mut self, idx: usize) -> bool {
        let Some(item) = self.items.get_mut(idx) else {
            return false;
        };
        item.done = !item.done;
        if !item.done {
            item.alerted_day.clear();
        }
        self.save();
        true
    }

    /// Borra una tarea por índice.
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx >= self.items.len() {
            return false;
        }
        self.items.remove(idx);
        self.save();
        true
    }

    /// Evalúa y marca en memoria las tareas pendientes cuya hora `HH:MM` coincide con `now_hm`.
    pub fn take_due_alerts_in_memory(&mut self, today: &str, now_hm: &str) -> Vec<(String, String)> {
        let mut due_now = Vec::new();
        for item in &mut self.items {
            if !item.done && !item.due.is_empty() && item.alerted_day != today && item.due == now_hm {
                item.alerted_day = today.to_string();
                due_now.push((item.due.clone(), item.text.clone()));
            }
        }
        due_now
    }

    /// Devuelve y marca como avisadas (persistiendo en disco) las tareas pendientes vencidas en `now_hm`.
    pub fn take_due_alerts(&mut self, today: &str, now_hm: &str) -> Vec<(String, String)> {
        let due_now = self.take_due_alerts_in_memory(today, now_hm);
        if !due_now.is_empty() {
            self.save();
        }
        due_now
    }
}

/// Parsea un token tipo `HH:MM`, `H:MM`, `@HH:MM` o `@H:MM` y lo normaliza a `"HH:MM"`.
pub fn parse_hhmm(token: &str) -> Option<String> {
    let t = token.trim().strip_prefix('@').unwrap_or(token.trim());
    let (h_str, m_str) = t.split_once(':')?;
    if h_str.is_empty() || h_str.len() > 2 || m_str.len() != 2 {
        return None;
    }
    if !h_str.bytes().all(|b| b.is_ascii_digit()) || !m_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: u32 = h_str.parse().ok()?;
    let m: u32 = m_str.parse().ok()?;
    if h <= 23 && m <= 59 {
        Some(format!("{h:02}:{m:02}"))
    } else {
        None
    }
}

/// Extrae `(texto_limpio, Some("HH:MM") | None)` de una entrada rápida de tarea.
pub fn parse_quick_input(raw: &str) -> Option<(String, Option<String>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() >= 2 {
        if let Some(hm) = parse_hhmm(parts[0]) {
            let text = parts[1..].join(" ");
            if !text.is_empty() {
                return Some((text, Some(hm)));
            }
        }
        if let Some(hm) = parse_hhmm(parts[parts.len() - 1]) {
            let text = parts[..parts.len() - 1].join(" ");
            if !text.is_empty() {
                return Some((text, Some(hm)));
            }
        }
    }
    Some((trimmed.to_string(), None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_hora_al_inicio_y_al_final() {
        assert_eq!(parse_quick_input("9:05 Llamar al médico"), Some(("Llamar al médico".into(), Some("09:05".into()))));
        assert_eq!(parse_quick_input("Reunión de equipo 18:30"), Some(("Reunión de equipo".into(), Some("18:30".into()))));
        assert_eq!(parse_quick_input("Sacar la basura @21:00"), Some(("Sacar la basura".into(), Some("21:00".into()))));
        assert_eq!(parse_quick_input("Comprar pan"), Some(("Comprar pan".into(), None)));
        assert_eq!(parse_quick_input("   "), None);
    }

    #[test]
    fn rechaza_horas_invalidas() {
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("12:5"), None);
        assert_eq!(parse_quick_input("25:00 Ver serie"), Some(("25:00 Ver serie".into(), None)));
    }

    #[test]
    fn dispara_aviso_una_sola_vez_por_dia() {
        let mut list = TodoList {
            items: vec![
                TodoItem { text: "Llamar".into(), done: false, due: "14:30".into(), alerted_day: String::new() },
                TodoItem { text: "Ya hecha".into(), done: true, due: "14:30".into(), alerted_day: String::new() },
            ],
        };
        let first = list.take_due_alerts_in_memory("2026-11-24", "14:30");
        assert_eq!(first, vec![("14:30".into(), "Llamar".into())]);
        let second = list.take_due_alerts_in_memory("2026-11-24", "14:30");
        assert!(second.is_empty());
    }
}
