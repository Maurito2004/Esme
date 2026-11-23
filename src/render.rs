//! Renderizado por GPU: D3D11 -> DXGI swap chain de composición -> DirectComposition,
//! dibujado con Direct2D + DirectWrite. Transparencia por píxel (alpha premultiplicado).

use crate::anim::clamp01;
use crate::config::Pet;
use crate::mascot::{Eyes, Mood};
use crate::ui::{self, Btn, Layout, R};
use windows::core::{w, Interface, Result, BOOL, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows_numerics::{Matrix3x2, Vector2};

pub enum Collapsed<'a> {
    Clock(&'a str),
    Minimal,
    Mascot { time: &'a str, eyes: Eyes, pet: Pet },
    Media { time: &'a str },
    Lyric(&'a str),
}

pub enum LyricsView<'a> {
    Message(&'a str),
    Synced { lines: &'a [(f64, String)], scroll: f32, current: usize },
    Plain { lines: &'a [String], scroll: f32 },
}

pub struct MediaView<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub playing: bool,
    pub can_prev: bool,
    pub can_next: bool,
    /// None si la app no informa duración.
    pub progress: Option<f32>,
    pub pos: String,
    pub dur: String,
    pub lyrics: Option<LyricsView<'a>>,
}

pub struct ShelfView<'a> {
    /// (glifo, nombre visible) de cada archivo.
    pub tiles: &'a [(&'static str, String)],
    /// Hay archivos arrastrándose encima: resaltamos la zona.
    pub drop_hover: bool,
}

pub struct ClipView<'a> {
    pub items: &'a [String],
    /// Fila recién copiada (muestra "✓ Copiado").
    pub copied: Option<usize>,
}

pub enum Expanded<'a> {
    Clock { time: &'a str, date: &'a str },
    Media(MediaView<'a>),
}

pub struct Frame<'a> {
    pub cx: f32,
    /// Forma negra (DIPs).
    pub shape_w: f32,
    pub shape_h: f32,
    pub radius: f32,
    pub ear: f32,
    /// 0 = contraído, 1 = expandido (puede pasarse un poco por el rebote).
    pub expand: f32,
    /// Modo transparente: la píldora queda chica y el contenido flota debajo.
    pub transparent: bool,
    pub collapsed: Collapsed<'a>,
    pub expanded: Expanded<'a>,
    pub layout: Layout,
    pub hover: Option<Btn>,
    /// 0..1: cuánto se ve la mascota (desplaza el reloj).
    pub mascot_vis: f32,
    /// Panel del portapapeles abierto.
    pub clip: Option<ClipView<'a>>,
    /// Estante de archivos abierto.
    pub shelf: Option<ShelfView<'a>>,
}

struct Fonts {
    small: IDWriteTextFormat,
    big: IDWriteTextFormat,
    date: IDWriteTextFormat,
    title: IDWriteTextFormat,
    artist: IDWriteTextFormat,
    time: IDWriteTextFormat,
    icon: IDWriteTextFormat,
    icon_big: IDWriteTextFormat,
    lyric: IDWriteTextFormat,
    lyric_cur: IDWriteTextFormat,
    pill_lyric: IDWriteTextFormat,
    tiny: IDWriteTextFormat,
    tile: IDWriteTextFormat,
}

pub struct Renderer {
    swap: IDXGISwapChain1,
    d2d_factory: ID2D1Factory1,
    ctx: ID2D1DeviceContext,
    _dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    fonts: Fonts,
    wic: Option<IWICImagingFactory>,
    art: Option<ID2D1Bitmap1>,
    art_bytes: Option<std::sync::Arc<Vec<u8>>>,
    width_px: u32,
    height_px: u32,
    scale: f32,
}

// Glifos de Segoe Fluent Icons / Segoe MDL2 Assets.
const G_PREV: &str = "\u{E892}";
const G_NEXT: &str = "\u{E893}";
const G_PLAY: &str = "\u{E768}";
const G_PAUSE: &str = "\u{E769}";
const G_LYRICS: &str = "\u{E8E4}";
const G_MUSIC: &str = "\u{E8D6}";
const G_CLIP: &str = "\u{E77F}";
const G_SHELF: &str = "\u{E8B7}";

impl Renderer {
    pub fn new(hwnd: HWND, width_px: u32, height_px: u32, scale: f32) -> Result<Self> {
        unsafe {
            let mut d3d: Option<ID3D11Device> = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let d3d = d3d.unwrap();
            let dxgi_device: IDXGIDevice = d3d.cast()?;
            if let Ok(d1) = dxgi_device.cast::<IDXGIDevice1>() {
                let _ = d1.SetMaximumFrameLatency(1);
            }
            let factory: IDXGIFactory2 = dxgi_device.GetAdapter()?.GetParent()?;
            let swap = factory.CreateSwapChainForComposition(&d3d, &swap_desc(width_px, height_px), None)?;

            let opts = D2D1_FACTORY_OPTIONS { debugLevel: D2D1_DEBUG_LEVEL_NONE };
            let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&opts))?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
            let ctx = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            ctx.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let icons = if font_exists(&dw, "Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
            let ui_font = "Segoe UI Variable Display";
            let c = DWRITE_TEXT_ALIGNMENT_CENTER;
            let l = DWRITE_TEXT_ALIGNMENT_LEADING;
            let fonts = Fonts {
                small: tf(&dw, ui_font, 13.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, c)?,
                big: tf(&dw, ui_font, 42.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, c)?,
                date: tf(&dw, ui_font, 14.0, DWRITE_FONT_WEIGHT_NORMAL, c)?,
                title: tf(&dw, ui_font, 15.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, l)?,
                artist: tf(&dw, ui_font, 13.0, DWRITE_FONT_WEIGHT_NORMAL, l)?,
                time: tf(&dw, ui_font, 11.0, DWRITE_FONT_WEIGHT_NORMAL, c)?,
                icon: tf(&dw, icons, 15.0, DWRITE_FONT_WEIGHT_NORMAL, c)?,
                icon_big: tf(&dw, icons, 20.0, DWRITE_FONT_WEIGHT_NORMAL, c)?,
                lyric: tf(&dw, ui_font, 14.0, DWRITE_FONT_WEIGHT_NORMAL, c)?,
                lyric_cur: tf(&dw, ui_font, 16.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, c)?,
                pill_lyric: tf(&dw, ui_font, 13.5, DWRITE_FONT_WEIGHT_SEMI_BOLD, c)?,
                tiny: tf(&dw, ui_font, 9.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, c)?,
                tile: tf(&dw, ui_font, 11.0, DWRITE_FONT_WEIGHT_NORMAL, c)?,
            };
            for f in [&fonts.title, &fonts.artist, &fonts.lyric, &fonts.lyric_cur, &fonts.pill_lyric, &fonts.tile] {
                let sign = dw.CreateEllipsisTrimmingSign(f)?;
                let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                f.SetTrimming(&trim, &sign)?;
            }

            let wic: Option<IWICImagingFactory> = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok();

            let mut r = Self {
                swap,
                d2d_factory,
                ctx,
                _dcomp: dcomp,
                _target: target,
                _visual: visual,
                fonts,
                wic,
                art: None,
                art_bytes: None,
                width_px,
                height_px,
                scale,
            };
            r.bind_target()?;
            Ok(r)
        }
    }

    fn bind_target(&mut self) -> Result<()> {
        unsafe {
            let surface: IDXGISurface = self.swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            let bmp = self.ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            self.ctx.SetTarget(&bmp);
            self.ctx.SetDpi(96.0 * self.scale, 96.0 * self.scale);
            Ok(())
        }
    }

    pub fn resize(&mut self, width_px: u32, height_px: u32, scale: f32) -> Result<()> {
        if width_px == self.width_px && height_px == self.height_px && scale == self.scale {
            return Ok(());
        }
        unsafe {
            self.ctx.SetTarget(None);
            self.swap.ResizeBuffers(0, width_px, height_px, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
        }
        self.width_px = width_px;
        self.height_px = height_px;
        self.scale = scale;
        self.bind_target()
    }

    /// Carátula: se decodifica una sola vez por canción (WIC) y se reduce a 160 px.
    pub fn set_art(&mut self, bytes: Option<std::sync::Arc<Vec<u8>>>) {
        let same = match (&bytes, &self.art_bytes) {
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.art = bytes.as_ref().and_then(|b| self.decode(b).ok());
        self.art_bytes = bytes;
    }

    fn decode(&self, bytes: &[u8]) -> Result<ID2D1Bitmap1> {
        let wic = self.wic.as_ref().ok_or_else(windows::core::Error::empty)?;
        unsafe {
            let stream = wic.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
            let frame = dec.GetFrame(0)?;
            let (mut w, mut h) = (0u32, 0u32);
            frame.GetSize(&mut w, &mut h)?;
            let k = (160.0 / w.max(h).max(1) as f32).min(1.0);
            let scaler = wic.CreateBitmapScaler()?;
            scaler.Initialize(&frame, ((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1), WICBitmapInterpolationModeFant)?;
            let conv = wic.CreateFormatConverter()?;
            conv.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
            self.ctx.CreateBitmapFromWicBitmap(&conv, None)
        }
    }

    /// Dibuja y presenta un frame. Con `vsync` bloquea hasta el próximo refresco
    /// (eso marca el ritmo de las animaciones, sin timers).
    pub fn draw(&mut self, f: &Frame, vsync: bool) -> Result<()> {
        unsafe {
            self.ctx.BeginDraw();
            self.ctx.SetTransform(&Matrix3x2::identity());
            self.ctx.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));

            let geo = self.notch_geometry(f.cx, f.shape_w, f.shape_h, f.radius, f.ear)?;
            let black = self.brush(0.0, 0.0, 0.0, 1.0)?;
            self.ctx.FillGeometry(&geo, &black, None);

            // Contenido contraído: en modo negro se desvanece al expandir; en transparente queda.
            let a_small = if f.transparent { 1.0 } else { clamp01(1.0 - f.expand * 2.5) };
            if a_small > 0.01 {
                self.draw_collapsed(f, a_small)?;
            }
            // Contenido expandido: aparece en la segunda mitad de la animación.
            let a_big = clamp01((f.expand - 0.45) / 0.55);
            if a_big > 0.01 {
                self.draw_expanded(f, a_big)?;
            }

            self.ctx.EndDraw(None, None)?;
            self.swap.Present(if vsync { 1 } else { 0 }, DXGI_PRESENT(0)).ok()?;
            Ok(())
        }
    }

    fn draw_collapsed(&self, f: &Frame, a: f32) -> Result<()> {
        let h = f.shape_h;
        let left = f.cx - f.shape_w / 2.0;
        let right = f.cx + f.shape_w / 2.0;
        let white = self.brush(1.0, 1.0, 1.0, 0.92 * a)?;
        match &f.collapsed {
            Collapsed::Minimal => {}
            Collapsed::Clock(t) => self.text(t, &self.fonts.small, R::new(left, 0.0, right, h), &white),
            Collapsed::Mascot { time, eyes, pet } => {
                let v = f.mascot_vis;
                let shift = 26.0 * v;
                self.text(time, &self.fonts.small, R::new(left - shift, 0.0, right - shift, h), &white);
                if v > 0.01 {
                    self.draw_pet(right - 40.0, h / 2.0, *eyes, *pet, a * v)?;
                }
            }
            Collapsed::Media { time } => {
                self.text(time, &self.fonts.small, R::new(left, 0.0, right, h), &white);
                let art = R::new(left + 12.0, (h - 20.0) / 2.0, left + 32.0, (h + 20.0) / 2.0);
                if self.art.is_some() {
                    self.draw_art(art, 5.0, a)?;
                } else {
                    self.label(G_MUSIC, &self.fonts.icon, art, (1.0, 1.0, 1.0, 0.8 * a), false)?;
                }
                let accent = self.brush(0.55, 0.85, 0.6, 0.95 * a)?;
                for (i, bh) in [8.0f32, 14.0, 10.0, 5.0].iter().enumerate() {
                    let x = right - 34.0 + i as f32 * 5.0;
                    let rr = D2D1_ROUNDED_RECT { rect: rect(x, h / 2.0 - bh / 2.0, x + 3.0, h / 2.0 + bh / 2.0), radiusX: 1.5, radiusY: 1.5 };
                    unsafe { self.ctx.FillRoundedRectangle(&rr, &accent) };
                }
            }
            Collapsed::Lyric(line) => {
                let r = R::new(left + 16.0, 0.0, right - 16.0, h);
                self.text(line, &self.fonts.pill_lyric, r, &white);
            }
        }
        Ok(())
    }

    fn draw_expanded(&self, f: &Frame, a: f32) -> Result<()> {
        let rise = (1.0 - a) * 8.0;
        unsafe {
            self.ctx.SetTransform(&Matrix3x2::translation(0.0, -rise));
        }
        let sh = f.transparent;
        let l = &f.layout;
        match &f.expanded {
            Expanded::Clock { time, date } => {
                self.label(time, &self.fonts.big, l.clock, (1.0, 1.0, 1.0, a), sh)?;
                self.label(date, &self.fonts.date, l.date, (1.0, 1.0, 1.0, 0.65 * a), sh)?;
            }
            Expanded::Media(m) => {
                if self.art.is_some() {
                    if sh {
                        let shadow = self.brush(0.0, 0.0, 0.0, 0.35 * a)?;
                        let rr = D2D1_ROUNDED_RECT { rect: d2r(l.art.inflate(1.5)), radiusX: 11.0, radiusY: 11.0 };
                        unsafe { self.ctx.FillRoundedRectangle(&rr, &shadow) };
                    }
                    self.draw_art(l.art, 10.0, a)?;
                } else {
                    let bg = self.brush(1.0, 1.0, 1.0, 0.12 * a)?;
                    let rr = D2D1_ROUNDED_RECT { rect: d2r(l.art), radiusX: 10.0, radiusY: 10.0 };
                    unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
                    self.label(G_MUSIC, &self.fonts.icon_big, l.art, (1.0, 1.0, 1.0, 0.6 * a), false)?;
                }
                let title = if m.title.is_empty() { "Reproduciendo" } else { m.title };
                self.label(title, &self.fonts.title, l.title, (1.0, 1.0, 1.0, a), sh)?;
                self.label(m.artist, &self.fonts.artist, l.artist, (1.0, 1.0, 1.0, 0.62 * a), sh)?;

                let hv = |b: Btn| f.hover == Some(b);
                self.button(G_PREV, &self.fonts.icon, l.prev, a * if m.can_prev { 1.0 } else { 0.35 }, hv(Btn::Prev), sh)?;
                self.button(if m.playing { G_PAUSE } else { G_PLAY }, &self.fonts.icon_big, l.play, a, hv(Btn::Play), sh)?;
                self.button(G_NEXT, &self.fonts.icon, l.next, a * if m.can_next { 1.0 } else { 0.35 }, hv(Btn::Next), sh)?;
                let on = m.lyrics.is_some();
                self.button(G_LYRICS, &self.fonts.icon, l.lyrics_btn, a * if on { 1.0 } else { 0.7 }, hv(Btn::Lyrics) || on, sh)?;

                if let Some(p) = m.progress {
                    self.label(&m.pos, &self.fonts.time, l.t_left, (1.0, 1.0, 1.0, 0.6 * a), sh)?;
                    self.label(&m.dur, &self.fonts.time, l.t_right, (1.0, 1.0, 1.0, 0.6 * a), sh)?;
                    let t = l.track;
                    if sh {
                        let s = self.brush(0.0, 0.0, 0.0, 0.4 * a)?;
                        let rr = D2D1_ROUNDED_RECT { rect: d2r(t.inflate(1.0)), radiusX: 3.0, radiusY: 3.0 };
                        unsafe { self.ctx.FillRoundedRectangle(&rr, &s) };
                    }
                    let bg = self.brush(1.0, 1.0, 1.0, 0.25 * a)?;
                    let fg = self.brush(1.0, 1.0, 1.0, 0.92 * a)?;
                    let full = D2D1_ROUNDED_RECT { rect: d2r(t), radiusX: 2.0, radiusY: 2.0 };
                    let x = t.l + (t.r - t.l) * p.clamp(0.0, 1.0);
                    let done = D2D1_ROUNDED_RECT { rect: rect(t.l, t.t, x.max(t.l + 4.0), t.b), radiusX: 2.0, radiusY: 2.0 };
                    unsafe {
                        self.ctx.FillRoundedRectangle(&full, &bg);
                        self.ctx.FillRoundedRectangle(&done, &fg);
                        if hv(Btn::Progress) {
                            let e = D2D1_ELLIPSE { point: pt(x, t.cy()), radiusX: 5.5, radiusY: 5.5 };
                            self.ctx.FillEllipse(&e, &fg);
                        }
                    }
                }
                if let Some(ly) = &m.lyrics {
                    self.draw_lyrics(ly, l.lyrics, a, sh, f.hover)?;
                }
            }
        }
        let on = f.clip.is_some();
        self.button(G_CLIP, &self.fonts.icon, l.clip_btn, a * if on { 1.0 } else { 0.7 }, f.hover == Some(Btn::Clip) || on, sh)?;
        if let Some(cv) = &f.clip {
            self.draw_clip(cv, l.clip, a, sh, f.hover)?;
        }
        let on = f.shelf.is_some();
        self.button(G_SHELF, &self.fonts.icon, l.shelf_btn, a * if on { 1.0 } else { 0.7 }, f.hover == Some(Btn::Shelf) || on, sh)?;
        if let Some(sv) = &f.shelf {
            self.draw_shelf(sv, l, a, sh, f.hover)?;
        }
        unsafe {
            self.ctx.SetTransform(&Matrix3x2::identity());
        }
        Ok(())
    }

    fn draw_shelf(&self, sv: &ShelfView, l: &Layout, a: f32, sh: bool, hover: Option<Btn>) -> Result<()> {
        let area = l.shelf;
        if sv.tiles.is_empty() || sv.drop_hover {
            // Zona para soltar: borde punteado, ámbar cuando hay algo encima.
            let (r, g, b, al) = if sv.drop_hover { (1.0, 0.72, 0.3, 0.95) } else { (1.0, 1.0, 1.0, 0.35) };
            let br = self.brush(r, g, b, al * a)?;
            let props = D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 1.0,
                dashStyle: D2D1_DASH_STYLE_DASH,
                dashOffset: 0.0,
                transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
            };
            let dash = unsafe { self.d2d_factory.CreateStrokeStyle(&props, None)? };
            let zone = R::new(area.l, area.t + 4.0, area.r, area.b);
            let rr = D2D1_ROUNDED_RECT { rect: d2r(zone), radiusX: 12.0, radiusY: 12.0 };
            unsafe { self.ctx.DrawRoundedRectangle(&rr, &br, 1.5, &dash) };
            if sv.tiles.is_empty() || sv.drop_hover {
                let msg = if sv.drop_hover { "Soltá para guardar en el estante" } else { "Arrastrá archivos acá para tenerlos a mano" };
                if sv.tiles.is_empty() || sv.drop_hover {
                    let txt = if sv.tiles.is_empty() { zone } else { R::new(zone.l, zone.b - 24.0, zone.r, zone.b - 4.0) };
                    self.label(msg, &self.fonts.lyric, txt, (r, g, b, al * a), sh)?;
                }
            }
        }
        let n = sv.tiles.len();
        for (i, (glyph, name)) in sv.tiles.iter().enumerate() {
            let t = l.shelf_tile(i, n);
            if hover == Some(Btn::ShelfItem(i)) {
                let bg = self.brush(1.0, 1.0, 1.0, if sh { 0.2 } else { 0.1 } * a)?;
                let rr = D2D1_ROUNDED_RECT { rect: d2r(t), radiusX: 10.0, radiusY: 10.0 };
                unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
            }
            self.label(glyph, &self.fonts.icon_big, R::new(t.l, t.t + 4.0, t.r, t.t + 38.0), (1.0, 1.0, 1.0, 0.9 * a), sh)?;
            self.label(name, &self.fonts.tile, R::new(t.l + 3.0, t.t + 40.0, t.r - 3.0, t.b - 4.0), (1.0, 1.0, 1.0, 0.8 * a), sh)?;
        }
        Ok(())
    }

    fn draw_clip(&self, cv: &ClipView, area: R, a: f32, sh: bool, hover: Option<Btn>) -> Result<()> {
        if cv.items.is_empty() {
            return self.label("Copiá un texto y aparece acá", &self.fonts.lyric, area, (1.0, 1.0, 1.0, 0.55 * a), sh);
        }
        for (i, item) in cv.items.iter().take(ui::CLIP_ROWS).enumerate() {
            let t = area.t + 6.0 + i as f32 * ui::CLIP_ROW_H;
            let row = R::new(area.l, t, area.r, t + ui::CLIP_ROW_H - 2.0);
            if hover == Some(Btn::ClipItem(i)) {
                let bg = self.brush(1.0, 1.0, 1.0, if sh { 0.2 } else { 0.1 } * a)?;
                let rr = D2D1_ROUNDED_RECT { rect: d2r(row), radiusX: 8.0, radiusY: 8.0 };
                unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
            }
            let text_r = R::new(row.l + 10.0, row.t, row.r - 10.0, row.b);
            if cv.copied == Some(i) {
                self.label("✓ Copiado", &self.fonts.artist, text_r, (0.55, 0.9, 0.6, a), sh)?;
            } else {
                let one_line: String = item.chars().take(300).map(|c| if c.is_control() { ' ' } else { c }).collect();
                self.label(one_line.trim(), &self.fonts.artist, text_r, (1.0, 1.0, 1.0, 0.85 * a), sh)?;
            }
        }
        Ok(())
    }

    fn draw_lyrics(&self, ly: &LyricsView, area: R, a: f32, sh: bool, hover: Option<Btn>) -> Result<()> {
        unsafe {
            self.ctx.PushAxisAlignedClip(&d2r(area), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        let lh = ui::LYRIC_LINE_H;
        let line_rect = |i: usize, scroll: f32| {
            let y = area.cy() + (i as f32 - scroll) * lh;
            R::new(area.l, y - lh / 2.0, area.r, y + lh / 2.0)
        };
        let fade = |i: usize, scroll: f32| clamp01(1.0 - ((i as f32 - scroll).abs() - 0.3) * 0.36);
        let res = (|| -> Result<()> {
            match ly {
                LyricsView::Message(msg) => {
                    self.label(msg, &self.fonts.lyric, area, (1.0, 1.0, 1.0, 0.55 * a), sh)?;
                }
                LyricsView::Synced { lines, scroll, current } => {
                    let lo = (*scroll - 4.0).max(0.0) as usize;
                    let hi = ((*scroll + 5.0).max(0.0) as usize).min(lines.len());
                    for i in lo..hi {
                        let cur = i == *current;
                        let hov = hover == Some(Btn::LyricLine(i));
                        let al = fade(i, *scroll) * if cur { 1.0 } else if hov { 0.85 } else { 0.45 };
                        let font = if cur { &self.fonts.lyric_cur } else { &self.fonts.lyric };
                        self.label(&lines[i].1, font, line_rect(i, *scroll), (1.0, 1.0, 1.0, al * a), sh)?;
                    }
                }
                LyricsView::Plain { lines, scroll } => {
                    let lo = (*scroll - 4.0).max(0.0) as usize;
                    let hi = ((*scroll + 5.0).max(0.0) as usize).min(lines.len());
                    for i in lo..hi {
                        let al = fade(i, *scroll) * 0.8;
                        self.label(&lines[i], &self.fonts.lyric, line_rect(i, *scroll), (1.0, 1.0, 1.0, al * a), sh)?;
                    }
                }
            }
            Ok(())
        })();
        unsafe {
            self.ctx.PopAxisAlignedClip();
        }
        res
    }

    /// Mascotas vectoriales (Direct2D, sin imágenes), solo ojos: Esme (ojos ámbar/miel con
    /// brillo) o el gato (los ojitos blancos originales).
    fn draw_pet(&self, cx: f32, cy: f32, eyes: Eyes, pet: Pet, a: f32) -> Result<()> {
        let (gx, gy) = eyes.gaze;
        let gap = if pet == Pet::Esme { 7.5 } else { 8.0 };
        let style = self.round_stroke()?;

        // Ojos.
        let (glow, iris, closed) = match pet {
            Pet::Esme => (
                self.brush(1.0, 0.62, 0.22, 0.28 * a)?,
                self.brush(1.0, 0.92, 0.80, 0.98 * a)?,
                self.brush(1.0, 0.75, 0.40, 0.95 * a)?,
            ),
            Pet::Gato => (
                self.brush(1.0, 1.0, 1.0, 0.0)?,
                self.brush(1.0, 1.0, 1.0, 0.95 * a)?,
                self.brush(1.0, 1.0, 1.0, 0.9 * a)?,
            ),
        };
        for side in [-1.0f32, 1.0] {
            let ex = cx + side * gap + gx;
            let ey = cy + gy;
            match eyes.mood {
                Mood::Greeting => unsafe {
                    // Ojitos felices: "^ ^".
                    let g = self.d2d_factory.CreatePathGeometry()?;
                    let s = g.Open()?;
                    s.BeginFigure(pt(ex - 4.0, cy + 2.0), D2D1_FIGURE_BEGIN_HOLLOW);
                    s.AddLine(pt(ex, cy - 2.5));
                    s.AddLine(pt(ex + 4.0, cy + 2.0));
                    s.EndFigure(D2D1_FIGURE_END_OPEN);
                    s.Close()?;
                    self.ctx.DrawGeometry(&g, &closed, 2.0, &style);
                },
                _ if eyes.open < 0.08 => unsafe {
                    let rr = D2D1_ROUNDED_RECT { rect: rect(ex - 3.5, cy - 0.8, ex + 3.5, cy + 0.8), radiusX: 0.8, radiusY: 0.8 };
                    self.ctx.FillRoundedRectangle(&rr, &closed);
                },
                _ if pet == Pet::Gato => unsafe {
                    // Ojitos originales: píldoras blancas que se achican al pestañear.
                    let hh = (4.5 * eyes.open).max(0.9);
                    let rr = D2D1_ROUNDED_RECT { rect: rect(ex - 3.0, ey - hh, ex + 3.0, ey + hh), radiusX: 3.0, radiusY: hh.min(3.0) };
                    self.ctx.FillRoundedRectangle(&rr, &iris);
                },
                _ => unsafe {
                    let (rx, ry) = (3.6, 6.0);
                    let hh = (ry * eyes.open).max(0.9);
                    for k in [2.6f32, 1.3] {
                        let g = D2D1_ELLIPSE { point: pt(ex, ey), radiusX: rx + k, radiusY: hh + k };
                        self.ctx.FillEllipse(&g, &glow);
                    }
                    let e = D2D1_ELLIPSE { point: pt(ex, ey), radiusX: rx, radiusY: hh };
                    self.ctx.FillEllipse(&e, &iris);
                    if pet == Pet::Esme && eyes.open > 0.6 {
                        // Brillo azul medianoche.
                        let glint = self.brush(0.25, 0.35, 0.95, 0.55 * a)?;
                        let d = D2D1_ELLIPSE { point: pt(ex - 1.1, ey - 2.2), radiusX: 0.9, radiusY: 0.9 };
                        self.ctx.FillEllipse(&d, &glint);
                    }
                },
            }
        }
        if eyes.mood == Mood::Asleep {
            let z = self.brush(1.0, 1.0, 1.0, 0.7 * a)?;
            let x = cx + gap + 4.0;
            for (i, (dx, dy)) in [(0.0f32, 0.0f32), (5.0, -5.0), (10.0, -10.0)].iter().enumerate() {
                let al = 0.9 - i as f32 * 0.25;
                let zb = self.brush(1.0, 1.0, 1.0, 0.7 * a * al)?;
                let _ = &z;
                self.text("z", &self.fonts.tiny, R::new(x + dx, cy - 6.0 + dy, x + dx + 8.0, cy + 2.0 + dy), &zb);
            }
        }
        Ok(())
    }

    fn round_stroke(&self) -> Result<ID2D1StrokeStyle1> {
        let props = D2D1_STROKE_STYLE_PROPERTIES1 {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            dashCap: D2D1_CAP_STYLE_ROUND,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            miterLimit: 1.0,
            dashStyle: D2D1_DASH_STYLE_SOLID,
            dashOffset: 0.0,
            transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
        };
        unsafe { self.d2d_factory.CreateStrokeStyle(&props, None) }
    }

    fn draw_art(&self, r: R, radius: f32, a: f32) -> Result<()> {
        let Some(bmp) = &self.art else { return Ok(()) };
        unsafe {
            let size = bmp.GetSize();
            // Recorte cuadrado centrado (las miniaturas de YouTube son 16:9).
            let side = size.width.min(size.height);
            let src = rect((size.width - side) / 2.0, (size.height - side) / 2.0, (size.width + side) / 2.0, (size.height + side) / 2.0);
            let rr = D2D1_ROUNDED_RECT { rect: d2r(r), radiusX: radius, radiusY: radius };
            let mask = self.d2d_factory.CreateRoundedRectangleGeometry(&rr)?;
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: d2r(r),
                geometricMask: std::mem::ManuallyDrop::new(Some(mask.cast()?)),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: a,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            self.ctx.PushLayer(&params, None);
            self.ctx.DrawBitmap(bmp, Some(&d2r(r)), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, Some(&src), None);
            self.ctx.PopLayer();
            std::mem::ManuallyDrop::into_inner(params.geometricMask);
        }
        Ok(())
    }

    fn button(&self, glyph: &str, fmt: &IDWriteTextFormat, r: R, a: f32, hover: bool, sh: bool) -> Result<()> {
        if hover {
            let bg = self.brush(1.0, 1.0, 1.0, if sh { 0.22 } else { 0.14 } * a)?;
            let e = D2D1_ROUNDED_RECT { rect: d2r(r), radiusX: 14.0, radiusY: 14.0 };
            unsafe { self.ctx.FillRoundedRectangle(&e, &bg) };
        }
        self.label(glyph, fmt, r, (1.0, 1.0, 1.0, a), sh)
    }

    /// Texto con sombra opcional (para que se lea sobre cualquier fondo en modo transparente).
    fn label(&self, s: &str, fmt: &IDWriteTextFormat, r: R, c: (f32, f32, f32, f32), shadow: bool) -> Result<()> {
        if shadow {
            let b = self.brush(0.0, 0.0, 0.0, 0.42 * c.3)?;
            for (dx, dy) in [(0.0, 1.2), (1.0, 0.6), (-1.0, 0.6), (0.0, 2.0)] {
                self.text(s, fmt, R::new(r.l + dx, r.t + dy, r.r + dx, r.b + dy), &b);
            }
        }
        let b = self.brush(c.0, c.1, c.2, c.3)?;
        self.text(s, fmt, r, &b);
        Ok(())
    }

    fn brush(&self, r: f32, g: f32, b: f32, a: f32) -> Result<ID2D1SolidColorBrush> {
        unsafe { self.ctx.CreateSolidColorBrush(&rgba(r, g, b, a), None) }
    }

    fn text(&self, s: &str, fmt: &IDWriteTextFormat, r: R, brush: &ID2D1SolidColorBrush) {
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            self.ctx.DrawText(&wide, fmt, &d2r(r), brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    /// Forma de notch: pegada al borde superior, con "orejas" cóncavas arriba
    /// y esquinas inferiores redondeadas.
    fn notch_geometry(&self, cx: f32, w: f32, h: f32, radius: f32, ear: f32) -> Result<ID2D1PathGeometry1> {
        let h = h.max(1.0);
        let ear = ear.min(h * 0.5).max(0.0);
        let r = radius.min(h - ear).min(w / 2.0).max(0.0);
        let l = cx - w / 2.0;
        let rt = cx + w / 2.0;
        unsafe {
            let geo = self.d2d_factory.CreatePathGeometry()?;
            let sink = geo.Open()?;
            sink.BeginFigure(pt(l - ear, 0.0), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: pt(l, 0.0), point2: pt(l, ear) });
            sink.AddLine(pt(l, h - r));
            sink.AddArc(&arc(pt(l + r, h), r));
            sink.AddLine(pt(rt - r, h));
            sink.AddArc(&arc(pt(rt, h - r), r));
            sink.AddLine(pt(rt, ear));
            sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: pt(rt, 0.0), point2: pt(rt + ear, 0.0) });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            Ok(geo)
        }
    }
}

fn font_exists(dw: &IDWriteFactory, family: &str) -> bool {
    unsafe {
        let mut col = None;
        if dw.GetSystemFontCollection(&mut col, false).is_err() {
            return false;
        }
        let Some(col) = col else { return false };
        let name: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
        let mut idx = 0u32;
        let mut exists = BOOL(0);
        col.FindFamilyName(PCWSTR(name.as_ptr()), &mut idx, &mut exists).is_ok() && exists.as_bool()
    }
}

fn swap_desc(w: u32, h: u32) -> DXGI_SWAP_CHAIN_DESC1 {
    DXGI_SWAP_CHAIN_DESC1 {
        Width: w,
        Height: h,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    }
}

fn tf(dw: &IDWriteFactory, family: &str, size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
    unsafe {
        let fam: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
        let f = dw.CreateTextFormat(PCWSTR(fam.as_ptr()), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("es-AR"))?;
        f.SetTextAlignment(align)?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(f)
    }
}

fn arc(end: Vector2, r: f32) -> D2D1_ARC_SEGMENT {
    D2D1_ARC_SEGMENT {
        point: end,
        size: D2D_SIZE_F { width: r, height: r },
        rotationAngle: 0.0,
        sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
        arcSize: D2D1_ARC_SIZE_SMALL,
    }
}

fn pt(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn rect(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: l, top: t, right: r, bottom: b }
}

fn d2r(r: R) -> D2D_RECT_F {
    rect(r.l, r.t, r.r, r.b)
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}
