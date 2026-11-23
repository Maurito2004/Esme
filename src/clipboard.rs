//! Historial del portapapeles (solo texto), guardado en %APPDATA%\Esme\portapapeles.json.
//! Por eventos: Windows avisa cada copia con WM_CLIPBOARDUPDATE. Respeta a los gestores de
//! contraseñas que piden no guardar lo copiado.

use nanoserde::{DeJson, SerJson};
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

const CF_UNICODETEXT: u32 = 13;
const MAX_ITEMS: usize = 20;
const MAX_CHARS: usize = 100_000;

#[derive(DeJson, SerJson, Default)]
pub struct History {
    #[nserde(default)]
    pub items: Vec<String>,
}

fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("portapapeles.json")
}

impl History {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| History::deserialize_json(&s).ok()).unwrap_or_default()
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(crate::config::data_dir());
        let _ = std::fs::write(path(), self.serialize_json());
    }

    /// Agrega un texto al principio (sin duplicados). Devuelve true si cambió algo.
    pub fn push(&mut self, text: String) -> bool {
        if text.trim().is_empty() || text.chars().count() > MAX_CHARS {
            return false;
        }
        if self.items.first() == Some(&text) {
            return false;
        }
        self.items.retain(|t| *t != text);
        self.items.insert(0, text);
        self.items.truncate(MAX_ITEMS);
        self.save();
        true
    }
}

/// Lee el texto del portapapeles, salvo que la app que copió pidió excluirlo del historial.
pub fn read_text(hwnd: HWND) -> Option<String> {
    unsafe {
        let exclude = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let can_hist = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if IsClipboardFormatAvailable(exclude).is_ok() || IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        // Otra app puede tenerlo abierto un instante: reintentamos un poco.
        let mut opened = false;
        for _ in 0..4 {
            if OpenClipboard(Some(hwnd)).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        if !opened {
            return None;
        }
        let result = (|| {
            if let Ok(h) = GetClipboardData(can_hist) {
                let p = GlobalLock(HGLOBAL(h.0)) as *const u32;
                if !p.is_null() {
                    let allowed = *p != 0;
                    let _ = GlobalUnlock(HGLOBAL(h.0));
                    if !allowed {
                        return None;
                    }
                }
            }
            let h = GetClipboardData(CF_UNICODETEXT).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let max = GlobalSize(g) / 2;
            let len = (0..max).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Copia `text` al portapapeles.
pub fn set_text(hwnd: HWND, text: &str) -> bool {
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let ok = (|| {
            let g = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let p = GlobalLock(g) as *mut u16;
            if p.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(g);
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))).ok()
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}
