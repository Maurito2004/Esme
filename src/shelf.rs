//! Estante de archivos (File Shelf): soltás archivos sobre el notch y quedan a mano.
//! Solo guarda las rutas (nunca carga los archivos en memoria), en %APPDATA%\Esme\estante.json.
//! Entrada: OLE drag & drop (IDropTarget). Salida: arrastrar un ítem a Discord, el navegador o
//! una carpeta (DoDragDrop con el IDataObject del propio shell), o clic para abrirlo.

use nanoserde::{DeJson, SerJson};
use std::cell::Cell;
use std::sync::Mutex;
use windows::core::{implement, Ref, Result, BOOL, HRESULT, HSTRING};
use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, LPARAM, POINTL, S_OK, WPARAM};
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{BHID_DataObject, DragQueryFileW, IShellItem, SHCreateItemFromParsingName, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_SHELF_DRAG: u32 = WM_APP + 5;
pub const WM_SHELF_DROP: u32 = WM_APP + 6;
pub const MAX_ITEMS: usize = 10;
const CF_HDROP: u16 = 15;

/// Rutas recién soltadas, esperando que el hilo de la UI las tome.
static DROPPED: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(DeJson, SerJson, Default)]
pub struct Shelf {
    #[nserde(default)]
    pub items: Vec<String>,
}

fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("estante.json")
}

impl Shelf {
    pub fn load() -> Self {
        let mut s: Shelf =
            std::fs::read_to_string(path()).ok().and_then(|s| Shelf::deserialize_json(s.trim_start_matches('\u{feff}')).ok()).unwrap_or_default();
        // Si un archivo se borró o movió desde la última vez, lo sacamos.
        s.items.retain(|p| std::path::Path::new(p).exists());
        s
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(crate::config::data_dir());
        let _ = std::fs::write(path(), self.serialize_json());
    }

    /// Agrega rutas al principio (sin duplicados). Devuelve cuántas eran nuevas.
    pub fn add(&mut self, paths: Vec<String>) -> usize {
        let mut added = 0;
        for p in paths.into_iter().rev() {
            if !self.items.contains(&p) {
                added += 1;
            }
            self.items.retain(|x| *x != p);
            self.items.insert(0, p);
        }
        self.items.truncate(MAX_ITEMS);
        self.save();
        added
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.items.len() {
            self.items.remove(i);
            self.save();
        }
    }
}

/// Nombre visible (sin la carpeta).
pub fn file_name(p: &str) -> String {
    std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

/// Glifo de Segoe Fluent Icons según el tipo de archivo.
pub fn glyph(p: &str) -> &'static str {
    let path = std::path::Path::new(p);
    if path.is_dir() {
        return "\u{E8B7}"; // carpeta
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" | "heic" => "\u{EB9F}",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" => "\u{E8D6}",
        "mp4" | "mkv" | "mov" | "avi" | "webm" => "\u{E714}",
        "zip" | "rar" | "7z" | "tar" | "gz" => "\u{F012}",
        "exe" | "msi" | "bat" | "cmd" | "ps1" => "\u{E756}",
        "pdf" => "\u{EA90}",
        _ => "\u{E8A5}", // documento
    }
}

pub fn take_dropped() -> Vec<String> {
    std::mem::take(&mut *DROPPED.lock().unwrap())
}

fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

// ---------- Entrada: soltar archivos sobre el notch ----------

#[implement(IDropTarget)]
pub struct DropTarget {
    hwnd: isize,
    accept: Cell<bool>,
}

impl DropTarget {
    pub fn new(hwnd: HWND) -> IDropTarget {
        DropTarget { hwnd: hwnd.0 as isize, accept: Cell::new(false) }.into()
    }

    fn post(&self, msg: u32, wp: usize) {
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), msg, WPARAM(wp), LPARAM(0));
        }
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, data: Ref<'_, IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let ok = data.as_ref().map(|d| unsafe { d.QueryGetData(&hdrop_format()).is_ok() }).unwrap_or(false);
        self.accept.set(ok);
        unsafe { *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        if ok {
            self.post(WM_SHELF_DRAG, 1);
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        unsafe { *effect = if self.accept.get() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        if self.accept.get() {
            self.post(WM_SHELF_DRAG, 0);
        }
        Ok(())
    }

    fn Drop(&self, data: Ref<'_, IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let mut paths = Vec::new();
        if let Some(d) = data.as_ref() {
            unsafe {
                if let Ok(mut medium) = d.GetData(&hdrop_format()) {
                    let hdrop = HDROP(medium.u.hGlobal.0);
                    let n = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
                    for i in 0..n {
                        let len = DragQueryFileW(hdrop, i, None) as usize;
                        let mut buf = vec![0u16; len + 1];
                        DragQueryFileW(hdrop, i, Some(&mut buf));
                        paths.push(String::from_utf16_lossy(&buf[..len]));
                    }
                    ReleaseStgMedium(&mut medium);
                }
            }
        }
        unsafe { *effect = if paths.is_empty() { DROPEFFECT_NONE } else { DROPEFFECT_COPY } };
        DROPPED.lock().unwrap().extend(paths);
        self.post(WM_SHELF_DROP, 0);
        Ok(())
    }
}

// ---------- Salida: arrastrar un ítem a otra app ----------

#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Arrastra `path` fuera del notch. Bloquea (bucle modal de OLE) hasta que se suelta.
pub fn drag_out(path: &str) {
    unsafe {
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path), None) else { return };
        let Ok(data) = item.BindToHandler::<_, IDataObject>(None, &BHID_DataObject) else { return };
        let source: IDropSource = DropSource.into();
        let mut effect = DROPEFFECT_NONE;
        // Solo copiar o vincular: nunca mover el archivo original fuera de su carpeta.
        let _ = DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect);
    }
}
