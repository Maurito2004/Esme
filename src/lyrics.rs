//! Letras sincronizadas desde lrclib.net, con caché local en %APPDATA%\Esme\letras.
//!
//! Búsqueda: junta candidatos de varias consultas (exacta, por artista+título y por texto libre)
//! y solo acepta uno si el título se parece de verdad y la duración coincide. Así no aparecen
//! letras de otra canción en un video cualquiera.

use crate::http;
use nanoserde::DeJson;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Lyrics {
    Loading,
    Synced(Vec<(f64, String)>),
    Plain(Vec<String>),
    NotFound,
    /// Video muy largo (mezcla, radio, stream): no se busca letra.
    Mix,
    Error,
}

#[derive(DeJson, Debug)]
struct ApiTrack {
    #[nserde(rename = "trackName", default)]
    track: Option<String>,
    #[nserde(rename = "artistName", default)]
    artist: Option<String>,
    #[nserde(default)]
    duration: Option<f64>,
    #[nserde(rename = "syncedLyrics", default)]
    synced: Option<String>,
    #[nserde(rename = "plainLyrics", default)]
    plain: Option<String>,
    #[nserde(default)]
    instrumental: Option<bool>,
}

/// Más de esto, es una mezcla o un stream: no tiene una letra.
const MAX_SONG_SECS: f64 = 15.0 * 60.0;
/// Un "no encontré" se vuelve a intentar pasados unos días (lrclib suma letras todo el tiempo).
const NONE_TTL_SECS: u64 = 3 * 24 * 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Palabras normalizadas (minúsculas, sin acentos ni signos) para comparar títulos.
fn words(s: &str) -> Vec<String> {
    let fold = |c: char| match c {
        'á' | 'à' | 'ä' | 'â' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        c => c,
    };
    s.to_lowercase()
        .chars()
        .map(fold)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Parecido entre dos textos: proporción de palabras de `a` que están en `b` (0..1).
fn similarity(a: &str, b: &str) -> f64 {
    let wa = words(a);
    let wb = words(b);
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let hits = wa.iter().filter(|w| wb.contains(w)).count();
    let r1 = hits as f64 / wa.len() as f64;
    let hits2 = wb.iter().filter(|w| wa.contains(w)).count();
    let r2 = hits2 as f64 / wb.len() as f64;
    r1.max(r2) * 0.7 + r1.min(r2) * 0.3
}

fn get_json<T: DeJson>(path: &str, net_ok: &mut bool) -> Option<T> {
    let r = http::get("lrclib.net", path)?;
    *net_ok = true;
    if r.status != 200 {
        return None;
    }
    T::deserialize_json(&r.body).ok()
}

fn has_text(t: &ApiTrack) -> bool {
    t.synced.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
        || t.plain.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
        || t.instrumental == Some(true)
}

/// Busca la letra (bloqueante: llamar desde un hilo de fondo).
pub fn fetch(title: &str, artist: &str, album: &str, duration: f64) -> Lyrics {
    if duration > MAX_SONG_SECS {
        return Lyrics::Mix;
    }
    let (artist, title) = clean(title, artist);
    if title.is_empty() {
        return Lyrics::NotFound;
    }
    let cache = cache_path(&artist, &title);
    if let Ok(s) = std::fs::read_to_string(&cache) {
        let expired_none = s.starts_with("none")
            && s.lines().nth(1).and_then(|t| t.trim().parse::<u64>().ok()).map(|t| now_secs() > t + NONE_TTL_SECS).unwrap_or(true);
        if !expired_none {
            return parse_stored(&s);
        }
    }

    let mut net_ok = false;
    let mut candidates: Vec<ApiTrack> = Vec::new();

    // 1) Coincidencia exacta (artista + título + duración).
    if !artist.is_empty() {
        let mut path = format!("/api/get?artist_name={}&track_name={}", http::encode(&artist), http::encode(&title));
        if !album.is_empty() {
            path += &format!("&album_name={}", http::encode(album));
        }
        if duration > 0.0 {
            path += &format!("&duration={}", duration.round() as i64);
        }
        if let Some(t) = get_json::<ApiTrack>(&path, &mut net_ok) {
            candidates.push(t);
        }
    }
    // 2) Búsquedas aproximadas: por campos y por texto libre (sirve cuando el "artista" es el
    //    nombre del canal de YouTube y no el del músico).
    let mut queries = Vec::new();
    if !artist.is_empty() {
        queries.push(format!("/api/search?artist_name={}&track_name={}", http::encode(&artist), http::encode(&title)));
        queries.push(format!("/api/search?q={}", http::encode(&format!("{artist} {title}"))));
    }
    queries.push(format!("/api/search?q={}", http::encode(&title)));
    for q in queries {
        if candidates.iter().any(|c| c.synced.is_some() && score(c, &artist, &title, duration).is_some()) {
            break; // ya hay una sincronizada que coincide
        }
        if let Some(list) = get_json::<Vec<ApiTrack>>(&q, &mut net_ok) {
            candidates.extend(list.into_iter().take(15));
        }
    }

    if !net_ok {
        return Lyrics::Error;
    }
    let best = candidates
        .into_iter()
        .filter(has_text)
        .filter_map(|c| score(&c, &artist, &title, duration).map(|s| (s, c)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c);

    let stored = match &best {
        Some(t) if t.synced.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) => {
            format!("synced\n{}", t.synced.as_deref().unwrap())
        }
        Some(t) if t.plain.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) => {
            format!("plain\n{}", t.plain.as_deref().unwrap())
        }
        Some(t) if t.instrumental == Some(true) => "instrumental\n".to_string(),
        _ => format!("none\n{}", now_secs()),
    };
    let _ = std::fs::create_dir_all(cache.parent().unwrap());
    let _ = std::fs::write(&cache, &stored);
    parse_stored(&stored)
}

/// Puntaje de un candidato, o None si no es la misma canción.
fn score(c: &ApiTrack, artist: &str, title: &str, duration: f64) -> Option<f64> {
    let track = c.track.as_deref().unwrap_or("");
    let t_sim = similarity(title, track).max(similarity(&format!("{artist} {title}"), track));
    if t_sim < 0.6 {
        return None;
    }
    let d = match (c.duration, duration > 0.0) {
        (Some(cd), true) => {
            let d = (cd - duration).abs();
            if d > 7.0 {
                return None; // otra versión / otra canción
            }
            d
        }
        // Sin duración del reproductor: exigimos que el título coincida casi exacto.
        _ if t_sim < 0.9 => return None,
        _ => 3.0,
    };
    let a_sim = if artist.is_empty() { 0.5 } else { similarity(artist, c.artist.as_deref().unwrap_or("")) };
    let synced = if c.synced.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) { 1.0 } else { 0.0 };
    Some(t_sim * 3.0 + a_sim * 1.5 + synced * 2.0 - d * 0.15)
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
    fn parecido_de_titulos() {
        assert!(similarity("De Música Ligera", "De musica ligera") > 0.95);
        assert!(similarity("Persiana Americana", "Un video cualquiera de Twitter") < 0.3);
    }

    #[test]
    fn rechaza_otra_cancion() {
        let c = ApiTrack {
            track: Some("Otra Cancion".into()),
            artist: Some("Alguien".into()),
            duration: Some(200.0),
            synced: Some("[00:01.00] hola".into()),
            plain: None,
            instrumental: None,
        };
        assert!(score(&c, "Soda Stereo", "Persiana Americana", 200.0).is_none());
        let ok = ApiTrack { track: Some("Persiana Americana".into()), artist: Some("Soda Stereo".into()), ..c };
        assert!(score(&ok, "Soda Stereo", "Persiana Americana", 202.0).is_some());
    }

    #[test]
    fn json_lrclib() {
        let j = r#"{"id":1,"trackName":"X","duration":233.0,"instrumental":false,"plainLyrics":"a","syncedLyrics":null}"#;
        let t = ApiTrack::deserialize_json(j).unwrap();
        assert!(t.synced.is_none());
        assert_eq!(t.plain.as_deref(), Some("a"));
    }
}
