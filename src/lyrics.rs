//! Letras sincronizadas desde lrclib.net, con caché local en %APPDATA%\Noosa\letras.

use crate::http;
use nanoserde::DeJson;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Lyrics {
    Loading,
    Synced(Vec<(f64, String)>),
    Plain(Vec<String>),
    NotFound,
    Error,
}

#[derive(DeJson, Debug)]
struct ApiTrack {
    #[nserde(default)]
    duration: Option<f64>,
    #[nserde(rename = "syncedLyrics", default)]
    synced: Option<String>,
    #[nserde(rename = "plainLyrics", default)]
    plain: Option<String>,
    #[nserde(default)]
    instrumental: Option<bool>,
}

/// Busca la letra (bloqueante: llamar desde un hilo de fondo).
pub fn fetch(title: &str, artist: &str, album: &str, duration: f64) -> Lyrics {
    let (artist, title) = clean(title, artist);
    if title.is_empty() {
        return Lyrics::NotFound;
    }
    let cache = cache_path(&artist, &title);
    if let Ok(s) = std::fs::read_to_string(&cache) {
        return parse_stored(&s);
    }

    let mut best: Option<ApiTrack> = None;
    let mut net_ok = false;

    // 1) Coincidencia exacta (artista + título + duración).
    let mut path = format!("/api/get?artist_name={}&track_name={}", http::encode(&artist), http::encode(&title));
    if !album.is_empty() {
        path += &format!("&album_name={}", http::encode(album));
    }
    if duration > 0.0 {
        path += &format!("&duration={}", duration.round() as i64);
    }
    if let Some(r) = http::get("lrclib.net", &path) {
        net_ok = true;
        if r.status == 200 {
            best = ApiTrack::deserialize_json(&r.body).ok();
        }
    }

    // 2) Búsqueda aproximada: preferimos letra sincronizada y duración parecida.
    if best.as_ref().map(|b| b.synced.is_none()).unwrap_or(true) {
        let path = format!("/api/search?artist_name={}&track_name={}", http::encode(&artist), http::encode(&title));
        if let Some(r) = http::get("lrclib.net", &path) {
            net_ok = true;
            if r.status == 200 {
                if let Ok(list) = Vec::<ApiTrack>::deserialize_json(&r.body) {
                    let score = |t: &ApiTrack| {
                        let d = match (t.duration, duration > 0.0) {
                            (Some(td), true) => (td - duration).abs(),
                            _ => 30.0,
                        };
                        (if t.synced.is_some() { 0.0 } else { 1000.0 }) + d
                    };
                    if let Some(c) = list.into_iter().min_by(|a, b| score(a).total_cmp(&score(b))) {
                        let better = best.as_ref().map(|b| b.synced.is_none() && c.synced.is_some()).unwrap_or(true);
                        let close = match (c.duration, duration > 0.0) {
                            (Some(td), true) => (td - duration).abs() < 8.0,
                            _ => true,
                        };
                        if better && close {
                            best = Some(c);
                        }
                    }
                }
            }
        }
    }

    if !net_ok {
        return Lyrics::Error;
    }
    let stored = match &best {
        Some(t) if t.synced.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) => {
            format!("synced\n{}", t.synced.as_deref().unwrap())
        }
        Some(t) if t.plain.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) => {
            format!("plain\n{}", t.plain.as_deref().unwrap())
        }
        Some(t) if t.instrumental == Some(true) => "instrumental\n".to_string(),
        _ => "none\n".to_string(),
    };
    let _ = std::fs::create_dir_all(cache.parent().unwrap());
    let _ = std::fs::write(&cache, &stored);
    parse_stored(&stored)
}

fn parse_stored(s: &str) -> Lyrics {
    let (kind, body) = s.split_once('\n').unwrap_or((s, ""));
    match kind.trim() {
        "synced" => {
            let lines = parse_lrc(body);
            if lines.is_empty() { Lyrics::NotFound } else { Lyrics::Synced(lines) }
        }
        "plain" => Lyrics::Plain(body.lines().map(|l| l.trim().to_string()).collect()),
        "instrumental" => Lyrics::Plain(vec!["♪ Instrumental ♪".into()]),
        _ => Lyrics::NotFound,
    }
}

/// Parsea líneas `[mm:ss.xx] texto` (admite varias marcas de tiempo por línea).
pub fn parse_lrc(s: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for raw in s.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            rest = &stripped[end + 1..];
            if let Some((m, sec)) = tag.split_once(':') {
                if let (Ok(m), Ok(sec)) = (m.trim().parse::<f64>(), sec.trim().parse::<f64>()) {
                    stamps.push(m * 60.0 + sec);
                }
            }
        }
        let text = rest.trim();
        for t in stamps {
            out.push((t, if text.is_empty() { "♪".to_string() } else { text.to_string() }));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Limpia títulos de YouTube y similares: "Artista - Canción (Official Video)".
fn clean(title: &str, artist: &str) -> (String, String) {
    let mut t = strip_brackets(title);
    let mut a = artist.trim().to_string();
    for suffix in [" - Topic", "VEVO", "Vevo", " Official"] {
        if let Some(x) = a.strip_suffix(suffix) {
            a = x.trim().to_string();
        }
    }
    if let Some((left, right)) = t.split_once(" - ").or_else(|| t.split_once(" – ")) {
        let (l, r) = (left.trim().to_string(), right.trim().to_string());
        let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
        let (nl, na) = (norm(&l), norm(&a));
        if na.is_empty() || nl.contains(&na) || na.contains(&nl) {
            a = l;
            t = r;
        }
    }
    let t = t.split(" | ").next().unwrap_or("").trim().to_string();
    (a, t)
}

fn strip_brackets(s: &str) -> String {
    const NOISE: [&str; 12] = [
        "official", "video", "audio", "lyric", "letra", "visualizer", "hd", "4k", "remaster", "live", "en vivo", "mv",
    ];
    let mut out = String::new();
    let mut depth = 0;
    let mut chunk = String::new();
    for c in s.chars() {
        match c {
            '(' | '[' => {
                if depth == 0 {
                    chunk.clear();
                }
                depth += 1;
                if depth > 1 {
                    chunk.push(c);
                }
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let low = chunk.to_lowercase();
                    if !NOISE.iter().any(|n| low.contains(n)) {
                        out.push('(');
                        out.push_str(&chunk);
                        out.push(')');
                    }
                } else {
                    chunk.push(c);
                }
            }
            _ if depth > 0 => chunk.push(c),
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cache_path(artist: &str, title: &str) -> PathBuf {
    let safe: String = format!("{artist} - {title}")
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .take(120)
        .collect();
    crate::config::data_dir().join("letras").join(format!("{safe}.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc() {
        let v = parse_lrc("[00:12.50] Hola\n[01:02.00][01:30.00] Coro\n[ar: x]\n");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], (12.5, "Hola".into()));
        assert_eq!(v[2].0, 90.0);
    }

    #[test]
    fn limpia_titulos() {
        assert_eq!(
            clean("Soda Stereo - De Música Ligera (Official Video)", "SodaStereoVEVO"),
            ("Soda Stereo".into(), "De Música Ligera".into())
        );
        assert_eq!(clean("Persiana Americana", "Soda Stereo"), ("Soda Stereo".into(), "Persiana Americana".into()));
    }

    /// Requiere internet: `cargo test --release -- --ignored`.
    #[test]
    #[ignore]
    fn red_lrclib() {
        let dir = std::env::temp_dir().join("esme-test-letras");
        std::env::set_var("APPDATA", &dir);
        match fetch("De Música Ligera", "Soda Stereo", "", 214.0) {
            Lyrics::Synced(v) => assert!(v.len() > 10, "pocas líneas: {}", v.len()),
            other => panic!("esperaba letra sincronizada, llegó {other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn json_lrclib() {
        let j = r#"{"id":1,"trackName":"X","duration":233.0,"instrumental":false,"plainLyrics":"a","syncedLyrics":null}"#;
        let t = ApiTrack::deserialize_json(j).unwrap();
        assert!(t.synced.is_none());
        assert_eq!(t.plain.as_deref(), Some("a"));
    }
}
