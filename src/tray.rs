//! Ícono en la bandeja del sistema y su menú contextual.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

pub const CMD_AUTOSTART: u32 = 100;
pub const CMD_PET_ESME: u32 = 110;
pub const CMD_PET_GATO: u32 = 111;
pub const CMD_PET_OFF: u32 = 112;
pub const CMD_MINIMO: u32 = 102;
pub const CMD_AJUSTES: u32 = 103;
pub const CMD_SALIR: u32 = 104;
pub const CMD_TRANSPARENTE: u32 = 105;
pub const CMD_MOVER: u32 = 106;

pub struct MenuState {
    pub autostart: bool,
    /// 0 = Esme, 1 = Gato, 2 = desactivada.
    pub pet: u8,
    pub minimo: bool,
    pub transparente: bool,
    pub mover: bool,
}

#[derive(Clone)]
pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        let t = Self { hwnd, icon: load_icon() };
        t.add();
        t
    }

    /// (Re)agrega el ícono. Se llama también cuando el Explorador se reinicia.
    pub fn add(&self) {
        unsafe {
            let mut nid = self.nid();
            nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
        }
    }

    fn nid(&self) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            ..Default::default()
        };
        let tip: Vec<u16> = "Esme".encode_utf16().collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        nid
    }

    pub fn remove(&self) {
        unsafe {
            let nid = self.nid();
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            let _ = DestroyIcon(self.icon);
        }
    }

    /// Muestra el menú y devuelve el comando elegido (0 si se canceló).
    pub fn show_menu(&self, s: &MenuState) -> u32 {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return 0 };
            let chk = |b: bool| if b { MF_CHECKED } else { MF_UNCHECKED };
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, w!("Esme"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING | chk(s.autostart), CMD_AUTOSTART as usize, w!("Iniciar con Windows"));
            if let Ok(sub) = CreatePopupMenu() {
                let _ = AppendMenuW(sub, MF_STRING | chk(s.pet == 0), CMD_PET_ESME as usize, w!("Esme (Perrito)"));
                let _ = AppendMenuW(sub, MF_STRING | chk(s.pet == 1), CMD_PET_GATO as usize, w!("Gato"));
                let _ = AppendMenuW(sub, MF_STRING | chk(s.pet == 2), CMD_PET_OFF as usize, w!("Desactivada"));
                let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Mascota"));
            }
            let _ = AppendMenuW(menu, MF_STRING | chk(s.minimo), CMD_MINIMO as usize, w!("Notch mínimo"));
            let _ = AppendMenuW(menu, MF_STRING | chk(s.transparente), CMD_TRANSPARENTE as usize, w!("Fondo transparente al expandir"));
            let _ = AppendMenuW(menu, MF_STRING | chk(s.mover), CMD_MOVER as usize, w!("En pantalla completa: pasar al otro monitor"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_AJUSTES as usize, w!("Ajustes…"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_SALIR as usize, w!("Salir"));

            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            // Necesario para que el menú se cierre al hacer clic afuera.
            let _ = SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RIGHTALIGN,
                p.x,
                p.y,
                None,
                self.hwnd,
                None,
            );
            let _ = PostMessageW(Some(self.hwnd), WM_NULL, Default::default(), Default::default());
            let _ = DestroyMenu(menu);
            cmd.0 as u32
        }
    }
}

/// Ícono embebido en el exe (res/noosa.ico), al tamaño justo para la bandeja.
fn load_icon() -> HICON {
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        LoadImageW(Some(hinst.into()), PCWSTR(1 as *const u16), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
    }
}
