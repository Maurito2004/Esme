//! Smart OCR (Ctrl+Alt+T): congela la pantalla, elegís un área con el mouse y el texto se
//! reconoce offline con el motor nativo de Windows (Windows.Media.Ocr) y se copia al portapapeles.

use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use windows::core::{w, Result};
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_OCR_DONE: u32 = WM_APP + 7;

/// Resultado del último OCR inicializado de forma diferida (OnceLock): 0 asignaciones al arrancar.
static RESULT: OnceLock<Mutex<Option<std::result::Result<String, String>>>> = OnceLock::new();

fn result_slot() -> &'static Mutex<Option<std::result::Result<String, String>>> {
    RESULT.get_or_init(|| Mutex::new(None))
}

pub fn take_result() -> Option<std::result::Result<String, String>> {
    RESULT.get()?.lock().unwrap().take()
}

struct Overlay {
    hwnd: HWND,
    owner: HWND,
    vx: i32,
    vy: i32,
    orig: HDC,
    orig_bmp: HBITMAP,
    dim: HDC,
    dim_bmp: HBITMAP,
    back: HDC,
    back_bmp: HBITMAP,
    start: Option<POINT>,
    cur: POINT,
}

thread_local! {
    static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
}

fn sel_rect(a: POINT, b: POINT) -> RECT {
    RECT { left: a.x.min(b.x), top: a.y.min(b.y), right: a.x.max(b.x), bottom: a.y.max(b.y) }
}

/// Abre la selección. `owner` recibe WM_OCR_DONE cuando termina.
pub fn start(owner: HWND) {
    if OVERLAY.with(|o| o.borrow().is_some()) {
        return;
    }
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        // Foto de toda la pantalla (todos los monitores).
        let screen = GetDC(None);
        let mk = |dc: HDC| {
            let mem = CreateCompatibleDC(Some(dc));
            let bmp = CreateCompatibleBitmap(dc, vw, vh);
            SelectObject(mem, bmp.into());
            (mem, bmp)
        };
        let (orig, orig_bmp) = mk(screen);
        let (dim, dim_bmp) = mk(screen);
        let (back, back_bmp) = mk(screen);
        let _ = BitBlt(orig, 0, 0, vw, vh, Some(screen), vx, vy, SRCCOPY | CAPTUREBLT);
        ReleaseDC(None, screen);

        // Versión oscurecida para el fondo.
        let _ = BitBlt(dim, 0, 0, vw, vh, Some(orig), 0, 0, SRCCOPY);
        let black = CreateCompatibleDC(Some(orig));
        let px = CreateCompatibleBitmap(orig, 1, 1);
        SelectObject(black, px.into());
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 120, AlphaFormat: 0 };
        let _ = AlphaBlend(dim, 0, 0, vw, vh, black, 0, 0, 1, 1, blend);
        let _ = DeleteDC(black);
        let _ = DeleteObject(px.into());

        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("EsmeOcr");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class,
            w!("Esme OCR"),
            WS_POPUP,
            vx,
            vy,
            vw,
            vh,
            None,
            None,
            Some(hinst.into()),
            None,
        ) else {
            return;
        };
        OVERLAY.with(|o| {
            *o.borrow_mut() = Some(Overlay {
                hwnd,
                owner,
                vx,
                vy,
                orig,
                orig_bmp,
                dim,
                dim_bmp,
                back,
                back_bmp,
                start: None,
                cur: POINT::default(),
            })
        });
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
}

fn close() -> Option<Overlay> {
    let ov = OVERLAY.with(|o| o.borrow_mut().take())?;
    unsafe {
        let _ = DestroyWindow(ov.hwnd);
    }
    Some(ov)
}

fn free(ov: &Overlay) {
    unsafe {
        let _ = DeleteDC(ov.orig);
        let _ = DeleteDC(ov.dim);
        let _ = DeleteDC(ov.back);
        let _ = DeleteObject(ov.orig_bmp.into());
        let _ = DeleteObject(ov.dim_bmp.into());
        let _ = DeleteObject(ov.back_bmp.into());
    }
}

fn paint(ov: &Overlay, dirty: RECT, target: HDC) {
    unsafe {
        let (x, y, w, h) = (dirty.left, dirty.top, dirty.right - dirty.left, dirty.bottom - dirty.top);
        let _ = BitBlt(ov.back, x, y, w, h, Some(ov.dim), x, y, SRCCOPY);
        if let Some(s) = ov.start {
            let r = sel_rect(s, ov.cur);
            let _ = BitBlt(ov.back, r.left, r.top, r.right - r.left, r.bottom - r.top, Some(ov.orig), r.left, r.top, SRCCOPY);
            let brush = CreateSolidBrush(COLORREF(0x0040B0FF)); // ámbar (BGR)
            let _ = FrameRect(ov.back, &RECT { left: r.left - 1, top: r.top - 1, right: r.right + 1, bottom: r.bottom + 1 }, brush);
            let _ = DeleteObject(brush.into());
        }
        // Ayuda arriba del monitor principal.
        let mut hint: Vec<u16> = "Esme OCR · arrastrá para elegir el texto · Esc para cancelar".encode_utf16().collect();
        SetBkMode(ov.back, TRANSPARENT);
        SetTextColor(ov.back, COLORREF(0x00FFFFFF));
        let mut tr = RECT { left: -ov.vx, top: -ov.vy + 18, right: -ov.vx + GetSystemMetrics(SM_CXSCREEN), bottom: -ov.vy + 50 };
        DrawTextW(ov.back, &mut hint, &mut tr, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        let _ = BitBlt(target, x, y, w, h, Some(ov.back), x, y, SRCCOPY);
    }
}

fn invalidate(ov: &Overlay, old: Option<RECT>) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let mut add = |a: RECT| {
        if r.right == r.left {
            r = a;
        } else {
            r = RECT { left: r.left.min(a.left), top: r.top.min(a.top), right: r.right.max(a.right), bottom: r.bottom.max(a.bottom) };
        }
    };
    if let Some(o) = old {
        add(o);
    }
    if let Some(s) = ov.start {
        add(sel_rect(s, ov.cur));
    }
    let r = RECT { left: r.left - 4, top: r.top - 4, right: r.right + 4, bottom: r.bottom + 4 };
    unsafe {
        let _ = InvalidateRect(Some(ov.hwnd), Some(&r), false);
    }
}

fn pt_of(lp: LPARAM) -> POINT {
    POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 }
}

/// Recorta el área elegida (ampliada x2 si es chica: el OCR lee mejor) y la reconoce en otro hilo.
fn recognize(ov: &Overlay, r: RECT) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let s = if w < 900 && h < 400 { 2 } else { 1 };
    let (cw, ch) = (w * s, h * s);
    let mut px = vec![0u8; (cw * ch * 4) as usize];
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: cw,
                biHeight: -ch,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(Some(ov.orig), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else { return };
        let dc = CreateCompatibleDC(Some(ov.orig));
        SelectObject(dc, dib.into());
        SetStretchBltMode(dc, HALFTONE);
        let _ = StretchBlt(dc, 0, 0, cw, ch, Some(ov.orig), r.left, r.top, w, h, SRCCOPY);
        std::ptr::copy_nonoverlapping(bits as *const u8, px.as_mut_ptr(), px.len());
        let _ = DeleteDC(dc);
        let _ = DeleteObject(dib.into());
    }
    for a in px.chunks_exact_mut(4) {
        a[3] = 255;
    }
    let owner = ov.owner.0 as isize;
    std::thread::spawn(move || {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let out = run_ocr(px, cw, ch).map_err(|e| e.message().to_string()).and_then(|o| o);
        *result_slot().lock().unwrap() = Some(out);
        unsafe {
            let _ = PostMessageW(Some(HWND(owner as *mut _)), WM_OCR_DONE, WPARAM(0), LPARAM(0));
        }
    });
}

fn run_ocr(px: Vec<u8>, w: i32, h: i32) -> Result<std::result::Result<String, String>> {
    // El motor WinRT (Windows.Media.Ocr) solo se instancia aquí, la primera vez que se usa OCR,
    // y se suelta al terminar para no retener modelos de idioma en RAM en reposo.
    let Ok(engine) = OcrEngine::TryCreateFromUserProfileLanguages() else {
        return Ok(Err("OCR no disponible: instalá un idioma con reconocimiento de texto en Windows".into()));
    };
    let buf = CryptographicBuffer::CreateFromByteArray(&px)?;
    // Liberamos el buffer BGRA crudo antes de copiar/reconocer.
    drop(px);
    let bmp = SoftwareBitmap::CreateCopyFromBuffer(&buf, BitmapPixelFormat::Bgra8, w, h)?;
    drop(buf);
    let res = engine.RecognizeAsync(&bmp)?.get()?;
    let mut lines = Vec::new();
    for line in res.Lines()? {
        lines.push(line.Text()?.to_string());
    }
    let text = lines.join("\n").trim().to_string();
    Ok(if text.is_empty() { Err("No encontré texto en esa zona".into()) } else { Ok(text) })
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                OVERLAY.with(|o| {
                    if let Some(ov) = o.borrow().as_ref() {
                        paint(ov, ps.rcPaint, dc);
                    }
                });
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                OVERLAY.with(|o| {
                    if let Some(ov) = o.borrow_mut().as_mut() {
                        let p = pt_of(lp);
                        ov.start = Some(p);
                        ov.cur = p;
                        SetCapture(hwnd);
                        invalidate(ov, None);
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                OVERLAY.with(|o| {
                    if let Some(ov) = o.borrow_mut().as_mut() {
                        if let Some(s) = ov.start {
                            let old = sel_rect(s, ov.cur);
                            ov.cur = pt_of(lp);
                            invalidate(ov, Some(old));
                        }
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                if let Some(ov) = close() {
                    if let Some(s) = ov.start {
                        let r = sel_rect(s, pt_of(lp));
                        if r.right - r.left >= 6 && r.bottom - r.top >= 6 {
                            recognize(&ov, r);
                        }
                    }
                    free(&ov);
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                if let Some(ov) = close() {
                    free(&ov);
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
                if let Some(ov) = close() {
                    free(&ov);
                }
                LRESULT(0)
            }
            WM_ACTIVATE if (wp.0 & 0xFFFF) == 0 => {
                // Si se pierde el foco (Alt+Tab), cancelamos.
                if let Some(ov) = close() {
                    free(&ov);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
