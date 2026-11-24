//! Calculadora local en microsegundos, conversor de divisas con caché diario (%APPDATA%\Esme\tasas.json)
//! y parser de temporizador rápido (`timer 10m`, `timer 45s`). Cero dependencias externas.

use crate::{config, http};
use nanoserde::{DeJson, SerJson};
use std::collections::HashMap;

/// Resultado de evaluar el texto del input en tiempo real o al presionar Enter.
#[derive(Clone, Debug, PartialEq)]
pub enum InlineAction {
    /// Resultado inmediato al terminar en `=` (calculadora o conversión de moneda).
    InstantResult(String),
    /// Comando de temporizador rápido: duración en segundos y etiqueta legible (ej. "10m").
    QuickTimer(u64, String),
}

#[derive(DeJson, SerJson, Clone, Debug, Default)]
pub struct RatesCache {
    /// Fecha local "YYYY-MM-DD" de la última actualización.
    #[nserde(default)]
    pub date: String,
    /// Tasas relativas a 1 USD (ej. "ARS" -> 1050.0, "EUR" -> 0.92).
    #[nserde(default)]
    pub rates: HashMap<String, f64>,
}

/// Respuesta de dolarapi.com/v1/dolares/{blue,bolsa} (gratis, sin API key).
#[derive(DeJson)]
struct DolarApiResponse {
    #[nserde(default)]
    venta: f64,
}

/// Pide la cotización de venta de dolarapi.com (pesos por 1 USD).
fn fetch_dolar(casa: &str) -> Option<f64> {
    let resp = http::get("dolarapi.com", &format!("/v1/dolares/{casa}"))?;
    if resp.status != 200 {
        return None;
    }
    let v = DolarApiResponse::deserialize_json(&resp.body).ok()?.venta;
    (v > 0.0).then_some(v)
}

/// Respuesta de open.er-api.com/v6/latest/USD (gratis, sin API key).
#[derive(DeJson)]
struct ErApiResponse {
    #[nserde(default)]
    rates: HashMap<String, f64>,
}

fn rates_path() -> std::path::PathBuf {
    config::data_dir().join("tasas.json")
}

/// Tasas de respaldo offline relativas a 1 USD.
fn fallback_rates() -> HashMap<String, f64> {
    let mut m = HashMap::new();
    for (k, v) in [
        ("USD", 1.0),
        ("EUR", 0.92),
        ("ARS", 1575.0),
        // Dólar blue y MEP: pesos por 1 USD (respaldo offline aproximado).
        ("ARS_BLUE", 1675.0),
        ("ARS_MEP", 1740.0),
        ("BRL", 5.75),
        ("CLP", 970.0),
        ("UYU", 42.5),
        ("MXN", 20.1),
        ("COP", 4350.0),
        ("PEN", 3.76),
        ("GBP", 0.79),
        ("JPY", 154.0),
        ("CAD", 1.39),
        ("CHF", 0.88),
    ] {
        m.insert(k.to_string(), v);
    }
    m
}

impl RatesCache {
    pub fn load() -> Self {
        let mut cache: RatesCache = std::fs::read_to_string(rates_path())
            .ok()
            .and_then(|s| RatesCache::deserialize_json(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default();
        if cache.rates.is_empty() {
            cache.rates = fallback_rates();
        }
        cache
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(config::data_dir());
        let _ = std::fs::write(rates_path(), self.serialize_json());
    }

    /// ¿Ya se actualizó hoy?
    pub fn is_fresh(&self) -> bool {
        !self.date.is_empty() && self.date == crate::productivity::today()
    }
}

/// Actualiza las tasas desde un hilo de fondo como máximo 1 vez al día.
pub fn refresh_rates_if_needed() -> Option<RatesCache> {
    let cache = RatesCache::load();
    if cache.is_fresh() {
        return Some(cache);
    }
    let resp = http::get("open.er-api.com", "/v6/latest/USD")?;
    if resp.status != 200 {
        return None;
    }
    let parsed = ErApiResponse::deserialize_json(&resp.body).ok()?;
    if parsed.rates.is_empty() {
        return None;
    }
    let mut rates = parsed.rates;
    if let Some(v) = fetch_dolar("blue") {
        rates.insert("ARS_BLUE".into(), v);
    }
    if let Some(v) = fetch_dolar("bolsa") {
        rates.insert("ARS_MEP".into(), v);
    }
    let updated = RatesCache {
        date: crate::productivity::today(),
        rates,
    };
    updated.save();
    Some(updated)
}

/// Evalúa si el input actual es un cálculo/conversión terminado en `=` o un comando `timer ...`.
pub fn evaluate_input(input: &str, rates: &RatesCache) -> Option<InlineAction> {
    let trimmed = input.trim();
    if let Some(t) = parse_timer_cmd(trimmed) {
        return Some(InlineAction::QuickTimer(t.0, t.1));
    }
    if let Some(expr) = trimmed.strip_suffix('=') {
        let expr = expr.trim();
        if expr.is_empty() {
            return None;
        }
        if let Some(conv) = try_currency(expr, rates) {
            return Some(InlineAction::InstantResult(conv));
        }
        if let Some(val) = eval_math(expr) {
            return Some(InlineAction::InstantResult(format_number(val)));
        }
    }
    None
}

/// Reconoce `timer 10m`, `timer 45s`, `timer 1h 30m`, `temporizador 5m`.
fn parse_timer_cmd(s: &str) -> Option<(u64, String)> {
    let lower = s.to_lowercase();
    let rest = lower
        .strip_prefix("timer ")
        .or_else(|| lower.strip_prefix("temporizador "))?
        .trim();
    if rest.is_empty() {
        return None;
    }
    let mut total_secs = 0u64;
    let mut num_buf = String::new();
    for c in rest.chars() {
        if c.is_ascii_digit() {
            num_buf.push(c);
        } else if !c.is_whitespace() {
            let n: u64 = num_buf.parse().ok()?;
            num_buf.clear();
            match c {
                'h' => total_secs = total_secs.saturating_add(n * 3600),
                'm' => total_secs = total_secs.saturating_add(n * 60),
                's' => total_secs = total_secs.saturating_add(n),
                _ => return None,
            }
        }
    }
    if !num_buf.is_empty() {
        // Sin sufijo explícito: asumimos minutos.
        let n: u64 = num_buf.parse().ok()?;
        total_secs = total_secs.saturating_add(n * 60);
    }
    if total_secs == 0 || total_secs > 24 * 3600 {
        return None;
    }
    let label = if total_secs >= 3600 && total_secs % 60 == 0 {
        format!("{}h {}m", total_secs / 3600, (total_secs % 3600) / 60)
    } else if total_secs >= 60 && total_secs % 60 == 0 {
        format!("{} min", total_secs / 60)
    } else {
        format!("{} s", total_secs)
    };
    Some((total_secs, label))
}

/// Reconoce `100 usd to ars`, `50 eur a usd`, `25000 ars en usd`, `100.5 brl -> usd`.
fn try_currency(expr: &str, cache: &RatesCache) -> Option<String> {
    let norm = expr.replace(',', ".");
    let parts: Vec<&str> = norm.split_whitespace().collect();
    let (amt_s, from_s, to_s) = match parts.as_slice() {
        [amt, from, sep, to] if matches!(sep.to_lowercase().as_str(), "to" | "a" | "en" | "->" | "in") => {
            (*amt, *from, *to)
        }
        [amt, from, to] => (*amt, *from, *to),
        _ => return None,
    };
    let amount: f64 = amt_s.parse().ok()?;
    let from = normalize_currency(from_s)?;
    let to = normalize_currency(to_s)?;
    let r_from = cache.rates.get(from).copied().or_else(|| fallback_rates().get(from).copied())?;
    let r_to = cache.rates.get(to).copied().or_else(|| fallback_rates().get(to).copied())?;
    if r_from <= 0.0 {
        return None;
    }
    let usd = amount / r_from;
    let out = usd * r_to;
    Some(format!("{} {}", format_currency(out), currency_label(to)))
}

/// Cómo se muestra la moneda en el resultado: los dólares argentinos quedan como pesos.
fn currency_label(code: &str) -> &str {
    match code {
        "ARS_BLUE" => "ARS (blue)",
        "ARS_MEP" => "ARS (MEP)",
        c => c,
    }
}

fn normalize_currency(code: &str) -> Option<&'static str> {
    match code.to_uppercase().as_str() {
        "USD" | "US$" | "DOLAR" | "DOLARES" | "DÓLAR" | "DÓLARES" => Some("USD"),
        "EUR" | "EURO" | "EUROS" | "€" => Some("EUR"),
        "ARS" | "PESO" | "PESOS" | "$AR" | "OFICIAL" => Some("ARS"),
        "BLUE" | "ARSBLUE" => Some("ARS_BLUE"),
        "MEP" | "BOLSA" | "ARSMEP" => Some("ARS_MEP"),
        "BRL" | "REAL" | "REALES" | "R$" => Some("BRL"),
        "CLP" => Some("CLP"),
        "UYU" => Some("UYU"),
        "MXN" => Some("MXN"),
        "COP" => Some("COP"),
        "PEN" => Some("PEN"),
        "GBP" | "LIBRA" | "LIBRAS" | "£" => Some("GBP"),
        "JPY" | "YEN" | "YENES" | "¥" => Some("JPY"),
        "CAD" => Some("CAD"),
        "CHF" => Some("CHF"),
        _ => None,
    }
}

fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return "Error".into();
    }
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn format_currency(v: f64) -> String {
    if v.abs() >= 100.0 {
        format!("{v:.2}")
    } else if v.abs() >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.4}")
    }
}

// ---------- Parser aritmético recursivo descendente (+, -, *, /, %, ^, paréntesis) ----------

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

pub fn eval_math(expr: &str) -> Option<f64> {
    let clean = expr.replace(',', ".");
    let mut p = Parser { bytes: clean.as_bytes(), pos: 0 };
    let val = p.parse_expr()?;
    p.skip_ws();
    if p.pos == p.bytes.len() && val.is_finite() {
        Some(val)
    } else {
        None
    }
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn parse_expr(&mut self) -> Option<f64> {
        let mut left = self.parse_term()?;
        while let Some(op) = self.peek() {
            if op == b'+' || op == b'-' {
                self.pos += 1;
                let right = self.parse_term()?;
                left = if op == b'+' { left + right } else { left - right };
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_term(&mut self) -> Option<f64> {
        let mut left = self.parse_power()?;
        while let Some(op) = self.peek() {
            if op == b'*' || op == b'/' || op == b'%' {
                self.pos += 1;
                let right = self.parse_power()?;
                left = match op {
                    b'*' => left * right,
                    b'/' => {
                        if right == 0.0 {
                            return None;
                        }
                        left / right
                    }
                    _ => {
                        if right == 0.0 {
                            return None;
                        }
                        left % right
                    }
                };
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_power(&mut self) -> Option<f64> {
        let base = self.parse_unary()?;
        if self.peek() == Some(b'^') {
            self.pos += 1;
            let exp = self.parse_power()?;
            Some(base.powf(exp))
        } else {
            Some(base)
        }
    }

    fn parse_unary(&mut self) -> Option<f64> {
        match self.peek()? {
            b'+' => {
                self.pos += 1;
                self.parse_unary()
            }
            b'-' => {
                self.pos += 1;
                self.parse_unary().map(|v| -v)
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Option<f64> {
        self.skip_ws();
        let c = *self.bytes.get(self.pos)?;
        if c == b'(' {
            self.pos += 1;
            let val = self.parse_expr()?;
            if self.peek() != Some(b')') {
                return None;
            }
            self.pos += 1;
            return Some(val);
        }
        let start = self.pos;
        let mut has_dot = false;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_digit() {
                self.pos += 1;
            } else if b == b'.' && !has_dot {
                has_dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return None;
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        s.parse::<f64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resuelve_expresiones_locales() {
        let cache = RatesCache::load();
        assert_eq!(
            evaluate_input("1500 * 3 + (450 / 2) =", &cache),
            Some(InlineAction::InstantResult("4725".into()))
        );
        assert_eq!(
            evaluate_input("2 ^ 10 =", &cache),
            Some(InlineAction::InstantResult("1024".into()))
        );
    }

    #[test]
    fn convierte_monedas_con_fallback() {
        let cache = RatesCache::load();
        let res = evaluate_input("100 usd to ars =", &cache);
        assert!(matches!(res, Some(InlineAction::InstantResult(s)) if s.ends_with("ARS")));
    }

    #[test]
    fn convierte_a_dolar_blue() {
        let cache = RatesCache::load();
        let res = evaluate_input("100 usd a blue =", &cache);
        assert!(matches!(res, Some(InlineAction::InstantResult(s)) if s.ends_with("ARS (blue)")));
        let res = evaluate_input("50000 mep en usd =", &cache);
        assert!(matches!(res, Some(InlineAction::InstantResult(s)) if s.ends_with("USD")));
    }

    #[test]
    fn parsea_timer_rapido() {
        let cache = RatesCache::load();
        assert_eq!(
            evaluate_input("timer 10m", &cache),
            Some(InlineAction::QuickTimer(600, "10 min".into()))
        );
    }
}
