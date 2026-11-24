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

/// Dispara la consulta a Gemini en un hilo separado y avisa con `WM_ASK_DONE` al terminar.
pub fn spawn_query(
    hwnd: HWND,
    req_id: u64,
    api_key: String,
    model: String,
    task: PromptTask,
    user_text: String,
    shared: Arc<Mutex<Option<(u64, Result<String, String>)>>>,
) {
    let hwnd_val = hwnd.0 as isize;
    std::thread::spawn(move || {
        let res = query_gemini(&api_key, &model, task, &user_text);
        *shared.lock().unwrap() = Some((req_id, res));
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd_val as *mut _)), WM_ASK_DONE, WPARAM(0), LPARAM(0));
        }
    });
}

fn query_gemini(api_key: &str, model: &str, task: PromptTask, user_text: &str) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("Copiá tu API Key de Gemini y elegí Bandeja → Configuración → IA Gemini → Pegar API Key".into());
    }
    let sys_prompt = match task {
        PromptTask::Mode(AskMode::Ask) => SYSTEM_PROMPT_ASK,
        PromptTask::Mode(AskMode::Translate) => SYSTEM_PROMPT_TRANSLATE,
        PromptTask::SummarizeClip => SYSTEM_PROMPT_SUMMARIZE,
        PromptTask::SpellcheckClip => SYSTEM_PROMPT_SPELLCHECK,
    };
    let model_name = if model.trim().is_empty() { "gemini-2.5-flash" } else { model.trim() };
    let path = format!("/v1beta/models/{}:generateContent", http::encode(model_name));
    // La key va en un header y no en la URL, así no queda en logs ni proxies.
    let key = api_key.trim();
    if key.chars().any(|c| c.is_control()) {
        return Err("La API Key guardada no es válida; pegala de nuevo desde la bandeja".into());
    }
    let auth_header = format!("x-goog-api-key: {key}\r\n");
    let body = format!(
        r#"{{"systemInstruction":{{"parts":[{{"text":"{}"}}]}},"contents":[{{"role":"user","parts":[{{"text":"{}"}}]}}],"generationConfig":{{"temperature":0.3,"maxOutputTokens":220}}}}"#,
        escape_json(sys_prompt),
        escape_json(user_text.trim())
    );
    let resp = http::post_json("generativelanguage.googleapis.com", &path, &auth_header, &body)
        .ok_or_else(|| "Sin conexión con la API de Gemini".to_string())?;
    let parsed = GeminiResponse::deserialize_json(&resp.body)
        .map_err(|_| format!("Respuesta inválida de Gemini (HTTP {})", resp.status))?;
    if resp.status != 200 {
        let msg = parsed
            .error
            .map(|e| e.message)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| format!("Error HTTP {}", resp.status));
        return Err(msg);
    }
    let raw = parsed
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .map(|p| p.text.as_str())
        .unwrap_or("")
        .trim();
    if raw.is_empty() {
        return Err("Gemini no devolvió texto para esta consulta".into());
    }
    Ok(sanitize_plain_text(raw))
}
