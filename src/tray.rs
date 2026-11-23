//! Ícono en la bandeja del sistema y su menú contextual.
//! El menú es "owner-draw": lo dibujamos nosotros (fondo, resaltado, íconos, tildes y flechas)
//! para que el tema oscuro / claro se respete en cualquier versión de Windows.

use std::cell::RefCell;
use std::collections::HashMap;
use windows::core::{w, Interface, PCSTR, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_DISABLED, ODS_SELECTED, ODT_MENU};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

pub const CMD_AUTOSTART: u32 = 100;
pub const CMD_MINIMO: u32 = 102;
pub const CMD_SALIR: u32 = 104;
pub const CMD_TRANSPARENTE: u32 = 105;
pub const CMD_MOVER: u32 = 106;
pub const CMD_PET_ESME: u32 = 110;
pub const CMD_PET_GATO: u32 = 111;
pub const CMD_PET_OFF: u32 = 112;
pub const CMD_OCR: u32 = 120;
pub const CMD_TEMA_OSCURO: u32 = 130;
pub const CMD_TEMA_CLARO: u32 = 131;

pub struct MenuState {
    pub autostart: bool,
    /// 0 = Esme, 1 = Gato, 2 = desactivada.
    pub pet: u8,
    pub minimo: bool,
    pub transparente: bool,
    pub mover: bool,
    pub dark: bool,
}

// Glifos de Segoe Fluent Icons / MDL2.
const I_OCR: &str = "\u{E8B3}";
const I_PET: &str = "\u{E76E}";
const I_CONFIG: &str = "\u{E713}";
const I_SALIR: &str = "\u{E7E8}";
const I_PANTALLA: &str = "\u{E7F4}";
const I_TEMA: &str = "\u{E790}";
const I_CHECK: &str = "\u{E73E}";
const I_CHEVRON: &str = "\u{E76C}";

/// Un ítem del menú tal como lo dibujamos.
struct Item {
    text: String,
    accel: String,
    glyph: Option<&'static str>,
    checked: bool,
    disabled: bool,
    sep: bool,
    sub: bool,
}

/// Recursos de pintura del menú abierto (fuente, colores, íconos ya rasterizados).
struct Paint {
    dark: bool,
    scale: f32,
    font: HFONT,
    icons: HashMap<&'static str, HBITMAP>,
    icon_px: i32,
}

thread_local! {
    static ITEMS: RefCell<Vec<Item>> = const { RefCell::new(Vec::new()) };
    static PAINT: RefCell<Option<Paint>> = const { RefCell::new(None) };
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
        set_menu_theme(s.dark);
        prepare_paint(self.hwnd, s.dark);
        ITEMS.with(|i| i.borrow_mut().clear());
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return 0 };
            let it = |text: &str, glyph: Option<&'static str>, checked: bool| Item {
                text: text.into(),
                accel: String::new(),
                glyph,
                checked,
                disabled: false,
                sep: false,
                sub: false,
            };

            add(menu, 0, Item { disabled: true, ..it("Esme", None, false) });
            add_sep(menu);
            add(menu, CMD_OCR as usize, Item { accel: "Ctrl+Alt+T".into(), ..it("Capturar texto", Some(I_OCR), false) });

            let pets = CreatePopupMenu().unwrap_or_default();
            add(pets, CMD_PET_ESME as usize, it("Esme (Perrito)", None, s.pet == 0));
            add(pets, CMD_PET_GATO as usize, it("Gato", None, s.pet == 1));
            add(pets, CMD_PET_OFF as usize, it("Desactivada", None, s.pet == 2));
            add_sub(menu, pets, it("Mascota", Some(I_PET), false));

            let cfg = CreatePopupMenu().unwrap_or_default();
            let theme = CreatePopupMenu().unwrap_or_default();
            add(theme, CMD_TEMA_OSCURO as usize, it("Oscuro", None, s.dark));
            add(theme, CMD_TEMA_CLARO as usize, it("Claro", None, !s.dark));
            add_sub(cfg, theme, it("Tema del menú", Some(I_TEMA), false));
            let look = CreatePopupMenu().unwrap_or_default();
            add(look, CMD_MINIMO as usize, it("Notch mínimo", None, s.minimo));
            add(look, CMD_TRANSPARENTE as usize, it("Fondo transparente al expandir", None, s.transparente));
            add_sub(cfg, look, it("Apariencia", Some(I_PANTALLA), false));
            add_sep(cfg);
            add(cfg, CMD_MOVER as usize, it("En pantalla completa: pasar al otro monitor", None, s.mover));
            add(cfg, CMD_AUTOSTART as usize, it("Iniciar con Windows", None, s.autostart));
            add_sub(menu, cfg, it("Configuración", Some(I_CONFIG), false));

            add_sep(menu);
            add(menu, CMD_SALIR as usize, it("Salir", Some(I_SALIR), false));

            // Fondo del menú (y de los submenús) en el color del tema.
            let (bg, _, _, _, _) = colors(s.dark);
            let brush = CreateSolidBrush(bg);
            let mi = MENUINFO {
                cbSize: std::mem::size_of::<MENUINFO>() as u32,
                fMask: MIM_BACKGROUND | MIM_APPLYTOSUBMENUS,
                hbrBack: brush,
                ..Default::default()
            };
            let _ = SetMenuInfo(menu, &mi);

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
            let _ = DeleteObject(brush.into());
            free_paint();
            cmd.0 as u32
        }
    }
}

fn add(menu: HMENU, id: usize, item: Item) {
    let idx = ITEMS.with(|i| {
        let mut v = i.borrow_mut();
        v.push(item);
        v.len()
    });
    let disabled = ITEMS.with(|i| i.borrow()[idx - 1].disabled);
    let flags = MF_OWNERDRAW | if disabled { MF_DISABLED } else { MF_ENABLED };
    unsafe {
        let _ = AppendMenuW(menu, flags, id, PCWSTR(idx as *const u16));
    }
}

fn add_sep(menu: HMENU) {
    let idx = ITEMS.with(|i| {
        let mut v = i.borrow_mut();
        v.push(Item { text: String::new(), accel: String::new(), glyph: None, checked: false, disabled: true, sep: true, sub: false });
        v.len()
    });
    unsafe {
        let _ = AppendMenuW(menu, MF_OWNERDRAW | MF_DISABLED, 0, PCWSTR(idx as *const u16));
    }
}

fn add_sub(menu: HMENU, sub: HMENU, mut item: Item) {
    item.sub = true;
    let idx = ITEMS.with(|i| {
        let mut v = i.borrow_mut();
        v.push(item);
        v.len()
    });
    unsafe {
        let _ = AppendMenuW(menu, MF_OWNERDRAW | MF_POPUP, sub.0 as usize, PCWSTR(idx as *const u16));
    }
}

/// (fondo, resaltado, texto, texto deshabilitado, separador)
fn colors(dark: bool) -> (COLORREF, COLORREF, COLORREF, COLORREF, COLORREF) {
    let rgb = |r: u32, g: u32, b: u32| COLORREF(r | (g << 8) | (b << 16));
    if dark {
        (rgb(0x2B, 0x2B, 0x2B), rgb(0x3D, 0x3D, 0x3D), rgb(0xF2, 0xF2, 0xF2), rgb(0x8A, 0x8A, 0x8A), rgb(0x45, 0x45, 0x45))
    } else {
        (rgb(0xF9, 0xF9, 0xF9), rgb(0xE6, 0xE6, 0xE6), rgb(0x1A, 0x1A, 0x1A), rgb(0x9A, 0x9A, 0x9A), rgb(0xE0, 0xE0, 0xE0))
    }
}

fn prepare_paint(owner: HWND, dark: bool) {
    unsafe {
        let dpi = GetDpiForWindow(owner).max(96);
        let scale = dpi as f32 / 96.0;
        let mut ncm = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let _ = SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS.0, ncm.cbSize, Some(&mut ncm as *mut _ as *mut _), 0, dpi);
        let font = CreateFontIndirectW(&ncm.lfMenuFont);
        let icon_px = (16.0 * scale).round() as i32;
        let mut icons = HashMap::new();
        if let Some(p) = IconPainter::new(dark, icon_px as u32) {
            for g in [I_OCR, I_PET, I_CONFIG, I_SALIR, I_PANTALLA, I_TEMA, I_CHECK, I_CHEVRON] {
                if let Some(b) = p.bitmap(g) {
                    icons.insert(g, b);
                }
            }
        }
        PAINT.with(|pt| *pt.borrow_mut() = Some(Paint { dark, scale, font, icons, icon_px }));
    }
}

fn free_paint() {
    if let Some(p) = PAINT.with(|pt| pt.borrow_mut().take()) {
        unsafe {
            let _ = DeleteObject(p.font.into());
            for (_, b) in p.icons {
                let _ = DeleteObject(b.into());
            }
        }
    }
    ITEMS.with(|i| i.borrow_mut().clear());
}

/// WM_MEASUREITEM del menú. Devuelve true si lo manejó.
pub fn measure_item(lp: LPARAM) -> bool {
    unsafe {
        let mis = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
        if mis.CtlType != ODT_MENU {
            return false;
        }
        let idx = mis.itemData;
        let (scale, font) = match PAINT.with(|p| p.borrow().as_ref().map(|p| (p.scale, p.font))) {
            Some(v) => v,
            None => return false,
        };
        ITEMS.with(|items| {
            let items = items.borrow();
            let Some(it) = items.get(idx.wrapping_sub(1)) else { return false };
            if it.sep {
                mis.itemHeight = (9.0 * scale) as u32;
                mis.itemWidth = 10;
                return true;
            }
            let dc = GetDC(None);
            let old = SelectObject(dc, font.into());
            let ext = |s: &str| {
                let w: Vec<u16> = s.encode_utf16().collect();
                let mut sz = SIZE::default();
                let _ = GetTextExtentPoint32W(dc, &w, &mut sz);
                sz.cx
            };
            let tw = ext(&it.text);
            let aw = if it.accel.is_empty() { 0 } else { ext(&it.accel) + (24.0 * scale) as i32 };
            SelectObject(dc, old);
            ReleaseDC(None, dc);
            mis.itemWidth = (tw + aw) as u32 + (64.0 * scale) as u32;
            mis.itemHeight = (32.0 * scale) as u32;
            true
        })
    }
}

/// WM_DRAWITEM del menú. Devuelve true si lo manejó.
pub fn draw_item(lp: LPARAM) -> bool {
    unsafe {
        let dis = &*(lp.0 as *const DRAWITEMSTRUCT);
        if dis.CtlType != ODT_MENU {
            return false;
        }
        PAINT.with(|p| {
            let p = p.borrow();
            let Some(p) = p.as_ref() else { return false };
            ITEMS.with(|items| {
                let items = items.borrow();
                let Some(it) = items.get(dis.itemData.wrapping_sub(1)) else { return false };
                let dc = dis.hDC;
                let rc = dis.rcItem;
                let (bg, hi, fg, dim, sep) = colors(p.dark);
                let s = |v: f32| (v * p.scale).round() as i32;

                let bgb = CreateSolidBrush(bg);
                FillRect(dc, &rc, bgb);
                let _ = DeleteObject(bgb.into());

                if it.sep {
                    let y = (rc.top + rc.bottom) / 2;
                    let sb = CreateSolidBrush(sep);
                    FillRect(dc, &RECT { left: rc.left + s(10.0), top: y, right: rc.right - s(10.0), bottom: y + 1.max(s(1.0)) }, sb);
                    let _ = DeleteObject(sb.into());
                    let _ = ExcludeClipRect(dc, rc.left, rc.top, rc.right, rc.bottom);
                    return true;
                }

                let selected = dis.itemState.0 & ODS_SELECTED.0 != 0;
                let disabled = it.disabled || dis.itemState.0 & ODS_DISABLED.0 != 0;
                if selected && !disabled {
                    let hb = CreateSolidBrush(hi);
                    let old_b = SelectObject(dc, hb.into());
                    let old_p = SelectObject(dc, GetStockObject(NULL_PEN));
                    let _ = RoundRect(dc, rc.left + s(4.0), rc.top + s(2.0), rc.right - s(4.0), rc.bottom - s(2.0), s(8.0), s(8.0));
                    SelectObject(dc, old_b);
                    SelectObject(dc, old_p);
                    let _ = DeleteObject(hb.into());
                }

                // Columna del ícono: el glifo del ítem, o la tilde si está marcado.
                let glyph = it.glyph.or(if it.checked { Some(I_CHECK) } else { None });
                let ix = rc.left + s(12.0);
                if let Some(b) = glyph.and_then(|g| p.icons.get(g)) {
                    blit(dc, *b, ix, (rc.top + rc.bottom - p.icon_px) / 2, p.icon_px);
                }

                let old_f = SelectObject(dc, p.font.into());
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, if disabled { dim } else { fg });
                let mut text: Vec<u16> = it.text.encode_utf16().collect();
                let mut tr = RECT { left: ix + p.icon_px + s(12.0), top: rc.top, right: rc.right - s(28.0), bottom: rc.bottom };
                DrawTextW(dc, &mut text, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_NOPREFIX);
                if !it.accel.is_empty() {
                    SetTextColor(dc, dim);
                    let mut acc: Vec<u16> = it.accel.encode_utf16().collect();
                    let mut ar = RECT { left: rc.left, top: rc.top, right: rc.right - s(14.0), bottom: rc.bottom };
                    DrawTextW(dc, &mut acc, &mut ar, DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX);
                }
                SelectObject(dc, old_f);

                if it.sub {
                    if let Some(b) = p.icons.get(I_CHEVRON) {
                        let px = p.icon_px * 3 / 4;
                        blit(dc, *b, rc.right - s(12.0) - px, (rc.top + rc.bottom - px) / 2, px);
                    }
                }
                // Que Windows no dibuje encima su propia flecha de submenú.
                let _ = ExcludeClipRect(dc, rc.left, rc.top, rc.right, rc.bottom);
                true
            })
        })
    }
}

/// Copia un bitmap con alfa premultiplicado al DC del menú.
unsafe fn blit(dc: HDC, bmp: HBITMAP, x: i32, y: i32, size: i32) {
    let mem = CreateCompatibleDC(Some(dc));
    let old = SelectObject(mem, bmp.into());
    let mut info = BITMAP::default();
    GetObjectW(bmp.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _));
    let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
    let _ = AlphaBlend(dc, x, y, size, size, mem, 0, 0, info.bmWidth, info.bmHeight, blend);
    SelectObject(mem, old);
    let _ = DeleteDC(mem);
}

/// Marco de los menús en el tema elegido (Windows 10 1903+/11, uxtheme por ordinal).
fn set_menu_theme(dark: bool) {
    unsafe {
        let Ok(ux) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        // 135 = SetPreferredAppMode(mode): 2 = siempre oscuro, 3 = siempre claro.
        if let Some(f) = GetProcAddress(ux, PCSTR(135 as *const u8)) {
            let set: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set(if dark { 2 } else { 3 });
        }
        // 136 = FlushMenuThemes.
        if let Some(f) = GetProcAddress(ux, PCSTR(136 as *const u8)) {
            let flush: extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

/// Dibuja glifos de Segoe Fluent Icons en bitmaps de 32 bits con alfa (premultiplicado).
struct IconPainter {
    factory: ID2D1Factory,
    wic: IWICImagingFactory,
    format: IDWriteTextFormat,
    size: u32,
    dark: bool,
}

impl IconPainter {
    fn new(dark: bool, size: u32) -> Option<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let mut col = None;
            let _ = dw.GetSystemFontCollection(&mut col, false);
            let mut idx = 0u32;
            let mut exists = windows::core::BOOL(0);
            if let Some(c) = &col {
                let _ = c.FindFamilyName(w!("Segoe Fluent Icons"), &mut idx, &mut exists);
            }
            let family = if exists.as_bool() { w!("Segoe Fluent Icons") } else { w!("Segoe MDL2 Assets") };
            let format = dw
                .CreateTextFormat(family, None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size as f32 * 0.85, w!(""))
                .ok()?;
            let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            Some(Self { factory, wic, format, size, dark })
        }
    }

    fn bitmap(&self, glyph: &str) -> Option<HBITMAP> {
        let n = self.size;
        unsafe {
            let wb = self.wic.CreateBitmap(n, n, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad).ok()?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = self.factory.CreateWicBitmapRenderTarget(&wb, &props).ok()?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let c = if self.dark { 0.95 } else { 0.12 };
            let brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: c, g: c, b: c, a: 1.0 }, None).ok()?;
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let text: Vec<u16> = glyph.encode_utf16().collect();
            let r = D2D_RECT_F { left: 0.0, top: 0.0, right: n as f32, bottom: n as f32 };
            rt.DrawText(&text, &self.format, &r, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            rt.EndDraw(None, None).ok()?;

            let mut px = vec![0u8; (n * n * 4) as usize];
            wb.cast::<IWICBitmapSource>().ok()?.CopyPixels(std::ptr::null(), n * 4, &mut px).ok()?;
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: n as i32,
                    biHeight: -(n as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let hb = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
            Some(hb)
        }
    }
}

/// Ícono embebido en el exe (res/esme.ico), al tamaño justo para la bandeja.
fn load_icon() -> HICON {
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        LoadImageW(Some(hinst.into()), PCWSTR(1 as *const u16), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
    }
}
