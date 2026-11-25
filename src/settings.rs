//! Ventana de Ajustes (Fase 8): una ventana normal de Windows dibujada con Direct2D.
//! Solo existe mientras está abierta; al cerrarla se liberan la ventana, la GPU y las fuentes,
//! así que en reposo no cuesta nada.
//!
//! Este módulo sabe dibujar y ubicar controles; qué filas hay y qué hace cada una vive en
//! `app/settings.rs`.

use crate::render::{font_exists, tf};
use crate::ui::R;
use windows::core::{w, Result, BOOL, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Mensaje propio: terminó la búsqueda de ciudad.
pub const WM_GEO_DONE: u32 = WM_APP + 30;
/// Mensaje propio: terminó la comprobación de la API key (lista de modelos).
pub const WM_KEY_DONE: u32 = WM_APP + 31;
/// Timer del cursor parpadeante del campo de texto.
pub const T_CARET: usize = 1;

pub const SIDEBAR_W: f32 = 212.0;
const CONTENT_PAD: f32 = 28.0;
const CONTENT_MAX_W: f32 = 640.0;

/// (glifo, nombre) de cada sección.
pub const SECTIONS: [(&str, &str); 6] = [
    ("\u{E713}", "General"),
    ("\u{E771}", "Apariencia"),
    ("\u{E787}", "Hoy"),
    ("\u{E945}", "Ask Esme"),
    ("\u{EA8F}", "Avisos"),
    ("\u{E946}", "Acerca de"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    City,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Autostart,
    Mover,
    Minimo,
    Transparente,
    Pet,
    Tamano,
    Tema,
    HoyPomo,
    HoyAgua,
    HoyClima,
    HoyCal,
    HoyTodo,
    AguaMeta,
    PomoFoco,
    PomoDescanso,
    Modelo,
    AvisoVolumen,
    AvisoAudio,
    AvisoTeclado,
    AvisoEnergia,
    AvisoBrillo,
    AvisoBateria,
    AvisoWifi,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    CitySearch,
    CityClear,
    IcalOpenGoogle,
    IcalPaste,
    IcalClear,
    KeyGet,
    KeyPaste,
    KeyClear,
    OpenRepo,
    OpenData,
}

/// Lo que pasa al hacer clic en algo.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    Section(usize),
    Toggle(Key),
    Choose(Key, u32),
    Step(Key, i32),
    Focus(Field),
    Do(Cmd),
}

pub enum Control {
    None,
    Toggle(bool, Key),
    /// Opciones, cuál está elegida (u32::MAX = ninguna).
    Choice(&'static [&'static str], u32, Key),
    /// Valor mostrado entre − y +.
    Stepper(String, Key),
    /// Texto fijo a la derecha (atajos de teclado).
    Keys(&'static str),
    /// Campo de texto de ancho completo con botones a su derecha.
    Field(Field, &'static str, Vec<(&'static str, Cmd)>),
    /// Fila de botones debajo del texto.
    Buttons(Vec<(&'static str, Cmd)>),
}

pub struct Row {
    pub title: String,
    /// Puede tener varias líneas ("\n").
    pub desc: String,
    pub control: Control,
}

pub struct Group {
    pub title: &'static str,
    pub rows: Vec<Row>,
}

pub fn row(title: impl Into<String>, desc: impl Into<String>, control: Control) -> Row {
    Row { title: title.into(), desc: desc.into(), control }
}

struct Gfx {
    _factory: ID2D1Factory,
    rt: ID2D1HwndRenderTarget,
    f_brand: IDWriteTextFormat,
    f_title: IDWriteTextFormat,
    f_group: IDWriteTextFormat,
    f_row: IDWriteTextFormat,
    f_desc: IDWriteTextFormat,
    f_ctrl: IDWriteTextFormat,
    f_ctrl_c: IDWriteTextFormat,
    f_icon: IDWriteTextFormat,
    f_icon_c: IDWriteTextFormat,
}

pub struct Settings {
    pub hwnd: HWND,
    pub section: usize,
    pub focus: Option<Field>,
    pub city_text: String,
    pub caret_on: bool,
    /// Mensajes de estado: 0 = ciudad, 1 = calendario, 2 = API key.
    pub status: [String; 3],
    pub geo: std::sync::Arc<std::sync::Mutex<Option<std::result::Result<(String, f64, f64), String>>>>,
    /// Resultado de comprobar la key: el modelo elegido o el error.
    pub key_check: std::sync::Arc<std::sync::Mutex<Option<std::result::Result<String, String>>>>,
    pub scroll: f32,
    pub hover: Option<Act>,
    hits: Vec<(R, Act)>,
    content_h: f32,
    scale: f32,
    gfx: Option<Gfx>,
}

type C = (f32, f32, f32);

/// Paleta de la ventana. El acento es el azul medianoche de Esme en los dos temas.
#[derive(Clone, Copy)]
struct Pal {
    bg: C,
    side: C,
    card: C,
    line: C,
    sel: C,
    accent: C,
    text: C,
    dim: C,
    ctrl: C,
    ctrl_hover: C,
    input: C,
}

const WHITE: C = (1.0, 1.0, 1.0);

fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            bg: (0.075, 0.075, 0.09),
            side: (0.10, 0.10, 0.12),
            card: (0.125, 0.125, 0.15),
            line: (0.20, 0.20, 0.24),
            sel: (0.17, 0.18, 0.24),
            accent: (0.27, 0.45, 1.0),
            text: (0.95, 0.95, 0.97),
            dim: (0.62, 0.63, 0.68),
            ctrl: (0.20, 0.20, 0.25),
            ctrl_hover: (0.26, 0.27, 0.33),
            input: (0.06, 0.06, 0.075),
        }
    } else {
        Pal {
            bg: (0.955, 0.957, 0.968),
            side: (0.918, 0.922, 0.94),
            card: (1.0, 1.0, 1.0),
            line: (0.88, 0.885, 0.91),
            sel: (0.85, 0.885, 1.0),
            accent: (0.22, 0.40, 0.96),
            text: (0.10, 0.10, 0.13),
            dim: (0.40, 0.41, 0.46),
            ctrl: (0.905, 0.91, 0.935),
            ctrl_hover: (0.85, 0.86, 0.895),
            input: (1.0, 1.0, 1.0),
        }
    }
}

/// Barra de título oscura o clara (Windows 10 20H1+ y 11).
pub fn set_titlebar(hwnd: HWND, dark: bool) {
    let v = BOOL(dark as i32);
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const _ as _, 4);
    }
}

impl Settings {
    /// Crea la ventana (centrada en el monitor principal) y la muestra.
    pub fn open(wndproc: WNDPROC, dark: bool) -> Option<Self> {
        unsafe {
            let hinst = GetModuleHandleW(None).ok()?;
            let class = w!("EsmeAjustes");
            let icon = LoadImageW(Some(hinst.into()), PCWSTR(1 as *const u16), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE)
                .map(|h| HICON(h.0))
                .unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: wndproc,
                hInstance: hinst.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
                hIcon: icon,
                hIconSm: icon,
                ..Default::default()
            };
            // Si ya estaba registrada (segunda apertura), falla sin problema.
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("Ajustes de Esme"),
                WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                100,
                100,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .ok()?;
            set_titlebar(hwnd, dark);
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            let (w, h) = ((780.0 * scale) as i32, (580.0 * scale) as i32);
            let mut wa = RECT::default();
            let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
            let x = wa.left + ((wa.right - wa.left) - w) / 2;
            let y = wa.top + ((wa.bottom - wa.top) - h) / 2;
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
            let s = Settings {
                hwnd,
                section: 0,
                focus: None,
                city_text: String::new(),
                caret_on: false,
                status: Default::default(),
                geo: Default::default(),
                key_check: Default::default(),
                scroll: 0.0,
                hover: None,
                hits: Vec::new(),
                content_h: 0.0,
                scale,
                gfx: None,
            };
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            Some(s)
        }
    }

    fn ensure_gfx(&mut self) -> Result<()> {
        if self.gfx.is_some() {
            return Ok(());
        }
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let mut rc = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut rc);
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                dpiX: 96.0 * self.scale,
                dpiY: 96.0 * self.scale,
                ..Default::default()
            };
            let hprops = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U { width: rc.right.max(1) as u32, height: rc.bottom.max(1) as u32 },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let rt = factory.CreateHwndRenderTarget(&props, &hprops)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let ui_font = if font_exists(&dw, "Segoe UI Variable Text") { "Segoe UI Variable Text" } else { "Segoe UI" };
            let icons = if font_exists(&dw, "Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
            let lead = DWRITE_TEXT_ALIGNMENT_LEADING;
            let center = DWRITE_TEXT_ALIGNMENT_CENTER;
            let f_desc = tf(&dw, ui_font, 12.5, DWRITE_FONT_WEIGHT_NORMAL, lead)?;
            f_desc.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            self.gfx = Some(Gfx {
                f_brand: tf(&dw, ui_font, 17.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, lead)?,
                f_title: tf(&dw, ui_font, 24.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, lead)?,
                f_group: tf(&dw, ui_font, 13.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, lead)?,
                f_row: tf(&dw, ui_font, 14.0, DWRITE_FONT_WEIGHT_NORMAL, lead)?,
                f_desc,
                f_ctrl: tf(&dw, ui_font, 13.0, DWRITE_FONT_WEIGHT_NORMAL, lead)?,
                f_ctrl_c: tf(&dw, ui_font, 13.0, DWRITE_FONT_WEIGHT_NORMAL, center)?,
                f_icon: tf(&dw, icons, 16.0, DWRITE_FONT_WEIGHT_NORMAL, lead)?,
                f_icon_c: tf(&dw, icons, 12.0, DWRITE_FONT_WEIGHT_NORMAL, center)?,
                _factory: factory,
                rt,
            });
        }
        Ok(())
    }

    pub fn on_size(&mut self) {
        if let Some(g) = &self.gfx {
            let mut rc = RECT::default();
            unsafe {
                let _ = GetClientRect(self.hwnd, &mut rc);
                let _ = g.rt.Resize(&D2D_SIZE_U { width: rc.right.max(1) as u32, height: rc.bottom.max(1) as u32 });
            }
        }
        self.clamp_scroll();
    }

    pub fn on_dpi(&mut self, dpi: u32, suggested: &RECT) {
        self.scale = dpi.max(96) as f32 / 96.0;
        self.gfx = None; // se recrea con la DPI nueva
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                suggested.left,
                suggested.top,
                suggested.right - suggested.left,
                suggested.bottom - suggested.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// Tamaño del área cliente en DIPs.
    fn client_dips(&self) -> (f32, f32) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        (rc.right as f32 / self.scale, rc.bottom as f32 / self.scale)
    }

    pub fn hit(&self, x_px: i32, y_px: i32) -> Option<Act> {
        let (x, y) = (x_px as f32 / self.scale, y_px as f32 / self.scale);
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, a)| *a)
    }

    pub fn wheel(&mut self, delta: i16) {
        self.scroll -= delta as f32 / 120.0 * 48.0;
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let (_, h) = self.client_dips();
        let max = (self.content_h - h + 24.0).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    pub fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// Dibuja todo. `groups` es el contenido de la sección actual.
    pub fn paint(&mut self, groups: &[Group], dark: bool) {
        if self.ensure_gfx().is_err() {
            return;
        }
        let (w, h) = self.client_dips();
        let mut hits: Vec<(R, Act)> = Vec::new();
        let hover = self.hover;
        let section = self.section;
        let focus = self.focus;
        let caret = self.caret_on;
        let city = self.city_text.clone();
        let scroll = self.scroll;
        let g = self.gfx.as_ref().unwrap();
        let p = Painter { g, hover, pal: pal(dark) };
        let pl = p.pal;
        let content_h;
        unsafe {
            g.rt.BeginDraw();
            g.rt.Clear(Some(&rgba(pl.bg, 1.0)));

            // ---------- Contenido (con scroll) ----------
            let x0 = SIDEBAR_W + CONTENT_PAD;
            let x1 = (w - CONTENT_PAD).min(x0 + CONTENT_MAX_W).max(x0 + 300.0);
            let mut y = 26.0 - scroll;
            p.text(SECTIONS[section].1, &g.f_title, R::new(x0, y, x1, y + 34.0), pl.text, 1.0);
            y += 50.0;
            for grp in groups {
                if !grp.title.is_empty() {
                    p.text(grp.title, &g.f_group, R::new(x0 + 2.0, y, x1, y + 22.0), pl.dim, 1.0);
                    y += 26.0;
                }
                let card_top = y;
                let heights: Vec<f32> = grp.rows.iter().map(row_height).collect();
                let card_h: f32 = heights.iter().sum();
                p.round(R::new(x0, card_top, x1, card_top + card_h), 10.0, pl.card, 1.0);
                for (i, (r, rh)) in grp.rows.iter().zip(&heights).enumerate() {
                    if i > 0 {
                        p.fill(R::new(x0 + 16.0, y, x1 - 16.0, y + 1.0), pl.line, 1.0);
                    }
                    p.draw_row(r, R::new(x0, y, x1, y + rh), &mut hits, focus, caret, &city);
                    y += rh;
                }
                y += 22.0;
            }
            content_h = y + scroll;

            // ---------- Barra lateral (encima del contenido que scrollea) ----------
            p.fill(R::new(0.0, 0.0, SIDEBAR_W, h), pl.side, 1.0);
            p.text("Esme", &g.f_brand, R::new(24.0, 20.0, SIDEBAR_W, 44.0), pl.text, 1.0);
            p.text(concat!("Versión ", env!("CARGO_PKG_VERSION")), &g.f_desc, R::new(24.0, 46.0, SIDEBAR_W, 64.0), pl.dim, 1.0);
            let mut sy = 84.0;
            for (i, (glyph, name)) in SECTIONS.iter().enumerate() {
                let r = R::new(12.0, sy, SIDEBAR_W - 12.0, sy + 38.0);
                let sel = i == section;
                if sel || hover == Some(Act::Section(i)) {
                    p.round(r, 8.0, if sel { pl.sel } else { pl.ctrl }, if sel { 1.0 } else { 0.6 });
                }
                if sel {
                    p.round(R::new(r.l, r.t + 10.0, r.l + 3.0, r.b - 10.0), 1.5, pl.accent, 1.0);
                }
                p.text(glyph, &g.f_icon, R::new(r.l + 14.0, r.t, r.l + 40.0, r.b), if sel { pl.accent } else { pl.dim }, 1.0);
                p.text(name, &g.f_row, R::new(r.l + 44.0, r.t, r.r - 6.0, r.b), pl.text, if sel { 1.0 } else { 0.85 });
                hits.push((r, Act::Section(i)));
                sy += 42.0;
            }
            p.text("Los cambios se guardan solos", &g.f_desc, R::new(24.0, h - 34.0, SIDEBAR_W, h - 12.0), pl.dim, 0.8);

            let _ = g.rt.EndDraw(None, None);
        }
        self.hits = hits;
        self.content_h = content_h;
    }
}

fn row_height(r: &Row) -> f32 {
    let lines = if r.desc.is_empty() { 0 } else { r.desc.lines().count() };
    let base = 22.0 + 20.0 + lines as f32 * 18.0;
    match r.control {
        Control::Field(..) => base + 46.0,
        Control::Buttons(..) => base + 44.0,
        _ if r.desc.is_empty() => 50.0,
        _ => base.max(62.0),
    }
}

struct Painter<'a> {
    g: &'a Gfx,
    hover: Option<Act>,
    pal: Pal,
}

impl Painter<'_> {
    fn brush(&self, c: (f32, f32, f32), a: f32) -> Option<ID2D1SolidColorBrush> {
        unsafe { self.g.rt.CreateSolidColorBrush(&rgba(c, a), None).ok() }
    }

    fn fill(&self, r: R, c: (f32, f32, f32), a: f32) {
        if let Some(b) = self.brush(c, a) {
            unsafe { self.g.rt.FillRectangle(&d2r(r), &b) };
        }
    }

    fn round(&self, r: R, rad: f32, c: (f32, f32, f32), a: f32) {
        if let Some(b) = self.brush(c, a) {
            let rr = D2D1_ROUNDED_RECT { rect: d2r(r), radiusX: rad, radiusY: rad };
            unsafe { self.g.rt.FillRoundedRectangle(&rr, &b) };
        }
    }

    fn outline(&self, r: R, rad: f32, c: (f32, f32, f32), a: f32, width: f32) {
        if let Some(b) = self.brush(c, a) {
            let rr = D2D1_ROUNDED_RECT { rect: d2r(r), radiusX: rad, radiusY: rad };
            unsafe { self.g.rt.DrawRoundedRectangle(&rr, &b, width, None) };
        }
    }

    fn text(&self, s: &str, f: &IDWriteTextFormat, r: R, c: (f32, f32, f32), a: f32) {
        if let Some(b) = self.brush(c, a) {
            let wide: Vec<u16> = s.encode_utf16().collect();
            unsafe {
                self.g.rt.DrawText(&wide, f, &d2r(r), &b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            }
        }
    }

    fn hovered(&self, a: Act) -> bool {
        self.hover == Some(a)
    }

    /// Botón tipo píldora; devuelve su rectángulo.
    fn pill_button(&self, label: &str, r: R, act: Act, primary: bool, hits: &mut Vec<(R, Act)>) {
        let hv = self.hovered(act);
        let c = if primary { self.pal.accent } else if hv { self.pal.ctrl_hover } else { self.pal.ctrl };
        self.round(r, r.h() / 2.0, c, if primary && hv { 0.85 } else { 1.0 });
        self.text(label, &self.g.f_ctrl_c, r, if primary { WHITE } else { self.pal.text }, 1.0);
        hits.push((r, act));
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_row(&self, row: &Row, r: R, hits: &mut Vec<(R, Act)>, focus: Option<Field>, caret: bool, city: &str) {
        let g = self.g;
        let tx = r.l + 18.0;
        let ctrl_w = match &row.control {
            Control::Toggle(..) => 60.0,
            Control::Choice(opts, ..) => opts.iter().map(|o| seg_w(o)).sum::<f32>() + 8.0,
            Control::Stepper(..) => 140.0,
            Control::Keys(k) => k.chars().count() as f32 * 7.6 + 24.0,
            _ => 0.0,
        };
        let text_r = r.r - 18.0 - ctrl_w - 12.0;
        let compact = row.desc.is_empty() && !matches!(row.control, Control::Field(..) | Control::Buttons(..));
        let title_r = if compact { R::new(tx, r.t, text_r, r.b) } else { R::new(tx, r.t + 12.0, text_r, r.t + 34.0) };
        self.text(&row.title, &g.f_row, title_r, self.pal.text, 1.0);
        let mut dy = r.t + 34.0;
        for line in row.desc.lines() {
            self.text(line, &g.f_desc, R::new(tx, dy, text_r.max(r.r - 18.0 - ctrl_w), dy + 18.0), self.pal.dim, 1.0);
            dy += 18.0;
        }
        let cy = if compact { (r.t + r.b) / 2.0 } else { r.t + 31.0 };
        match &row.control {
            Control::None => {}
            Control::Toggle(on, key) => {
                let t = R::new(r.r - 18.0 - 44.0, cy - 11.0, r.r - 18.0, cy + 11.0);
                let act = Act::Toggle(*key);
                let hv = self.hovered(act);
                if *on {
                    self.round(t, 11.0, self.pal.accent, if hv { 0.85 } else { 1.0 });
                } else {
                    self.round(t, 11.0, if hv { self.pal.ctrl_hover } else { self.pal.ctrl }, 1.0);
                    self.outline(t, 11.0, self.pal.dim, 0.6, 1.0);
                }
                let kx = if *on { t.r - 11.0 } else { t.l + 11.0 };
                let knob = R::new(kx - 7.0, cy - 7.0, kx + 7.0, cy + 7.0);
                self.round(knob, 7.0, if *on { WHITE } else { self.pal.dim }, 1.0);
                hits.push((R::new(t.l - 6.0, r.t, r.r, r.b), act));
            }
            Control::Choice(opts, sel, key) => {
                let total: f32 = opts.iter().map(|o| seg_w(o)).sum::<f32>() + 8.0;
                let c = R::new(r.r - 18.0 - total, cy - 16.0, r.r - 18.0, cy + 16.0);
                self.round(c, 9.0, self.pal.ctrl, 1.0);
                let mut x = c.l + 4.0;
                for (i, o) in opts.iter().enumerate() {
                    let sw = seg_w(o);
                    let s = R::new(x, c.t + 4.0, x + sw, c.b - 4.0);
                    let act = Act::Choose(*key, i as u32);
                    if i as u32 == *sel {
                        self.round(s, 6.0, self.pal.accent, 1.0);
                    } else if self.hovered(act) {
                        self.round(s, 6.0, self.pal.ctrl_hover, 1.0);
                    }
                    let on = i as u32 == *sel;
                    self.text(o, &g.f_ctrl_c, s, if on { WHITE } else { self.pal.text }, if on { 1.0 } else { 0.8 });
                    hits.push((s, act));
                    x += sw;
                }
            }
            Control::Stepper(value, key) => {
                let plus = R::new(r.r - 18.0 - 30.0, cy - 15.0, r.r - 18.0, cy + 15.0);
                let minus = R::new(plus.l - 110.0, cy - 15.0, plus.l - 80.0, cy + 15.0);
                for (b, glyph, d) in [(minus, "\u{E738}", -1), (plus, "\u{E710}", 1)] {
                    let act = Act::Step(*key, d);
                    self.round(b, 15.0, if self.hovered(act) { self.pal.ctrl_hover } else { self.pal.ctrl }, 1.0);
                    self.text(glyph, &g.f_icon_c, b, self.pal.text, 1.0);
                    hits.push((b, act));
                }
                self.text(value, &g.f_ctrl_c, R::new(minus.r, cy - 12.0, plus.l, cy + 12.0), self.pal.text, 1.0);
            }
            Control::Keys(k) => {
                let kw = k.chars().count() as f32 * 7.6 + 24.0;
                let b = R::new(r.r - 18.0 - kw, cy - 14.0, r.r - 18.0, cy + 14.0);
                self.round(b, 7.0, self.pal.ctrl, 1.0);
                self.text(k, &g.f_ctrl_c, b, self.pal.text, 0.9);
            }
            Control::Field(field, placeholder, buttons) => {
                let by = r.b - 46.0;
                let buttons_w: f32 = buttons.iter().map(|(l, _)| btn_w(l) + 8.0).sum();
                let input = R::new(tx, by, r.r - 18.0 - buttons_w, by + 34.0);
                let focused = focus == Some(*field);
                self.round(input, 8.0, self.pal.input, 1.0);
                self.outline(input, 8.0, if focused { self.pal.accent } else { self.pal.line }, 1.0, if focused { 1.5 } else { 1.0 });
                let txt_r = R::new(input.l + 12.0, input.t, input.r - 10.0, input.b);
                if city.is_empty() {
                    self.text(placeholder, &g.f_ctrl, txt_r, self.pal.dim, 0.8);
                } else {
                    self.text(city, &g.f_ctrl, txt_r, self.pal.text, 1.0);
                }
                if focused && caret {
                    let cx = txt_r.l + text_width(g, city).min(txt_r.w() - 2.0) + 1.0;
                    self.fill(R::new(cx, input.t + 8.0, cx + 1.5, input.b - 8.0), self.pal.text, 1.0);
                }
                hits.push((input, Act::Focus(*field)));
                let mut x = input.r + 8.0;
                for (i, (label, cmd)) in buttons.iter().enumerate() {
                    let bw = btn_w(label);
                    self.pill_button(label, R::new(x, by + 1.0, x + bw, by + 33.0), Act::Do(*cmd), i == 0, hits);
                    x += bw + 8.0;
                }
            }
            Control::Buttons(buttons) => {
                let by = r.b - 46.0;
                let mut x = tx;
                for (i, (label, cmd)) in buttons.iter().enumerate() {
                    let bw = btn_w(label);
                    self.pill_button(label, R::new(x, by, x + bw, by + 32.0), Act::Do(*cmd), i == 0, hits);
                    x += bw + 8.0;
                }
            }
        }
    }
}

fn seg_w(label: &str) -> f32 {
    label.chars().count() as f32 * 7.4 + 26.0
}

fn btn_w(label: &str) -> f32 {
    label.chars().count() as f32 * 7.2 + 32.0
}

/// Ancho real del texto con la fuente de los controles (para ubicar el cursor).
fn text_width(g: &Gfx, s: &str) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    unsafe {
        let Ok(dw) = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) else { return 0.0 };
        let wide: Vec<u16> = s.encode_utf16().collect();
        let Ok(layout) = dw.CreateTextLayout(&wide, &g.f_ctrl, 4000.0, 40.0) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        if layout.GetMetrics(&mut m).is_ok() {
            m.widthIncludingTrailingWhitespace
        } else {
            0.0
        }
    }
}

fn rgba(c: (f32, f32, f32), a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0, g: c.1, b: c.2, a }
}

fn d2r(r: R) -> D2D_RECT_F {
    D2D_RECT_F { left: r.l, top: r.t, right: r.r, bottom: r.b }
}
