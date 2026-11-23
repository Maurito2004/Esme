//! Detección de pantalla completa (juegos, videos) por monitor.
//! No hace polling: se consulta solo cuando Windows avisa un cambio de ventana en primer
//! plano, minimizado/restaurado, o el aviso ABN_FULLSCREENAPP del shell.

use windows::core::{BOOL, PWSTR};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

/// Juegos que cuentan como "modo juego" aunque estén maximizados con barra de título.
pub const JUEGOS_BASE: &[&str] = &["robloxplayerbeta.exe", "robloxplayer.exe", "windows10universal.exe"];
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

struct Scan<'a> {
    games: &'a [String],
    own: HWND,
    fg: HWND,
    decided: Vec<isize>,
    full: Vec<isize>,
}

/// Monitores cuyo contenido de arriba es una ventana a pantalla completa.
pub fn fullscreen_monitors(own: HWND, games: &[String]) -> Vec<isize> {
    let mut s = Scan { games, own, fg: unsafe { GetForegroundWindow() }, decided: Vec::new(), full: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut s as *mut Scan as isize));
    }
    s.full
}

unsafe extern "system" fn visit(hwnd: HWND, lp: LPARAM) -> BOOL {
    let s = &mut *(lp.0 as *mut Scan);
    if hwnd == s.own || !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return true.into();
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    // Overlays (NVIDIA, Discord, etc.), ventanas de herramientas y "siempre visibles" que no
    // tienen el foco no cuentan: son capas transparentes encima del escritorio.
    if ex & (WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0) != 0 {
        return true.into();
    }
    if ex & WS_EX_TOPMOST.0 != 0 && hwnd != s.fg {
        return true.into();
    }
    let mut cloaked = 0u32;
    if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4).is_ok() && cloaked != 0 {
        return true.into();
    }
    let mut cls = [0u16; 64];
    let n = GetClassNameW(hwnd, &mut cls) as usize;
    let class = String::from_utf16_lossy(&cls[..n]);
    if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return true.into();
    }
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_err() || r.right - r.left < 50 || r.bottom - r.top < 50 {
        return true.into();
    }
    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let key = mon.0 as isize;
    if s.decided.contains(&key) {
        return true.into();
    }
    // La primera ventana real (en orden Z) sobre cada monitor decide.
    s.decided.push(key);
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(mon, &mut mi).as_bool() {
        let m = mi.rcMonitor;
        let covers = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
        // Una ventana maximizada normal tiene barra de título: esa no es "modo juego".
        let has_caption = style & WS_CAPTION.0 == WS_CAPTION.0;
        let is_game = (IsZoomed(hwnd).as_bool() || covers) && s.games.iter().any(|g| *g == process_name(hwnd));
        if (covers && !has_caption) || is_game {
            s.full.push(key);
        }
    }
    true.into()
}

/// Nombre del ejecutable de la ventana, en minúsculas (ej. "robloxplayerbeta.exe").
fn process_name(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(h);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().unwrap_or("").to_lowercase()
    }
}

/// Todos los monitores, el principal primero.
pub fn monitors() -> Vec<HMONITOR> {
    unsafe extern "system" fn cb(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
        (*(lp.0 as *mut Vec<HMONITOR>)).push(m);
        true.into()
    }
    let mut v: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut v as *mut _ as isize));
        let primary = MonitorFromPoint(windows::Win32::Foundation::POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        v.sort_by_key(|m| m.0 != primary.0);
    }
    v
}
