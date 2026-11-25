//! Buscar actualizaciones (v1.0): consulta el último release en GitHub solo cuando tocás el botón.
//! No descarga ni reemplaza nada: si hay una versión nueva, abre la página para bajarla.

use nanoserde::DeJson;

const HOST: &str = "api.github.com";
const PATH: &str = "/repos/Maurito2004/Esme/releases/latest";
pub const RELEASES_URL: &str = "https://github.com/Maurito2004/Esme/releases/latest";

#[derive(DeJson)]
struct Release {
    #[nserde(default)]
    tag_name: String,
    #[nserde(default)]
    html_url: String,
}

pub enum Check {
    /// Ya tenés la última versión.
    UpToDate,
    /// Hay una nueva: (versión, página del release).
    Newer(String, String),
}

/// Pregunta a GitHub cuál es el último release. Llamar desde un hilo aparte.
pub fn check() -> Result<Check, String> {
    let r = crate::http::get_with_headers(HOST, PATH, "Accept: application/vnd.github+json\r\n")
        .ok_or_else(|| "Sin conexión con GitHub".to_string())?;
    match r.status {
        200 => {}
        403 | 429 => return Err("GitHub está limitando consultas: probá en un rato".into()),
        404 => return Err("Todavía no hay versiones publicadas".into()),
        s => return Err(format!("GitHub respondió {s}")),
    }
    let rel = Release::deserialize_json(&r.body).map_err(|_| "No entendí la respuesta de GitHub".to_string())?;
    let tag = rel.tag_name.trim().to_string();
    if tag.is_empty() {
        return Err("No entendí la respuesta de GitHub".into());
    }
    let url = if rel.html_url.starts_with("https://github.com/") { rel.html_url } else { RELEASES_URL.to_string() };
    if is_newer(&tag, env!("CARGO_PKG_VERSION")) {
        Ok(Check::Newer(tag, url))
    } else {
        Ok(Check::UpToDate)
    }
}

fn parts(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-']).take(3).map(|p| p.parse().unwrap_or(0)).collect()
}

/// ¿`tag` (ej. "v1.0.1") es más nuevo que `current` (ej. "1.0.0")?
pub fn is_newer(tag: &str, current: &str) -> bool {
    let (mut a, mut b) = (parts(tag), parts(current));
    a.resize(3, 0);
    b.resize(3, 0);
    a > b
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compara_versiones() {
        assert!(is_newer("v1.0.1", "1.0.0"));
        assert!(is_newer("v1.1", "1.0.9"));
        assert!(is_newer("v2.0.0", "1.9.9"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("v0.9.1", "1.0.0"));
    }
}
