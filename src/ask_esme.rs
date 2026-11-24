//! Ask Esme + Traductor Rápido + Acciones de Portapapeles (Fase 6).
//! Usa WinHTTP en un hilo separado + nanoserde contra la API REST de Gemini.

use crate::http;
use nanoserde::DeJson;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_ASK_DONE: u32 = WM_APP + 10;

const SYSTEM_PROMPT_ASK: &str = "Sos Esme, un asistente integrado en una barra dinámica minimalista de Windows. \
Respondé SIEMPRE en español natural, en texto plano (sin Markdown, sin asteriscos **, sin encabezados #, sin bloques de código) \
y en un máximo estricto de 3 a 4 renglones cortos y directos para que entre limpio en el notch.";

const SYSTEM_PROMPT_TRANSLATE: &str = "Sos el traductor instantáneo de Esme. \
Detectá automáticamente el idioma del texto del usuario: si está en español, traducilo al inglés; \
si está en cualquier otro idioma, traducilo al español natural. \
Devolvé ÚNICAMENTE la traducción limpia en texto plano, sin comillas, sin explicaciones ni Markdown.";

const SYSTEM_PROMPT_SUMMARIZE: &str = "Resumí el siguiente texto del portapapeles en español natural, \
en texto plano sin Markdown ni viñetas largas, en máximo 3 renglones claros y concisos.";

const SYSTEM_PROMPT_SPELLCHECK: &str = "Corregí la ortografía, puntuación y gramática del siguiente texto \
manteniendo su idioma y tono original. Devolvé ÚNICAMENTE el texto corregido en texto plano, sin explicaciones ni comillas.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskMode {
    Ask,
    Translate,
}

impl AskMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Ask => Self::Translate,
            Self::Translate => Self::Ask,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "✦ Preguntar",
            Self::Translate => "⇄ Traducir",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Ask => "Preguntale a Esme, '1500*3=' o 'timer 10m'… (Tab: Traducir)",
            Self::Translate => "Escribí o pegá texto para traducir… (Tab: Preguntar)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptTask {
    Mode(AskMode),
    SummarizeClip,
    SpellcheckClip,
}

/// Estado del panel Ask Esme dentro de `App`.
pub struct AskState {
    pub mode: AskMode,
    pub input: String,
    pub selected_all: bool,
    pub focus: bool,
    pub caret_visible: bool,
    pub loading: bool,
    pub response: String,
    pub copied_feedback: bool,
    pub req_id: u64,
    pub shared: Arc<Mutex<Option<(u64, Result<String, String>)>>>,
}

impl Default for AskState {
    fn default() -> Self {
        Self {
            mode: AskMode::Ask,
            input: String::new(),
            selected_all: false,
            focus: false,
            caret_visible: true,
            loading: false,
            response: String::new(),
            copied_feedback: false,
            req_id: 0,
            shared: Arc::new(Mutex::new(None)),
        }
    }
}

// Estructuras mínimas con nanoserde para parsear la respuesta de Gemini REST v1beta.
#[derive(DeJson)]
struct GeminiResponse {
    #[nserde(default)]
    candidates: Vec<Candidate>,
    #[nserde(default)]
    error: Option<GeminiError>,
}

#[derive(DeJson)]
struct Candidate {
    content: CandidateContent,
}

#[derive(DeJson)]
struct CandidateContent {
    #[nserde(default)]
    parts: Vec<Part>,
}

#[derive(DeJson)]
struct Part {
    #[nserde(default)]
    text: String,
}

#[derive(DeJson)]
struct GeminiError {
    #[nserde(default)]
    message: String,
}

/// Escapa un string para insertarlo de forma segura en un JSON manual sin dependencias extra.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Limpia cualquier resto de Markdown si el modelo devuelve asteriscos o saltos excesivos.
fn sanitize_plain_text(s: &str) -> String {
    let cleaned = s.replace("**", "").replace('`', "");
    let lines: Vec<&str> = cleaned
        .lines()
        .map(|l| l.trim().trim_start_matches("# ").trim_start_matches("## ").trim_start_matches("### "))
        .filter(|l| !l.is_empty())
        .take(4)
        .collect();
    lines.join("\n")
}

/// Modelo nuevo que eligió el hilo de consulta (el anterior se retiró o no había ninguno).
/// La interfaz lo guarda en la configuración al recibir `WM_ASK_DONE`.
static NEW_MODEL: Mutex<Option<String>> = Mutex::new(None);

pub fn take_new_model() -> Option<String> {
    NEW_MODEL.lock().unwrap().take()
}

/// Dispara la consulta a Gemini en un hilo separado y avisa con `WM_ASK_DONE` al terminar.
#[allow(clippy::too_many_arguments)]
pub fn spawn_query(
    hwnd: HWND,
    req_id: u64,
    api_key: String,
    model: String,
    family: String,
    task: PromptTask,
    user_text: String,
    shared: Arc<Mutex<Option<(u64, Result<String, String>)>>>,
) {
    let hwnd_val = hwnd.0 as isize;
    std::thread::spawn(move || {
        let res = query_gemini(&api_key, &model, &family, task, &user_text);
        *shared.lock().unwrap() = Some((req_id, res));
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd_val as *mut _)), WM_ASK_DONE, WPARAM(0), LPARAM(0));
        }
    });
}

const HOST: &str = "generativelanguage.googleapis.com";

fn auth_header(api_key: &str) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Falta la API Key: Ajustes → Ask Esme → Pegar key".into());
    }
    if key.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("La API Key guardada no es válida; pegala de nuevo en Ajustes → Ask Esme".into());
    }
    // La key va en un header y no en la URL, así no queda en logs ni proxies.
    Ok(format!("x-goog-api-key: {key}\r\n"))
}

#[derive(DeJson)]
struct ModelList {
    #[nserde(default)]
    models: Vec<ModelInfo>,
    #[nserde(default, rename = "nextPageToken")]
    next_page_token: String,
    #[nserde(default)]
    error: Option<GeminiError>,
}

#[derive(DeJson)]
struct ModelInfo {
    #[nserde(default)]
    name: String,
    #[nserde(default, rename = "supportedGenerationMethods")]
    methods: Vec<String>,
}

/// Modelos de texto que esta key puede usar (sin el prefijo "models/").
pub fn list_models(api_key: &str) -> Result<Vec<String>, String> {
    auth_header(api_key)?; // valida el formato
    // Las keys nuevas de AI Studio ("AQ.…") no sirven en el header para listar modelos (401),
    // pero sí como parámetro. Va solo a Google y por HTTPS; las consultas siguen usando el header.
    let key_q = http::encode(api_key.trim());
    let mut out = Vec::new();
    let mut token = String::new();
    for _ in 0..4 {
        let mut path = format!("/v1beta/models?pageSize=200&key={key_q}");
        if !token.is_empty() {
            path += &format!("&pageToken={}", http::encode(&token));
        }
        let resp = http::get(HOST, &path).ok_or("Sin conexión con Google")?;
        let list = ModelList::deserialize_json(&resp.body).map_err(|_| format!("Respuesta inválida de Google (HTTP {})", resp.status))?;
        if resp.status != 200 {
            let google = list.error.map(|e| e.message).filter(|m| !m.is_empty());
            return Err(match (resp.status, google) {
                (_, Some(m)) if m.to_lowercase().contains("api key not valid") => "La API Key no es válida: copiala de nuevo desde Google AI Studio".into(),
                (_, Some(m)) => m,
                (s, None) => format!("Google rechazó la consulta (HTTP {s})"),
            });
        }
        out.extend(
            list.models
                .into_iter()
                .filter(|m| m.methods.iter().any(|x| x == "generateContent"))
                .map(|m| m.name.trim_start_matches("models/").to_string()),
        );
        if list.next_page_token.is_empty() {
            break;
        }
        token = list.next_page_token;
    }
    Ok(out)
}

/// El modelo más nuevo de la familia ("flash", "flash-lite" o "pro").
/// Prefiere versiones estables; si no hay, acepta una preview.
pub fn pick_model(models: &[String], family: &str) -> Option<String> {
    rank_models(models, family).into_iter().next()
}

/// Modelos de la familia, del mejor al peor: estables primero (más nuevo arriba), después previews.
pub fn rank_models(models: &[String], family: &str) -> Vec<String> {
    const SKIP: [&str; 12] = ["image", "tts", "audio", "live", "embedding", "native", "computer", "robotics", "learnlm", "gemma", "vision", "aqa"];
    let in_family = |n: &str| match family {
        "flash-lite" => n.contains("-flash-lite"),
        "pro" => n.contains("-pro"),
        _ => n.contains("-flash") && !n.contains("-flash-lite"),
    };
    let version = |n: &str| -> f64 { n.trim_start_matches("gemini-").split('-').next().and_then(|v| v.parse().ok()).unwrap_or(-1.0) };
    let candidates: Vec<&String> = models
        .iter()
        .filter(|n| n.starts_with("gemini-") && in_family(n) && version(n) > 0.0 && !SKIP.iter().any(|s| n.contains(s)))
        .collect();
    let unstable = |n: &str| n.contains("preview") || n.contains("exp");
    let mut v: Vec<String> = candidates.into_iter().cloned().collect();
    v.sort_by(|a, b| {
        unstable(a)
            .cmp(&unstable(b))
            .then(version(b).partial_cmp(&version(a)).unwrap_or(std::cmp::Ordering::Equal))
            // A igual versión, el nombre más corto (el alias, no "-001" ni fechas).
            .then(a.len().cmp(&b.len()))
    });
    v
}

/// ¿Falla pasajera de Google (saturado, límite, caída o sin respuesta)?
fn transient(status: u32) -> bool {
    matches!(status, 0 | 429 | 500 | 502 | 503 | 504)
}

/// Si Google sugiere un reemplazo ("use models/gemini-x-flash"), lo extraemos.
fn suggested_model(msg: &str) -> Option<String> {
    // "This model models/<viejo> ... use models/<nuevo>": nos quedamos con el que sigue a "use".
    let i = msg.find("use models/").map(|i| i + 4).or_else(|| msg.rfind("models/"))?;
    let name: String = msg[i + 7..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.').collect();
    let name = name.trim_end_matches('.').to_string();
    name.starts_with("gemini-").then_some(name)
}

/// ¿El error dice que el modelo ya no existe o no está disponible para esta key?
fn model_gone(status: u32, msg: &str) -> bool {
    let m = msg.to_lowercase();
    status == 404 || m.contains("no longer available") || m.contains("not found") || m.contains("is not supported") || m.contains("deprecated")
}

/// Elige el mejor modelo disponible para la familia pedida.
fn resolve_model(api_key: &str, family: &str, hint: Option<&str>) -> Result<String, String> {
    match list_models(api_key) {
        Ok(models) => pick_model(&models, family)
            .or_else(|| hint.and_then(suggested_model))
            .ok_or_else(|| "Tu key no tiene ningún modelo Gemini de texto disponible".to_string()),
        Err(e) => hint.and_then(suggested_model).ok_or(e),
    }
}

fn query_gemini(api_key: &str, model: &str, family: &str, task: PromptTask, user_text: &str) -> Result<String, String> {
    let header = auth_header(api_key)?;
    let sys_prompt = match task {
        PromptTask::Mode(AskMode::Ask) => SYSTEM_PROMPT_ASK,
        PromptTask::Mode(AskMode::Translate) => SYSTEM_PROMPT_TRANSLATE,
        PromptTask::SummarizeClip => SYSTEM_PROMPT_SUMMARIZE,
        PromptTask::SpellcheckClip => SYSTEM_PROMPT_SPELLCHECK,
    };
    let mut model = model.trim().to_string();
    if model.is_empty() {
        model = resolve_model(api_key, family, None)?;
        *NEW_MODEL.lock().unwrap() = Some(model.clone());
    }
    match generate(&header, &model, sys_prompt, user_text) {
        // El modelo guardado se retiró: probamos los siguientes de la lista (hasta 3)
        // y recordamos el primero que responda.
        Err((status, msg)) if model_gone(status, &msg) => {
            let mut queue: Vec<String> = Vec::new();
            if let Some(s) = suggested_model(&msg) {
                queue.push(s);
            }
            if let Ok(m) = list_models(api_key) {
                queue.extend(rank_models(&m, family));
            }
            queue.retain(|n| *n != model);
            queue.dedup();
            let mut last = msg;
            for cand in queue.into_iter().take(3) {
                match generate(&header, &cand, sys_prompt, user_text) {
                    Ok(t) => {
                        *NEW_MODEL.lock().unwrap() = Some(cand);
                        return Ok(t);
                    }
                    Err((s, m)) if model_gone(s, &m) => last = m,
                    Err((s, m)) => return Err(friendly(s, &m, &m)),
                }
            }
            Err(last)
        }
        // Saturado o caído: probamos una vez con el modelo anterior de la misma familia
        // (sin guardarlo: la próxima vez se vuelve a intentar con el más nuevo).
        // Saturado, caído o colgado: probamos los siguientes de la familia (hasta 4) y
        // recordamos el que respondió, así las próximas consultas van directo a ese.
        Err((status, msg)) if transient(status) => {
            let queue: Vec<String> = list_models(api_key)
                .map(|m| rank_models(&m, family))
                .unwrap_or_default()
                .into_iter()
                .filter(|n| *n != model)
                .take(4)
                .collect();
            let mut last = (status, msg);
            for cand in queue {
                match generate(&header, &cand, sys_prompt, user_text) {
                    Ok(t) => {
                        *NEW_MODEL.lock().unwrap() = Some(cand);
                        return Ok(t);
                    }
                    Err(e) => last = e,
                }
            }
            Err(friendly(last.0, &last.1, &last.1))
        }
        r => r.map_err(|(_, m)| m),
    }
}

fn friendly(status: u32, last: &str, first: &str) -> String {
    match status {
        0 => "Gemini no respondió a tiempo; probá de nuevo en un rato".into(),
        429 => "Llegaste al límite gratis de Gemini por ahora; probá en unos minutos".into(),
        500..=504 => "Los servidores de Gemini están saturados; probá de nuevo en un rato".into(),
        _ if !last.is_empty() => last.into(),
        _ => first.into(),
    }
}

/// Espera por modelo antes de pasar al siguiente (un Flash sano responde en 1–5 s).
const ATTEMPT_MS: i32 = 15_000;

fn generate(header: &str, model: &str, sys_prompt: &str, user_text: &str) -> Result<String, (u32, String)> {
    let path = format!("/v1beta/models/{}:generateContent", http::encode(model));
    // Margen amplio: los modelos que "piensan" gastan parte del límite antes de responder.
    let body = format!(
        r#"{{"systemInstruction":{{"parts":[{{"text":"{}"}}]}},"contents":[{{"role":"user","parts":[{{"text":"{}"}}]}}],"generationConfig":{{"temperature":0.3,"maxOutputTokens":2048}}}}"#,
        escape_json(sys_prompt),
        escape_json(user_text.trim())
    );
    let resp = http::post_json(HOST, &path, header, &body, ATTEMPT_MS).ok_or((0, "Sin conexión o sin respuesta de Gemini".to_string()))?;
    let parsed = GeminiResponse::deserialize_json(&resp.body).map_err(|_| (resp.status, format!("Respuesta inválida de Gemini (HTTP {})", resp.status)))?;
    if resp.status != 200 {
        let msg = parsed.error.map(|e| e.message).filter(|m| !m.is_empty()).unwrap_or_else(|| format!("Error HTTP {}", resp.status));
        return Err((resp.status, msg));
    }
    let raw: String = parsed
        .candidates
        .first()
        .map(|c| c.content.parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        return Err((200, "Gemini no devolvió texto para esta consulta".into()));
    }
    Ok(sanitize_plain_text(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn elige_el_flash_mas_nuevo_y_estable() {
        let m = names(&[
            "gemini-2.5-flash",
            "gemini-3.8-flash",
            "gemini-3.8-flash-001",
            "gemini-4.0-flash-preview",
            "gemini-3.8-flash-lite",
            "gemini-3.8-pro",
            "gemini-3.8-flash-image",
            "gemma-3-27b-it",
        ]);
        assert_eq!(pick_model(&m, "flash").as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(pick_model(&m, "flash-lite").as_deref(), Some("gemini-3.8-flash-lite"));
        assert_eq!(pick_model(&m, "pro").as_deref(), Some("gemini-3.8-pro"));
        assert_eq!(pick_model(&names(&["gemini-4.0-flash-preview"]), "flash").as_deref(), Some("gemini-4.0-flash-preview"));
        // Lista real de una key de AI Studio (nov. 2026).
        let real = names(&["gemini-2.5-flash", "gemini-flash-latest", "gemini-3-flash-preview", "gemini-3.1-flash-lite", "gemini-3.5-flash", "gemini-3.5-flash-lite", "gemini-omni-1.2-flash", "gemini-3.6-flash", "gemini-3.7-flash", "gemini-3.8-flash", "gemini-3.8-flash-tts", "gemini-3.8-flash-lite-tts", "gemini-3.1-pro-preview", "gemini-2.5-pro"]);
        assert_eq!(rank_models(&real, "flash")[..2], ["gemini-3.8-flash".to_string(), "gemini-3.7-flash".to_string()]);
        assert_eq!(pick_model(&real, "flash-lite").as_deref(), Some("gemini-3.5-flash-lite"));
        assert_eq!(pick_model(&real, "pro").as_deref(), Some("gemini-2.5-pro"));
    }

    #[test]
    fn lee_el_modelo_sugerido_por_google() {
        let msg = "This model models/gemini-2.5-flash is no longer available to new users. Please update your code to use models/gemini-3.8-flash for the latest features.";
        assert_eq!(suggested_model(msg).as_deref(), Some("gemini-3.8-flash"));
        assert!(model_gone(400, msg));
    }
}
