//! Calendario de solo lectura desde una URL privada iCal (Google Calendar → "Dirección secreta en
//! formato iCal"). Se baja cada una hora y se muestran los próximos eventos.
//! Repeticiones soportadas: diarias, semanales (con BYDAY), mensuales y anuales.

use crate::http;

#[derive(Clone, Debug)]
pub struct Event {
    /// Inicio en hora local, en segundos desde 1970 "como si fuera UTC" (sin zona horaria).
    pub start: i64,
    pub all_day: bool,
    pub title: String,
}

/// Baja y parsea el calendario. Err con un mensaje corto si falla.
pub fn fetch(url: &str) -> Result<Vec<Event>, String> {
    let rest = url.trim().strip_prefix("https://").ok_or("La URL tiene que empezar con https://")?;
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).ok_or("URL inválida")?;
    let r = http::get(host, &path).ok_or("Sin conexión con el calendario")?;
    if r.status != 200 {
        return Err(format!("El calendario respondió {}", r.status));
    }
    Ok(parse(&r.body, local_offset_secs()))
}

/// Diferencia entre la hora local y UTC, en segundos.
pub fn local_offset_secs() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    let r = unsafe { GetTimeZoneInformation(&mut tz) };
    let bias = tz.Bias + if r == 2 { tz.DaylightBias } else { tz.StandardBias };
    -(bias as i64) * 60
}

/// "Ahora" en hora local, en el mismo formato que `Event::start`.
pub fn now_local() -> i64 {
    let utc = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    utc + local_offset_secs()
}

/// Próximos `n` eventos desde `now` (incluye los de hoy que ya empezaron hace menos de 1 hora).
pub fn upcoming(events: &[Event], now: i64, n: usize) -> Vec<Event> {
    let mut v: Vec<Event> = events
        .iter()
        .filter(|e| if e.all_day { e.start + 86_400 > now } else { e.start + 3600 > now })
        .cloned()
        .collect();
    v.sort_by_key(|e| e.start);
    v.truncate(n);
    v
}

pub fn parse(ics: &str, offset: i64) -> Vec<Event> {
    // Desplegar líneas continuadas (empiezan con espacio o tab).
    let mut lines: Vec<String> = Vec::new();
    for raw in ics.lines() {
        if let Some(cont) = raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')) {
            if let Some(last) = lines.last_mut() {
                last.push_str(cont);
                continue;
            }
        }
        lines.push(raw.trim_end_matches('\r').to_string());
    }
    let now = now_local();
    let horizon = now + 60 * 86_400;
    let mut out = Vec::new();
    let mut ev: Option<(Option<(i64, bool)>, String, String)> = None;
    for l in &lines {
        if l == "BEGIN:VEVENT" {
            ev = Some((None, String::new(), String::new()));
            continue;
        }
        if l == "END:VEVENT" {
            if let Some((Some((start, all_day)), title, rrule)) = ev.take() {
                let title = if title.is_empty() { "(sin título)".to_string() } else { title };
                for s in occurrences(start, all_day, &rrule, now - 86_400, horizon) {
                    out.push(Event { start: s, all_day, title: title.clone() });
                }
            }
            ev = None;
            continue;
        }
        let Some(cur) = ev.as_mut() else { continue };
        let Some((key, value)) = l.split_once(':') else { continue };
        let name = key.split(';').next().unwrap_or("");
        match name {
            "DTSTART" => cur.0 = parse_dt(key, value, offset),
            "SUMMARY" => cur.1 = unescape(value),
            "RRULE" => cur.2 = value.to_string(),
            _ => {}
        }
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

/// DTSTART → (segundos locales, todo el día).
fn parse_dt(key: &str, v: &str, offset: i64) -> Option<(i64, bool)> {
    let v = v.trim();
    let num = |s: &str| s.parse::<i64>().ok();
    let (y, m, d) = (num(v.get(0..4)?)?, num(v.get(4..6)?)?, num(v.get(6..8)?)?);
    let day = days_from_civil(y, m, d) * 86_400;
    if key.contains("VALUE=DATE") && !key.contains("DATE-TIME") || v.len() == 8 {
        return Some((day, true));
    }
    let (hh, mm, ss) = (num(v.get(9..11)?)?, num(v.get(11..13)?)?, num(v.get(13..15)?).unwrap_or(0));
    let t = day + hh * 3600 + mm * 60 + ss;
    // "Z" = UTC → pasamos a hora local. Con TZID asumimos que es la zona local del usuario.
    Some((if v.ends_with('Z') { t + offset } else { t }, false))
}

/// Inicios de un evento (con o sin repetición) dentro de [from, to].
fn occurrences(start: i64, all_day: bool, rrule: &str, from: i64, to: i64) -> Vec<i64> {
    let _ = all_day;
    if rrule.is_empty() {
        return if start >= from && start <= to { vec![start] } else { vec![] };
    }
    let mut freq = "";
    let mut interval = 1i64;
    let mut count: Option<i64> = None;
    let mut until: Option<i64> = None;
    let mut byday: Vec<i64> = Vec::new();
    for part in rrule.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        match k {
            "FREQ" => freq = v,
            "INTERVAL" => interval = v.parse().unwrap_or(1).max(1),
            "COUNT" => count = v.parse().ok(),
            "UNTIL" => until = parse_dt("UNTIL", v, 0).map(|x| x.0 + 86_399),
            "BYDAY" => {
                byday = v
                    .split(',')
                    .filter_map(|d| {
                        let d = d.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit());
                        ["SU", "MO", "TU", "WE", "TH", "FR", "SA"].iter().position(|x| *x == d).map(|i| i as i64)
                    })
                    .collect()
            }
            _ => {}
        }
    }
    let end = until.unwrap_or(i64::MAX).min(to);
    let tod = start.rem_euclid(86_400);
    let day0 = start.div_euclid(86_400);
    let mut out = Vec::new();
    let mut n = 0i64;
    let push = |t: i64, out: &mut Vec<i64>, n: &mut i64| -> bool {
        if t > end || count.map(|c| *n >= c).unwrap_or(false) {
            return false;
        }
        *n += 1;
        if t >= from {
            out.push(t);
        }
        true
    };
    match freq {
        "DAILY" => {
            let mut d = day0;
            while push(d * 86_400 + tod, &mut out, &mut n) {
                d += interval;
            }
        }
        "WEEKLY" => {
            let wd0 = (day0 + 4).rem_euclid(7); // 1970-01-01 fue jueves
            let days = if byday.is_empty() { vec![wd0] } else { byday.clone() };
            let week0 = day0 - wd0;
            let mut w = 0i64;
            'outer: loop {
                let mut ds: Vec<i64> = days.iter().map(|wd| week0 + w * 7 + wd).filter(|d| *d >= day0).collect();
                ds.sort();
                for d in ds {
                    if !push(d * 86_400 + tod, &mut out, &mut n) {
                        break 'outer;
                    }
                }
                w += interval;
                if (week0 + w * 7) * 86_400 > end {
                    break;
                }
            }
        }
        "MONTHLY" | "YEARLY" => {
            let (y0, m0, d0) = civil_from_days(day0);
            let step = if freq == "YEARLY" { 12 * interval } else { interval };
            let mut k = 0i64;
            loop {
                let mi = m0 - 1 + k * step;
                let (y, m) = (y0 + mi.div_euclid(12), mi.rem_euclid(12) + 1);
                let t = days_from_civil(y, m, d0) * 86_400 + tod;
                if !push(t, &mut out, &mut n) {
                    break;
                }
                k += 1;
                if k > 2000 {
                    break;
                }
            }
        }
        _ => {
            if start >= from && start <= to {
                out.push(start);
            }
        }
    }
    out
}

/// Días desde 1970-01-01 (algoritmo de Howard Hinnant).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// "Hoy 15:00", "Mañana", "Jue 27 · 10:30"
pub fn when(e: &Event, now: i64) -> String {
    const DIAS: [&str; 7] = ["Dom", "Lun", "Mar", "Mié", "Jue", "Vie", "Sáb"];
    let d = e.start.div_euclid(86_400);
    let today = now.div_euclid(86_400);
    let hm = format!("{:02}:{:02}", e.start.rem_euclid(86_400) / 3600, e.start.rem_euclid(3600) / 60);
    let day = match d - today {
        0 => "Hoy".to_string(),
        1 => "Mañana".to_string(),
        _ => {
            let (_, _, dd) = civil_from_days(d);
            format!("{} {}", DIAS[((d + 4).rem_euclid(7)) as usize], dd)
        }
    };
    if e.all_day { day } else { format!("{day} {hm}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fechas() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(days_from_civil(2026, 11, 23)), (2026, 11, 23));
    }

    #[test]
    fn eventos_y_repeticiones() {
        // Un cumpleaños anual que cae mañana, y un evento con el título partido en dos líneas.
        let (_, m, d) = civil_from_days(now_local().div_euclid(86_400) + 1);
        let (y, mo, dd) = civil_from_days(now_local().div_euclid(86_400) + 2);
        let ics = format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:{y:04}{mo:02}{dd:02}T130000Z\r\nSUMMARY:Reuni\r\n ón\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:2020{m:02}{d:02}\r\nRRULE:FREQ=YEARLY\r\nSUMMARY:Cumple\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        let ev = parse(&ics, -3 * 3600);
        assert!(ev.iter().any(|e| e.title == "Cumple" && e.all_day), "{ev:?}");
        assert!(ev.iter().any(|e| e.title == "Reunión"), "{ev:?}");
        let occ = occurrences(days_from_civil(2026, 1, 5) * 86_400, false, "FREQ=WEEKLY;BYDAY=MO,WE", days_from_civil(2026, 1, 5) * 86_400, days_from_civil(2026, 1, 12) * 86_400 + 1);
        assert_eq!(occ.len(), 3); // lun 5, mié 7, lun 12
    }
}
