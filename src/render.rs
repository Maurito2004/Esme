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
    /// Letra sincronizada en la píldora con desplazamiento vertical suave al cambiar de línea.
    LyricRoll { cur: &'a str, next: &'a str, frac: f32 },
    /// Overlay del sistema: ícono, texto y, si corresponde, un nivel 0..1 (volumen).
    Overlay { glyph: &'a str, text: &'a str, level: Option<f32> },
    /// Píldora de cargador: anillo circular animado y porcentaje que cuenta hasta el valor actual.
    Charger { charging: bool, pct: f32, ring: f32 },
}

/// Instantánea propietaria del contenido contraído para el morph saliente (~220 ms).
#[derive(Clone)]
pub enum CollapsedSnap {
    Clock(String),
    Minimal,
    Mascot { time: String, eyes: Eyes, pet: Pet },
    Media { time: String },
    Lyric(String),
    LyricRoll { cur: String, next: String, frac: f32 },
    Overlay { glyph: &'static str, text: String, level: Option<f32> },
    Charger { charging: bool, pct: f32, ring: f32 },
}

impl CollapsedSnap {
    pub fn as_ref(&self) -> Collapsed<'_> {
        match self {
            Self::Clock(t) => Collapsed::Clock(t.as_str()),
            Self::Minimal => Collapsed::Minimal,
            Self::Mascot { time, eyes, pet } => Collapsed::Mascot { time: time.as_str(), eyes: *eyes, pet: *pet },
            Self::Media { time } => Collapsed::Media { time: time.as_str() },
            Self::Lyric(s) => Collapsed::Lyric(s.as_str()),
            Self::LyricRoll { cur, next, frac } => {
                Collapsed::LyricRoll { cur: cur.as_str(), next: next.as_str(), frac: *frac }
            }
            Self::Overlay { glyph, text, level } => Collapsed::Overlay { glyph: *glyph, text: text.as_str(), level: *level },
            Self::Charger { charging, pct, ring } => {
                Collapsed::Charger { charging: *charging, pct: *pct, ring: *ring }
            }
        }
    }
}

#[derive(Clone, Copy)]
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

/// Panel "Hoy": Pomodoro, agua, clima y próximos eventos.
pub struct TodayView<'a> {
    pub pomo_label: &'a str,
    pub pomo_time: String,
    pub pomo_running: bool,
    pub pomo_progress: f32,
    pub water: u32,
    pub water_prev: u32,
    pub water_num_t: f32,
    pub water_dir: f32,
    pub water_hours: [u32; 24],
    pub water_cur_hour: usize,
    pub water_bars_t: f32,
    pub water_bump: Option<(usize, f32)>,
    pub goal: u32,
    pub weather_glyph: &'a str,
    pub weather: &'a str,
    /// (cuándo, título) de los próximos eventos; vacío = mostrar `cal_hint`.
    pub events: &'a [(String, String)],
    pub cal_hint: &'a str,
    /// Tareas visibles en la ventana actual del To-Do.
    pub todos: &'a [crate::todo::TodoItem],
    pub todo_strikes: &'a [f32],
    pub todo_pop: Option<(usize, f32)>,
    pub todo_done: usize,
    pub todo_done_prev: usize,
    pub todo_count_t: f32,
    pub todo_count_dir: f32,
    pub todo_total: usize,
    pub todo_scroll: usize,
    pub todo_input: &'a str,
    pub todo_focused: bool,
    pub todo_caret: bool,
}

pub struct ShelfView<'a> {
    /// (glifo, nombre visible) de cada archivo.
    pub tiles: &'a [(&'static str, String)],
    /// Hay archivos arrastrándose encima: resaltamos la zona.
    pub drop_hover: bool,
    /// Entrada animada (0..1) de los archivos recién soltados (escala 0.9 → 1.0 + enfoque).
    pub enter: f32,
    /// Cuántos archivos al inicio del estante están entrando en este momento.
    pub new_count: usize,
}

/// Mezclador: (nombre, volumen 0..1, silenciado).
pub struct MixerView<'a> {
    pub rows: &'a [(String, f32, bool)],
    /// Fila que se está arrastrando (muestra la perilla aunque el mouse se salga).
    pub drag: Option<usize>,
}

pub struct ClipView<'a> {
    pub items: &'a [String],
    /// Fila recién copiada (muestra "✓ Copiado").
    pub copied: Option<usize>,
}

/// Panel "Ask Esme" + Traductor + Calculadora inline (Fase 6).
pub struct AskView<'a> {
    pub mode_label: &'a str,
    pub is_translate: bool,
    pub input: &'a str,
    pub placeholder: &'a str,
    pub focused: bool,
    pub caret_on: bool,
    pub selected_all: bool,
    /// Resultado inline en vivo si termina en `=` o `timer ...`.
    pub inline_preview: Option<&'a str>,
    pub loading: bool,
    pub response: &'a str,
    pub copied: bool,
    pub has_clip: bool,
    /// Ojitos de la mascota en el panel (miran el input al escribir, "piensan" al esperar).
    pub pet: Option<(Eyes, Pet)>,
}

pub enum Expanded<'a> {
    Clock { time: &'a str, date: &'a str },
    Media(MediaView<'a>),
}

/// Subpanel saliente durante una transición entre páginas del panel expandido (~220 ms).
pub enum PrevSub<'a> {
    Ask(AskView<'a>),
    Today(TodayView<'a>),
    Shelf(ShelfView<'a>),
    Clip(ClipView<'a>),
    Mixer(MixerView<'a>),
    Lyrics(LyricsView<'a>),
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
    /// Visibilidad del contenido expandido (cierre en dos tiempos: 1→0 antes de encoger la forma).
    pub content_vis: f32,
    /// Modo transparente: la píldora queda chica y el contenido flota debajo.
    pub transparent: bool,
    pub collapsed: Collapsed<'a>,
    /// Morph de contenido en la píldora contraída (0→1 en ~220 ms).
    pub pill_morph: f32,
    pub prev_collapsed: Option<&'a CollapsedSnap>,
    pub expanded: Expanded<'a>,
    pub layout: Layout,
    /// Morph + deslizamiento entre páginas del panel expandido (0→1 en ~220 ms).
    pub page_morph: f32,
    /// +1 si sube el índice (entra desde la derecha), -1 si baja (desde la izquierda), 0 si solo morph.
    pub page_dir: f32,
    pub prev_sub: Option<(Layout, PrevSub<'a>)>,
    pub hover: Option<Btn>,
    /// Botón presionado y su escala elástica (1.0 → 0.92 → 1.0).
    pub pressed: Option<(Btn, f32)>,
    /// 0..1: cuánto se ve la mascota (desplaza el reloj).
    pub mascot_vis: f32,
    /// Panel del portapapeles abierto.
    pub clip: Option<ClipView<'a>>,
    /// Estante de archivos abierto.
    pub shelf: Option<ShelfView<'a>>,
    /// Panel "Hoy" abierto.
    pub today: Option<TodayView<'a>>,
    /// Panel "Ask Esme" (Fase 6) abierto.
    pub ask: Option<AskView<'a>>,
    /// Mezclador por app abierto.
    pub mixer: Option<MixerView<'a>>,
    /// Color de acento: el de los ojos de la mascota (ámbar en Esme, blanco en el gato).
    pub accent: (f32, f32, f32),
    /// Línea de luz estática azul → acento pegada al borde inferior de la píldora cerrada.
    pub pill_glow_line: bool,
    /// Brillo suave que "respira" (~4 s) con Pomodoro corriendo en la píldora cerrada: `(fase 0..1, es_foco)`.
    pub pomo_breath: Option<(f32, bool)>,
    /// Píldora flotante: separación (DIPs) con el borde de arriba; 0 = pegada (notch clásico).
    pub float_gap: f32,
    /// Ecualizador vivo: segundos desde que arrancó la animación (None = barras quietas).
    pub eq_t: Option<f32>,
    /// Vista activa del panel expandido (para los puntitos indicadores de página).
    pub active_page: usize,
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
    wrap: IDWriteTextFormat,
    dw: IDWriteFactory,
}

pub struct Renderer {
    dxgi_device: IDXGIDevice,
    swap: IDXGISwapChain1,
    d2d_factory: ID2D1Factory1,
    ctx: ID2D1DeviceContext,
    _dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    fonts: Fonts,
    /// Fábrica WIC inicializada de forma diferida (solo al decodificar carátulas) y liberada en reposo.
    wic: Option<IWICImagingFactory>,
    art: Option<ID2D1Bitmap1>,
    /// Carátula anterior, para el fundido al cambiar de canción (se libera al terminar el fade).
    prev_art: Option<ID2D1Bitmap1>,
    art_changed: Option<std::time::Instant>,
    /// Color predominante de la carátula actual (para el aura del borde).
    art_color: Option<(f32, f32, f32)>,
    /// Desplazamiento vertical de todo el dibujo (píldora flotante).
    oy: f32,
    /// ID de la carátula cargada (0 = ninguna). Nunca retenemos el Vec<u8> original en RAM.
    art_id: u64,
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
/// Duración del fundido entre carátulas.
const ART_FADE_SECS: f32 = 0.35;
/// Azul de la marca (borde del notch).
const BLUE: (f32, f32, f32) = (0.27, 0.45, 1.0);
const G_CLIP: &str = "\u{E77F}";
const G_SHELF: &str = "\u{E8B7}";
const G_TODAY: &str = "\u{E787}";
const G_ASK: &str = "\u{E945}";
const G_VOLUME: &str = "\u{E767}";
const G_MUTE: &str = "\u{E74F}";

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
            let ui_font = if font_exists(&dw, "Segoe UI Variable Display") { "Segoe UI Variable Display" } else { "Segoe UI" };
            let c = DWRITE_TEXT_ALIGNMENT_CENTER;
            let l = DWRITE_TEXT_ALIGNMENT_LEADING;
            let wrap = tf(&dw, ui_font, 13.0, DWRITE_FONT_WEIGHT_NORMAL, l)?;
            wrap.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            wrap.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            // Si una respuesta no entra, la última línea visible termina en "…".
            if let Ok(sign) = dw.CreateEllipsisTrimmingSign(&wrap) {
                let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_WORD, delimiter: 0, delimiterCount: 0 };
                let _ = wrap.SetTrimming(&trim, &sign);
            }
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
                wrap,
                dw: dw.clone(),
            };
            for f in [&fonts.title, &fonts.artist, &fonts.lyric, &fonts.lyric_cur, &fonts.pill_lyric, &fonts.tile] {
                let sign = dw.CreateEllipsisTrimmingSign(f)?;
                let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                f.SetTrimming(&trim, &sign)?;
            }

            // WIC se crea de forma diferida (lazy) la primera vez que llega una carátula.
            let mut r = Self {
                dxgi_device,
                swap,
                d2d_factory,
                ctx,
                _dcomp: dcomp,
                _target: target,
                _visual: visual,
                fonts,
                wic: None,
                art: None,
                prev_art: None,
                art_changed: None,
                art_color: None,
                oy: 0.0,
                art_id: 0,
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

    /// Inicializa la fábrica WIC bajo demanda (no carga windowscodecs.dll al arrancar).
    fn ensure_wic(&mut self) -> Option<&IWICImagingFactory> {
        if self.wic.is_none() {
            self.wic = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };
        }
        self.wic.as_ref()
    }

    /// Libera cachés gráficos secundarios en reposo absoluto y pide a DXGI purgar buffers del driver.
    pub fn release_idle_caches(&mut self) {
        if self.art_fade() >= 1.0 {
            self.prev_art = None;
        }
        self.wic = None;
        unsafe {
            if let Ok(d3) = self.dxgi_device.cast::<IDXGIDevice3>() {
                d3.Trim();
            }
        }
    }

    /// Carátula: decodificada en una sola pasada WIC, escalada al tamaño exacto de UI (64×64 DIP × DPI)
    /// y descartando inmediatamente el buffer original de alta resolución.
    pub fn set_art(&mut self, id: u64, bytes: Option<Vec<u8>>) {
        if id == self.art_id && (bytes.is_none() || self.art.is_some()) {
            return;
        }
        if id == 0 {
            self.prev_art = self.art.take();
            self.art = None;
            self.art_color = None;
            self.art_id = 0;
            self.art_changed = Some(std::time::Instant::now());
            return;
        }
        let Some(raw) = bytes else {
            return;
        };
        self.prev_art = self.art.take();
        if let Some((bmp, color)) = self.decode_art_and_color(&raw) {
            self.art = Some(bmp);
            self.art_color = color;
        } else {
            self.art = None;
            self.art_color = None;
        }
        drop(raw);
        self.art_id = id;
        self.art_changed = Some(std::time::Instant::now());
    }

    /// Avance del fundido de carátula (1 = terminado).
    fn art_fade(&self) -> f32 {
        self.art_changed.map(|t| (t.elapsed().as_secs_f32() / ART_FADE_SECS).min(1.0)).unwrap_or(1.0)
    }

    /// ¿Hay una animación propia del renderer en curso? (fundido de carátula)
    pub fn animating(&self) -> bool {
        self.art_fade() < 1.0 && (self.prev_art.is_some() || self.art.is_some())
    }

    pub fn art_color(&self) -> Option<(f32, f32, f32)> {
        self.art_color
    }

    /// Decodifica la carátula en una sola pasada WIC, la escala con `IWICBitmapScaler` al tamaño
    /// físico exacto del slot de 64×64 DIP (64 px a 100% DPI, máx. 128 px a 200% DPI) y calcula
    /// el color dominante sobre esos mismos píxeles sin decodificar dos veces.
    fn decode_art_and_color(&mut self, bytes: &[u8]) -> Option<(ID2D1Bitmap1, Option<(f32, f32, f32)>)> {
        let target_px = (64.0 * self.scale).round().clamp(64.0, 128.0);
        let wic = self.ensure_wic()?.clone();
        unsafe {
            let stream = wic.CreateStream().ok()?;
            stream.InitializeFromMemory(bytes).ok()?;
            let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
            let frame = dec.GetFrame(0).ok()?;
            let (mut w, mut h) = (0u32, 0u32);
            frame.GetSize(&mut w, &mut h).ok()?;
            // Mantenemos relación de aspecto dejando el lado corto en `target_px` (sin ampliar si ya es chica).
            let short_side = w.min(h).max(1) as f32;
            let k = (target_px / short_side).min(1.0);
            let tw = ((w as f32 * k).round() as u32).clamp(1, (target_px * 2.0) as u32);
            let th = ((h as f32 * k).round() as u32).clamp(1, (target_px * 2.0) as u32);

            let scaler = wic.CreateBitmapScaler().ok()?;
            scaler.Initialize(&frame, tw, th, WICBitmapInterpolationModeFant).ok()?;
            let conv = wic.CreateFormatConverter().ok()?;
            conv.Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )
            .ok()?;

            // 1) Color predominante leyendo una muestra liviana del bitmap ya reducido.
            let stride = tw * 4;
            let mut px = vec![0u8; (stride * th) as usize];
            let color = if conv.CopyPixels(std::ptr::null(), stride, &mut px).is_ok() {
                dominant_from_pbgra(&px)
            } else {
                None
            };
            drop(px);

            // 2) Bitmap D2D final de tamaño exacto (64×64 = ~16 KB en VRAM a 100% DPI).
            let bmp = self.ctx.CreateBitmapFromWicBitmap(&conv, None).ok()?;
            Some((bmp, color))
        }
    }

    /// Dibuja y presenta un frame. Con `vsync` bloquea hasta el próximo refresco
    /// (eso marca el ritmo de las animaciones, sin timers).
    pub fn draw(&mut self, f: &Frame, vsync: bool) -> Result<()> {
        unsafe {
            self.ctx.BeginDraw();
            self.oy = f.float_gap;
            self.ctx.SetTransform(&Matrix3x2::identity());
            self.ctx.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
            self.ctx.SetTransform(&Matrix3x2::translation(0.0, self.oy));

            let geo = if f.float_gap > 0.0 {
                self.pill_geometry(f.cx, f.shape_w, f.shape_h, f.radius)?
            } else {
                self.notch_geometry(f.cx, f.shape_w, f.shape_h, f.radius, f.ear)?
            };
            let black = self.brush(0.0, 0.0, 0.0, 1.0)?;
            self.ctx.FillGeometry(&geo, &black, None);

            // Borde de luz al expandir: degradé azul → color de los ojos de la mascota.
            let glow = clamp01(f.expand);
            if glow > 0.02 && !f.transparent {
                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(BLUE.0, BLUE.1, BLUE.2, 0.75 * glow) },
                    D2D1_GRADIENT_STOP { position: 0.5, color: rgba(BLUE.0, BLUE.1, BLUE.2, 0.15 * glow) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(f.accent.0, f.accent.1, f.accent.2, 0.75 * glow) },
                ];
                let coll = self.ctx.CreateGradientStopCollection(&stops, D2D1_COLOR_SPACE_SRGB, D2D1_COLOR_SPACE_SRGB, D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_EXTEND_MODE_CLAMP, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED)?;
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: pt(f.cx - f.shape_w / 2.0, f.shape_h),
                    endPoint: pt(f.cx + f.shape_w / 2.0, f.shape_h),
                };
                let gb = self.ctx.CreateLinearGradientBrush(&props, None, &coll)?;
                self.ctx.DrawGeometry(&geo, &gb, 1.5, None);
            }

            // Línea de luz estática azul → acento pegada al borde inferior de la píldora cerrada.
            let line_vis = if f.transparent { 1.0 } else { clamp01(1.0 - f.expand * 2.2) };
            if f.pill_glow_line && line_vis > 0.01 && !matches!(f.collapsed, Collapsed::Minimal) && f.shape_h >= 12.0 {
                self.draw_pill_glow_line(f, line_vis)?;
            }
            // Pomodoro corriendo con la píldora cerrada: brillo suave que respira (~4 s, máx. 15 FPS).
            if let Some((phase, focus)) = f.pomo_breath {
                if line_vis > 0.01 && f.shape_h >= 12.0 {
                    self.draw_pomo_breath(f, &geo, phase, focus, line_vis)?;
                }
            }

            // Contenido contraído: en modo negro se desvanece al expandir; en transparente queda.
            let a_small = if f.transparent { 1.0 } else { clamp01(1.0 - f.expand * 2.5) };
            if a_small > 0.01 {
                self.draw_collapsed(f, &geo, a_small)?;
            }
            // Contenido expandido: aparece al abrir y se va primero (~120 ms) antes de encoger al cerrar.
            let a_big = clamp01((f.expand - 0.45) / 0.55) * clamp01((f.content_vis - 0.06) / 0.94);
            if a_big > 0.01 {
                self.draw_expanded(f, &geo, a_big)?;
            }

            self.ctx.EndDraw(None, None)?;
            self.swap.Present(if vsync { 1 } else { 0 }, DXGI_PRESENT(0)).ok()?;
            // Al terminar el crossfade de carátula liberamos el bitmap anterior de VRAM de inmediato.
            if self.prev_art.is_some() && self.art_fade() >= 1.0 {
                self.prev_art = None;
            }
            Ok(())
        }
    }

    /// Transformación afín (escala uniforme `s` alrededor de `(px, py)` + traslación `(dx, dy)` + `self.oy`).
    fn transform_at(&self, s: f32, px: f32, py: f32, dx: f32, dy: f32) -> Matrix3x2 {
        Matrix3x2 {
            M11: s,
            M12: 0.0,
            M21: 0.0,
            M22: s,
            M31: px * (1.0 - s) + dx,
            M32: py * (1.0 - s) + dy + self.oy,
        }
    }

    /// Aplica una escala local `s` alrededor de `(px, py)` componiéndola con la transformación activa
    /// (preserva `self.oy` y cualquier morph/deslizamiento de página) y restaura la matriz anterior.
    fn with_local_scale<F>(&self, s: f32, px: f32, py: f32, draw: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        if (s - 1.0).abs() < 0.001 {
            return draw();
        }
        unsafe {
            let mut prev = Matrix3x2::identity();
            self.ctx.GetTransform(&mut prev);
            let local = Matrix3x2 {
                M11: s,
                M12: 0.0,
                M21: 0.0,
                M22: s,
                M31: px * (1.0 - s),
                M32: py * (1.0 - s),
            };
            let combined = mul_m3x2(local, prev);
            self.ctx.SetTransform(&combined);
            let res = draw();
            self.ctx.SetTransform(&prev);
            res
        }
    }

    /// Brillo suave que respira en el contorno de la píldora cerrada mientras corre el Pomodoro.
    fn draw_pomo_breath(&self, f: &Frame, geo: &ID2D1PathGeometry1, phase: f32, focus: bool, vis: f32) -> Result<()> {
        let p = clamp01(phase);
        let c = if focus { (1.0, 0.42, 0.32) } else { (0.35, 0.88, 0.58) };
        let halo = self.brush(c.0, c.1, c.2, (0.06 + 0.16 * p) * vis)?;
        let rim = self.brush(c.0, c.1, c.2, (0.18 + 0.52 * p) * vis)?;
        unsafe {
            self.ctx.DrawGeometry(geo, &halo, 3.2, None);
            self.ctx.DrawGeometry(geo, &rim, 1.35, None);
        }
        // Sutil reflejo inferior pulsante dentro de la píldora.
        let h = f.shape_h;
        let inset = f.radius.min(h * 0.5).max(10.0);
        let x0 = f.cx - f.shape_w / 2.0 + inset;
        let x1 = f.cx + f.shape_w / 2.0 - inset;
        if x1 - x0 >= 12.0 {
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(c.0, c.1, c.2, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.5, color: rgba(c.0, c.1, c.2, (0.14 + 0.32 * p) * vis) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(c.0, c.1, c.2, 0.0) },
            ];
            unsafe {
                let coll = self.ctx.CreateGradientStopCollection(
                    &stops,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_BUFFER_PRECISION_8BPC_UNORM,
                    D2D1_EXTEND_MODE_CLAMP,
                    D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
                )?;
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: pt(x0, h - 1.5),
                    endPoint: pt(x1, h - 1.5),
                };
                let gb = self.ctx.CreateLinearGradientBrush(&props, None, &coll)?;
                let rr = D2D1_ROUNDED_RECT { rect: rect(x0, h - 2.8, x1, h - 0.4), radiusX: 1.2, radiusY: 1.2 };
                self.ctx.FillRoundedRectangle(&rr, &gb);
            }
        }
        Ok(())
    }

    /// Recorta temporalmente el dibujo dentro de la geometría del notch/píldora durante transiciones.
    fn with_geo_mask<F>(&self, geo: &ID2D1PathGeometry1, bounds: D2D_RECT_F, draw: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        unsafe {
            let mask: ID2D1Geometry = geo.cast()?;
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: bounds,
                geometricMask: std::mem::ManuallyDrop::new(Some(mask)),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            self.ctx.PushLayer(&params, None);
            let res = draw();
            self.ctx.SetTransform(&Matrix3x2::translation(0.0, self.oy));
            self.ctx.PopLayer();
            let _ = std::mem::ManuallyDrop::into_inner(params.geometricMask);
            res
        }
    }

    /// Línea fina con degradé azul → acento pegada al borde inferior de la píldora cerrada (sin costo en reposo).
    fn draw_pill_glow_line(&self, f: &Frame, vis: f32) -> Result<()> {
        let h = f.shape_h;
        let r_bot = if f.float_gap > 0.0 {
            if h <= 40.0 { h / 2.0 } else { f.radius.max(16.0) }.min(h / 2.0).min(f.shape_w / 2.0).max(0.0)
        } else {
            let ear = f.ear.min(h * 0.5).max(0.0);
            f.radius.min(h - ear).min(f.shape_w / 2.0).max(0.0)
        };
        let inset = r_bot.max(10.0);
        let x0 = f.cx - f.shape_w / 2.0 + inset;
        let x1 = f.cx + f.shape_w / 2.0 - inset;
        if x1 - x0 < 12.0 {
            return Ok(());
        }
        let mid = (
            (BLUE.0 + f.accent.0) * 0.5,
            (BLUE.1 + f.accent.1) * 0.5,
            (BLUE.2 + f.accent.2) * 0.5,
        );
        let halo_stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(BLUE.0, BLUE.1, BLUE.2, 0.0) },
            D2D1_GRADIENT_STOP { position: 0.18, color: rgba(BLUE.0, BLUE.1, BLUE.2, 0.24 * vis) },
            D2D1_GRADIENT_STOP { position: 0.5, color: rgba(mid.0, mid.1, mid.2, 0.28 * vis) },
            D2D1_GRADIENT_STOP { position: 0.82, color: rgba(f.accent.0, f.accent.1, f.accent.2, 0.24 * vis) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(f.accent.0, f.accent.1, f.accent.2, 0.0) },
        ];
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(BLUE.0, BLUE.1, BLUE.2, 0.0) },
            D2D1_GRADIENT_STOP { position: 0.14, color: rgba(BLUE.0, BLUE.1, BLUE.2, 0.85 * vis) },
            D2D1_GRADIENT_STOP { position: 0.5, color: rgba(mid.0, mid.1, mid.2, 0.92 * vis) },
            D2D1_GRADIENT_STOP { position: 0.86, color: rgba(f.accent.0, f.accent.1, f.accent.2, 0.85 * vis) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(f.accent.0, f.accent.1, f.accent.2, 0.0) },
        ];
        unsafe {
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: pt(x0, h - 1.0),
                endPoint: pt(x1, h - 1.0),
            };
            let halo_coll = self.ctx.CreateGradientStopCollection(
                &halo_stops,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_BUFFER_PRECISION_8BPC_UNORM,
                D2D1_EXTEND_MODE_CLAMP,
                D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
            )?;
            let halo_gb = self.ctx.CreateLinearGradientBrush(&props, None, &halo_coll)?;
            let halo_rr = D2D1_ROUNDED_RECT { rect: rect(x0, h - 3.0, x1, h - 0.3), radiusX: 1.35, radiusY: 1.35 };
            self.ctx.FillRoundedRectangle(&halo_rr, &halo_gb);

            let coll = self.ctx.CreateGradientStopCollection(
                &stops,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_BUFFER_PRECISION_8BPC_UNORM,
                D2D1_EXTEND_MODE_CLAMP,
                D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
            )?;
            let gb = self.ctx.CreateLinearGradientBrush(&props, None, &coll)?;
            let line_rr = D2D1_ROUNDED_RECT { rect: rect(x0, h - 1.7, x1, h - 0.3), radiusX: 0.7, radiusY: 0.7 };
            self.ctx.FillRoundedRectangle(&line_rr, &gb);
        }
        Ok(())
    }

    fn draw_collapsed(&self, f: &Frame, geo: &ID2D1PathGeometry1, a: f32) -> Result<()> {
        let m = clamp01(f.pill_morph);
        let py = f.shape_h / 2.0;
        if m < 0.999 {
            if let Some(prev) = f.prev_collapsed {
                let bounds = rect(f.cx - f.shape_w / 2.0 - f.ear - 2.0, -2.0, f.cx + f.shape_w / 2.0 + f.ear + 2.0, f.shape_h + 2.0);
                let out_t = clamp01(m * 1.45);
                let in_t = clamp01((m - 0.15) / 0.85);
                return self.with_geo_mask(geo, bounds, || {
                    let a_out = a * (1.0 - out_t);
                    if a_out > 0.01 {
                        let s_out = 1.0 - 0.04 * out_t;
                        unsafe { self.ctx.SetTransform(&self.transform_at(s_out, f.cx, py, 0.0, 0.0)) };
                        self.draw_collapsed_item(&prev.as_ref(), f, a_out)?;
                    }
                    let a_in = a * in_t;
                    if a_in > 0.01 {
                        let s_in = 0.96 + 0.04 * in_t;
                        unsafe { self.ctx.SetTransform(&self.transform_at(s_in, f.cx, py, 0.0, 0.0)) };
                        self.draw_collapsed_item(&f.collapsed, f, a_in)?;
                    }
                    Ok(())
                });
            }
        }
        if a < 0.999 {
            let s = 0.96 + 0.04 * a;
            unsafe { self.ctx.SetTransform(&self.transform_at(s, f.cx, py, 0.0, 0.0)) };
            let res = self.draw_collapsed_item(&f.collapsed, f, a);
            unsafe { self.ctx.SetTransform(&Matrix3x2::translation(0.0, self.oy)) };
            res
        } else {
            self.draw_collapsed_item(&f.collapsed, f, a)
        }
    }

    fn draw_collapsed_item(&self, item: &Collapsed, f: &Frame, a: f32) -> Result<()> {
        let h = f.shape_h;
        let left = f.cx - f.shape_w / 2.0;
        let right = f.cx + f.shape_w / 2.0;
        let white = self.brush(1.0, 1.0, 1.0, 0.92 * a)?;
        match item {
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
                // Ecualizador: con música sonando las barras bailan (~10 FPS) con el color de la carátula.
                let (er, eg, eb) = self.art_color.map(|c| boost(c)).unwrap_or((0.55, 0.85, 0.6));
                let accent = self.brush(er, eg, eb, 0.95 * a)?;
                let mut heights = [8.0f32, 14.0, 10.0, 5.0];
                if let Some(t) = f.eq_t {
                    const SPD: [(f32, f32, f32); 4] = [(7.1, 2.3, 0.0), (9.4, 3.1, 1.7), (6.3, 4.2, 3.1), (8.2, 1.9, 4.6)];
                    for (i, (w1, w2, ph)) in SPD.iter().enumerate() {
                        let v = 0.5 + 0.5 * (t * w1 + ph).sin() * (0.65 + 0.35 * (t * w2 + ph * 0.5).sin());
                        heights[i] = 4.0 + 12.0 * v.clamp(0.0, 1.0);
                    }
                }
                for (i, bh) in heights.iter().enumerate() {
                    let x = right - 34.0 + i as f32 * 5.0;
                    let rr = D2D1_ROUNDED_RECT { rect: rect(x, h / 2.0 - bh / 2.0, x + 3.0, h / 2.0 + bh / 2.0), radiusX: 1.5, radiusY: 1.5 };
                    unsafe { self.ctx.FillRoundedRectangle(&rr, &accent) };
                }
            }
            Collapsed::Overlay { glyph, text, level } => {
                let icon = R::new(left + 12.0, 0.0, left + 40.0, h);
                self.label(glyph, &self.fonts.icon, icon, (1.0, 1.0, 1.0, 0.95 * a), false)?;
                match level {
                    Some(v) => {
                        let track = R::new(left + 48.0, h / 2.0 - 2.0, right - 52.0, h / 2.0 + 2.0);
                        let bg = self.brush(1.0, 1.0, 1.0, 0.22 * a)?;
                        let fg = self.brush(f.accent.0, f.accent.1, f.accent.2, 0.95 * a)?; // color de la mascota
                        let full = D2D1_ROUNDED_RECT { rect: d2r(track), radiusX: 2.0, radiusY: 2.0 };
                        let x = track.l + (track.r - track.l) * v.clamp(0.0, 1.0);
                        let done = D2D1_ROUNDED_RECT { rect: rect(track.l, track.t, x.max(track.l + 4.0), track.b), radiusX: 2.0, radiusY: 2.0 };
                        unsafe {
                            self.ctx.FillRoundedRectangle(&full, &bg);
                            if *v > 0.0 {
                                self.ctx.FillRoundedRectangle(&done, &fg);
                            }
                        }
                        self.text(text, &self.fonts.small, R::new(right - 50.0, 0.0, right - 10.0, h), &white);
                    }
                    None => {
                        let r = R::new(left + 44.0, 0.0, right - 14.0, h);
                        self.text(text, &self.fonts.pill_lyric, r, &white);
                    }
                }
            }
            Collapsed::Lyric(line) => {
                let r = R::new(left + 16.0, 0.0, right - 16.0, h);
                self.text(line, &self.fonts.pill_lyric, r, &white);
            }
            Collapsed::LyricRoll { cur, next, frac } => {
                let clip_r = R::new(left + 14.0, 0.0, right - 14.0, h);
                let t = clamp01(*frac);
                let dy = t * h;
                unsafe {
                    self.ctx.PushAxisAlignedClip(&d2r(clip_r), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                }
                let res = (|| -> Result<()> {
                    let b_cur = self.brush(1.0, 1.0, 1.0, 0.92 * a * (1.0 - t * 0.85))?;
                    self.text(cur, &self.fonts.pill_lyric, R::new(left + 16.0, -dy, right - 16.0, h - dy), &b_cur);
                    if !next.is_empty() && t > 0.01 {
                        let b_next = self.brush(1.0, 1.0, 1.0, 0.92 * a * (0.15 + 0.85 * t))?;
                        self.text(
                            next,
                            &self.fonts.pill_lyric,
                            R::new(left + 16.0, h - dy, right - 16.0, 2.0 * h - dy),
                            &b_next,
                        );
                    }
                    Ok(())
                })();
                unsafe {
                    self.ctx.PopAxisAlignedClip();
                }
                res?;
            }
            Collapsed::Charger { charging, pct, ring } => {
                let col = if *charging { (0.26, 0.90, 0.48) } else { (1.0, 0.72, 0.25) };
                let rcx = left + 26.0;
                let rcy = h / 2.0;
                let rad = (h * 0.30).clamp(7.0, 10.0);
                let track_br = self.brush(1.0, 1.0, 1.0, 0.20 * a)?;
                let ring_br = self.brush(col.0, col.1, col.2, 0.96 * a)?;
                let style = self.round_stroke()?;
                let ell = D2D1_ELLIPSE { point: pt(rcx, rcy), radiusX: rad, radiusY: rad };
                unsafe {
                    self.ctx.DrawEllipse(&ell, &track_br, 2.4, None);
                }
                let frac = clamp01(*ring);
                if frac >= 0.995 {
                    unsafe {
                        self.ctx.DrawEllipse(&ell, &ring_br, 2.4, &style);
                    }
                } else if frac > 0.005 {
                    let theta = frac * std::f32::consts::TAU;
                    let ang_end = theta - std::f32::consts::FRAC_PI_2;
                    let end_pt = pt(rcx + rad * ang_end.cos(), rcy + rad * ang_end.sin());
                    unsafe {
                        let g = self.d2d_factory.CreatePathGeometry()?;
                        let s = g.Open()?;
                        s.BeginFigure(pt(rcx, rcy - rad), D2D1_FIGURE_BEGIN_HOLLOW);
                        if frac <= 0.5 {
                            s.AddArc(&arc_cw(end_pt, rad));
                        } else {
                            s.AddArc(&arc_cw(pt(rcx, rcy + rad), rad));
                            s.AddArc(&arc_cw(end_pt, rad));
                        }
                        s.EndFigure(D2D1_FIGURE_END_OPEN);
                        s.Close()?;
                        self.ctx.DrawGeometry(&g, &ring_br, 2.4, &style);
                    }
                }
                // Mini rayo vectorial dentro del anillo al conectar, o punto central en batería.
                if *charging {
                    unsafe {
                        let bolt = self.d2d_factory.CreatePathGeometry()?;
                        let s = bolt.Open()?;
                        s.BeginFigure(pt(rcx + 0.8, rcy - 3.6), D2D1_FIGURE_BEGIN_HOLLOW);
                        s.AddLine(pt(rcx - 2.0, rcy + 0.2));
                        s.AddLine(pt(rcx + 1.8, rcy + 0.2));
                        s.AddLine(pt(rcx - 0.8, rcy + 3.6));
                        s.EndFigure(D2D1_FIGURE_END_OPEN);
                        s.Close()?;
                        self.ctx.DrawGeometry(&bolt, &ring_br, 1.35, &style);
                    }
                } else {
                    let dot = D2D1_ELLIPSE { point: pt(rcx, rcy), radiusX: 2.2, radiusY: 2.2 };
                    unsafe { self.ctx.FillEllipse(&dot, &ring_br) };
                }
                let status = if *charging { "Cargando" } else { "Con batería" };
                let label_br = self.brush(1.0, 1.0, 1.0, 0.86 * a)?;
                self.text(status, &self.fonts.small, R::new(left + 42.0, 0.0, right - 54.0, h), &label_br);
                let pct_txt = format!("{}%", pct.round().clamp(0.0, 100.0) as i32);
                self.text(&pct_txt, &self.fonts.small, R::new(right - 56.0, 0.0, right - 12.0, h), &ring_br);
            }
        }
        Ok(())
    }

    fn draw_expanded(&self, f: &Frame, geo: &ID2D1PathGeometry1, a: f32) -> Result<()> {
        let pm = clamp01(f.page_morph);
        let morphing = pm < 0.999;
        if !f.transparent && (morphing || a < 0.999) {
            let bounds = rect(f.cx - f.shape_w / 2.0 - f.ear - 2.0, -2.0, f.cx + f.shape_w / 2.0 + f.ear + 2.0, f.shape_h + 2.0);
            self.with_geo_mask(geo, bounds, || self.draw_expanded_body(f, a, pm, morphing))
        } else {
            let res = self.draw_expanded_body(f, a, pm, morphing);
            unsafe {
                self.ctx.SetTransform(&Matrix3x2::translation(0.0, self.oy));
            }
            res
        }
    }

    fn draw_expanded_body(&self, f: &Frame, a: f32, pm: f32, morphing: bool) -> Result<()> {
        let rise = (1.0 - a) * 8.0;
        let s_open = 0.96 + 0.04 * a;
        let sh = f.transparent;
        let l = &f.layout;

        // Cabecera (Reloj o Multimedia) + botones de la barra superior.
        let pure_hdr_morph = morphing && f.page_dir == 0.0 && f.prev_sub.is_none();
        let a_hdr = if pure_hdr_morph { a * pm } else { a };
        let s_hdr = s_open * if pure_hdr_morph { 0.96 + 0.04 * pm } else { 1.0 };
        unsafe {
            self.ctx.SetTransform(&self.transform_at(s_hdr, f.cx, l.area.cy(), 0.0, -rise));
        }
        match &f.expanded {
            Expanded::Clock { time, date } => {
                self.label(time, &self.fonts.big, l.clock, (1.0, 1.0, 1.0, a_hdr), sh)?;
                self.label(date, &self.fonts.date, l.date, (1.0, 1.0, 1.0, 0.65 * a_hdr), sh)?;
            }
            Expanded::Media(m) => {
                if self.art.is_some() {
                    if sh {
                        let shadow = self.brush(0.0, 0.0, 0.0, 0.35 * a_hdr)?;
                        let rr = D2D1_ROUNDED_RECT { rect: d2r(l.art.inflate(1.5)), radiusX: 11.0, radiusY: 11.0 };
                        unsafe { self.ctx.FillRoundedRectangle(&rr, &shadow) };
                    }
                    self.draw_art(l.art, 10.0, a_hdr)?;
                } else {
                    let bg = self.brush(1.0, 1.0, 1.0, 0.12 * a_hdr)?;
                    let rr = D2D1_ROUNDED_RECT { rect: d2r(l.art), radiusX: 10.0, radiusY: 10.0 };
                    unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
                    self.label(G_MUSIC, &self.fonts.icon_big, l.art, (1.0, 1.0, 1.0, 0.6 * a_hdr), false)?;
                }
                let title = if m.title.is_empty() { "Reproduciendo" } else { m.title };
                self.label(title, &self.fonts.title, l.title, (1.0, 1.0, 1.0, a_hdr), sh)?;
                self.label(m.artist, &self.fonts.artist, l.artist, (1.0, 1.0, 1.0, 0.62 * a_hdr), sh)?;

                let hv = |b: Btn| f.hover == Some(b);
                let bs = |b: Btn| btn_scale_for(f.pressed, b);
                self.button(G_PREV, &self.fonts.icon, l.prev, a_hdr * if m.can_prev { 1.0 } else { 0.35 }, hv(Btn::Prev), sh, bs(Btn::Prev))?;
                self.button(if m.playing { G_PAUSE } else { G_PLAY }, &self.fonts.icon_big, l.play, a_hdr, hv(Btn::Play), sh, bs(Btn::Play))?;
                self.button(G_NEXT, &self.fonts.icon, l.next, a_hdr * if m.can_next { 1.0 } else { 0.35 }, hv(Btn::Next), sh, bs(Btn::Next))?;
                let on = m.lyrics.is_some();
                self.button(G_LYRICS, &self.fonts.icon, l.lyrics_btn, a_hdr * if on { 1.0 } else { 0.7 }, hv(Btn::Lyrics) || on, sh, bs(Btn::Lyrics))?;

                if let Some(p) = m.progress {
                    self.label(&m.pos, &self.fonts.time, l.t_left, (1.0, 1.0, 1.0, 0.6 * a_hdr), sh)?;
                    self.label(&m.dur, &self.fonts.time, l.t_right, (1.0, 1.0, 1.0, 0.6 * a_hdr), sh)?;
                    let t = l.track;
                    if sh {
                        let s = self.brush(0.0, 0.0, 0.0, 0.4 * a_hdr)?;
                        let rr = D2D1_ROUNDED_RECT { rect: d2r(t.inflate(1.0)), radiusX: 3.0, radiusY: 3.0 };
                        unsafe { self.ctx.FillRoundedRectangle(&rr, &s) };
                    }
                    let bg = self.brush(1.0, 1.0, 1.0, 0.25 * a_hdr)?;
                    let fg = self.brush(f.accent.0, f.accent.1, f.accent.2, 0.92 * a_hdr)?;
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
            }
        }
        let bs = |b: Btn| btn_scale_for(f.pressed, b);
        let on = f.clip.is_some();
        self.button(G_CLIP, &self.fonts.icon, l.clip_btn, a_hdr * if on { 1.0 } else { 0.7 }, f.hover == Some(Btn::Clip) || on, sh, bs(Btn::Clip))?;
        let on = f.shelf.is_some();
        self.button(G_SHELF, &self.fonts.icon, l.shelf_btn, a_hdr * if on { 1.0 } else { 0.7 }, f.hover == Some(Btn::Shelf) || on, sh, bs(Btn::Shelf))?;
        let on = f.today.is_some();
        self.button(G_TODAY, &self.fonts.icon, l.today_btn, a_hdr * if on { 1.0 } else { 0.7 }, f.hover == Some(Btn::Today) || on, sh, bs(Btn::Today))?;
        let on = f.ask.is_some();
        self.button(G_ASK, &self.fonts.icon, l.ask_btn, a_hdr * if on { 1.0 } else { 0.7 }, f.hover == Some(Btn::Ask) || on, sh, bs(Btn::Ask))?;
        if l.mixer_btn.r > l.mixer_btn.l {
            let on = f.mixer.is_some();
            self.button(G_VOLUME, &self.fonts.icon, l.mixer_btn, a_hdr * if on { 1.0 } else { 0.7 }, f.hover == Some(Btn::Mixer) || on, sh, bs(Btn::Mixer))?;
        }

        // Subpanel saliente (morph escala 1.0 → 0.96 + fundido + salida lateral de 24 px).
        let slide = 24.0;
        if morphing {
            if let Some((pl, prev)) = &f.prev_sub {
                let out_t = clamp01(pm * 1.45);
                let a_out = a * (1.0 - out_t);
                if a_out > 0.01 {
                    let s_out = s_open * (1.0 - 0.04 * out_t);
                    let dx_out = -f.page_dir * slide * out_t;
                    unsafe {
                        self.ctx.SetTransform(&self.transform_at(s_out, f.cx, pl.area.cy(), dx_out, -rise));
                    }
                    self.draw_prev_sub(prev, pl, f.accent, a_out, sh)?;
                }
            }
        }

        // Subpanel entrante / activo (morph escala 0.96 → 1.0 + fundido + entrada lateral de 24 px).
        let ap = clamp01((a - 0.35) / 0.65);
        let in_t = if morphing { clamp01((pm - 0.12) / 0.88) } else { 1.0 };
        let a_in = ap * in_t;
        if a_in > 0.01 {
            let s_in = s_open * if morphing { 0.96 + 0.04 * in_t } else { 1.0 };
            let dx_in = if morphing { f.page_dir * slide * (1.0 - pm) } else { 0.0 };
            let dy_in = -rise + (1.0 - ap) * 10.0;
            unsafe {
                self.ctx.SetTransform(&self.transform_at(s_in, f.cx, l.area.cy(), dx_in, dy_in));
            }
            if let Expanded::Media(m) = &f.expanded {
                if let Some(ly) = &m.lyrics {
                    self.draw_lyrics(ly, l.lyrics, a_in, sh, f.hover)?;
                }
            }
            if let Some(cv) = &f.clip {
                self.draw_clip(cv, l.clip, a_in, sh, f.hover, f.pressed)?;
            }
            if let Some(sv) = &f.shelf {
                self.draw_shelf(sv, l, f.accent, a_in, sh, f.hover, f.pressed)?;
            }
            if let Some(tv) = &f.today {
                self.draw_today(tv, l, f.accent, a_in, sh, f.hover, f.pressed)?;
            }
            if let Some(av) = &f.ask {
                self.draw_ask(av, l, f.accent, a_in, sh, f.hover, f.pressed)?;
            }
            if let Some(mv) = &f.mixer {
                self.draw_mixer(mv, l, f.accent, a_in, sh, f.hover, f.pressed)?;
            }
        }

        // Puntitos indicadores de página fijos al pie del panel.
        unsafe {
            self.ctx.SetTransform(&self.transform_at(s_open, f.cx, l.area.cy(), 0.0, -rise));
        }
        self.draw_page_dots(l, f.active_page, f.accent, a, sh, f.hover, f.pressed)?;
        Ok(())
    }

    fn draw_prev_sub(&self, prev: &PrevSub, l: &Layout, accent: (f32, f32, f32), a: f32, sh: bool) -> Result<()> {
        match prev {
            PrevSub::Ask(av) => self.draw_ask(av, l, accent, a, sh, None, None),
            PrevSub::Today(tv) => self.draw_today(tv, l, accent, a, sh, None, None),
            PrevSub::Shelf(sv) => self.draw_shelf(sv, l, accent, a, sh, None, None),
            PrevSub::Clip(cv) => self.draw_clip(cv, l.clip, a, sh, None, None),
            PrevSub::Mixer(mv) => self.draw_mixer(mv, l, accent, a, sh, None, None),
            PrevSub::Lyrics(ly) => self.draw_lyrics(ly, l.lyrics, a, sh, None),
        }
    }

    /// Panel Ask Esme (Fase 6): chip de modo (Tab), acciones de portapapeles, input Direct2D con cursor y respuesta.
    #[allow(clippy::too_many_arguments)]
    fn draw_ask(
        &self,
        av: &AskView,
        l: &Layout,
        accent: (f32, f32, f32),
        a: f32,
        sh: bool,
        hover: Option<Btn>,
        pressed: Option<(Btn, f32)>,
    ) -> Result<()> {
        let hv = |b: Btn| hover == Some(b);

        // 1. Chip de Modo (Preguntar / Traducir).
        let mode_c = if av.is_translate { BLUE } else { accent };
        let ms = btn_scale_for(pressed, Btn::AskMode);
        self.with_local_scale(ms, l.ask_mode.cx(), l.ask_mode.cy(), || {
            let mode_bg = self.brush(mode_c.0, mode_c.1, mode_c.2, (if hv(Btn::AskMode) { 0.32 } else { 0.20 }) * a)?;
            let mode_border = self.brush(mode_c.0, mode_c.1, mode_c.2, 0.75 * a)?;
            let mode_rr = D2D1_ROUNDED_RECT { rect: d2r(l.ask_mode), radiusX: 8.0, radiusY: 8.0 };
            unsafe {
                self.ctx.FillRoundedRectangle(&mode_rr, &mode_bg);
                self.ctx.DrawRoundedRectangle(&mode_rr, &mode_border, 1.0, None);
            }
            self.label(av.mode_label, &self.fonts.time, l.ask_mode, (1.0, 1.0, 1.0, 0.95 * a), sh)
        })?;
        if let Some((eyes, pet)) = av.pet {
            let ex = (l.ask_mode.r + l.ask_summarize.l) / 2.0;
            if l.ask_summarize.l - l.ask_mode.r > 40.0 {
                self.draw_pet(ex, l.ask_mode.cy(), eyes, pet, a)?;
            }
        }

        // 2. Botones rápidos sobre el último texto del portapapeles ("✨ Resumir clip" y "✎ Corregir").
        let clip_alpha = if av.has_clip { 0.9 } else { 0.38 };
        for (rect_btn, btn_id, txt) in [
            (l.ask_summarize, Btn::AskSummarize, "✨ Resumir clip"),
            (l.ask_spellcheck, Btn::AskSpellcheck, "✎ Corregir"),
        ] {
            let s = btn_scale_for(pressed, btn_id);
            self.with_local_scale(s, rect_btn.cx(), rect_btn.cy(), || {
                let bg_a = if hv(btn_id) && av.has_clip { 0.22 } else { 0.10 };
                let bg = self.brush(1.0, 1.0, 1.0, bg_a * a)?;
                let rr = D2D1_ROUNDED_RECT { rect: d2r(rect_btn), radiusX: 7.0, radiusY: 7.0 };
                unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
                self.label(txt, &self.fonts.time, rect_btn, (1.0, 1.0, 1.0, clip_alpha * a), sh)
            })?;
        }

        // 3. Cuadro de texto nativo en Direct2D.
        let inp = l.ask_input;
        if sh {
            let s = self.brush(0.0, 0.0, 0.0, 0.45 * a)?;
            let rr = D2D1_ROUNDED_RECT { rect: d2r(inp.inflate(1.0)), radiusX: 9.0, radiusY: 9.0 };
            unsafe { self.ctx.FillRoundedRectangle(&rr, &s) };
        }
        let box_bg = self.brush(1.0, 1.0, 1.0, (if sh { 0.16 } else { 0.09 }) * a)?;
        let border_c = if av.focused { mode_c } else { (1.0, 1.0, 1.0) };
        let border_a = if av.focused { 0.85 } else if hv(Btn::AskInput) { 0.4 } else { 0.22 };
        let box_border = self.brush(border_c.0, border_c.1, border_c.2, border_a * a)?;
        let inp_rr = D2D1_ROUNDED_RECT { rect: d2r(inp), radiusX: 8.0, radiusY: 8.0 };
        unsafe {
            self.ctx.FillRoundedRectangle(&inp_rr, &box_bg);
            self.ctx.DrawRoundedRectangle(&inp_rr, &box_border, if av.focused { 1.4 } else { 1.0 }, None);
        }

        let pad_x = inp.l + 10.0;
        let max_text_w = (inp.r - inp.l - 20.0).max(20.0);
        let text_r = R::new(pad_x, inp.t, inp.r - 10.0, inp.b);

        if av.input.is_empty() {
            self.label(av.placeholder, &self.fonts.artist, text_r, (1.0, 1.0, 1.0, 0.42 * a), sh)?;
            if av.focused && av.caret_on {
                let caret = self.brush(mode_c.0, mode_c.1, mode_c.2, 0.95 * a)?;
                let cr = rect(pad_x, inp.t + 7.0, pad_x + 1.6, inp.b - 7.0);
                unsafe { self.ctx.FillRectangle(&cr, &caret) };
            }
        } else {
            // Si la cadena es muy larga, mostramos la cola visible para que el cursor siempre quede a la vista.
            let display = tail_fitting(av.input, 58);
            let tw = self.measure_text_width(&display, &self.fonts.artist, max_text_w);
            if av.selected_all {
                let sel_bg = self.brush(BLUE.0, BLUE.1, BLUE.2, 0.38 * a)?;
                let sr = D2D1_ROUNDED_RECT {
                    rect: rect(pad_x - 2.0, inp.t + 5.0, (pad_x + tw + 3.0).min(inp.r - 8.0), inp.b - 5.0),
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                unsafe { self.ctx.FillRoundedRectangle(&sr, &sel_bg) };
            }
            self.label(&display, &self.fonts.artist, text_r, (1.0, 1.0, 1.0, 0.96 * a), sh)?;

            // Vista previa inline en vivo (ej. "= 4725" o "⏱ Enter: 10 min") a la derecha del input.
            if let Some(prev) = av.inline_preview {
                let pr = R::new((pad_x + tw + 10.0).min(inp.r - 120.0), inp.t, inp.r - 10.0, inp.b);
                self.label(prev, &self.fonts.artist, pr, (accent.0, accent.1, accent.2, 0.95 * a), sh)?;
            }

            if av.focused && av.caret_on {
                let cx = (pad_x + tw + 1.5).min(inp.r - 10.0);
                let caret = self.brush(mode_c.0, mode_c.1, mode_c.2, 0.95 * a)?;
                let cr = rect(cx, inp.t + 7.0, cx + 1.6, inp.b - 7.0);
                unsafe { self.ctx.FillRectangle(&cr, &caret) };
            }
        }

        // 4. Área de respuesta o estado de carga.
        if l.ask_response.r > l.ask_response.l {
            let resp_r = l.ask_response;
            if av.loading {
                self.label("Pensando…", &self.fonts.artist, R::new(resp_r.l + 4.0, resp_r.t + 4.0, resp_r.r - 4.0, resp_r.t + 26.0), (accent.0, accent.1, accent.2, 0.85 * a), sh)?;
            } else if !av.response.is_empty() {
                let bg = self.brush(1.0, 1.0, 1.0, (if sh { 0.14 } else { 0.06 }) * a)?;
                let rr = D2D1_ROUNDED_RECT { rect: d2r(resp_r), radiusX: 8.0, radiusY: 8.0 };
                unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };

                let txt_box = R::new(resp_r.l + 8.0, resp_r.t + 5.0, resp_r.r - 8.0, resp_r.b - 24.0);
                self.label(av.response, &self.fonts.wrap, txt_box, (1.0, 1.0, 1.0, 0.92 * a), sh)?;

                // Botón de copiar respuesta.
                let cp = l.ask_copy;
                let cps = btn_scale_for(pressed, Btn::AskCopy);
                self.with_local_scale(cps, cp.cx(), cp.cy(), || {
                    let cp_bg = self.brush(1.0, 1.0, 1.0, (if hv(Btn::AskCopy) { 0.24 } else { 0.12 }) * a)?;
                    let cp_rr = D2D1_ROUNDED_RECT { rect: d2r(cp), radiusX: 6.0, radiusY: 6.0 };
                    unsafe { self.ctx.FillRoundedRectangle(&cp_rr, &cp_bg) };
                    let (cp_txt, cp_col) = if av.copied {
                        ("✓ Copiado", (0.55, 0.92, 0.62, a))
                    } else {
                        ("📋 Copiar", (1.0, 1.0, 1.0, 0.85 * a))
                    };
                    self.label(cp_txt, &self.fonts.time, cp, cp_col, sh)
                })?;
            }
        }
        Ok(())
    }

    /// Alto que ocupa un texto con la fuente de la respuesta de Ask Esme, ajustado a `width`.
    pub fn wrapped_height(&self, text: &str, width: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        unsafe {
            let wide: Vec<u16> = text.encode_utf16().collect();
            let Ok(layout) = self.fonts.dw.CreateTextLayout(&wide, &self.fonts.wrap, width.max(40.0), 2000.0) else {
                return 0.0;
            };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_ok() { m.height } else { 0.0 }
        }
    }

    /// Mide el ancho exacto en DIPs de un texto con DirectWrite para ubicar el cursor.
    fn measure_text_width(&self, text: &str, fmt: &IDWriteTextFormat, max_w: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        unsafe {
            let wide: Vec<u16> = text.encode_utf16().collect();
            let Ok(layout) = self.fonts.dw.CreateTextLayout(&wide, fmt, max_w, 40.0) else {
                return 0.0;
            };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_ok() {
                m.widthIncludingTrailingWhitespace
            } else {
                0.0
            }
        }
    }

    /// Barrita de progreso redondeada.
    fn bar(&self, r: R, v: f32, color: (f32, f32, f32), a: f32) -> Result<()> {
        let bg = self.brush(1.0, 1.0, 1.0, 0.18 * a)?;
        let fg = self.brush(color.0, color.1, color.2, 0.95 * a)?;
        let full = D2D1_ROUNDED_RECT { rect: d2r(r), radiusX: 2.0, radiusY: 2.0 };
        let x = r.l + (r.r - r.l) * v.clamp(0.0, 1.0);
        unsafe {
            self.ctx.FillRoundedRectangle(&full, &bg);
            if v > 0.0 {
                let done = D2D1_ROUNDED_RECT { rect: rect(r.l, r.t, x.max(r.l + 4.0), r.b), radiusX: 2.0, radiusY: 2.0 };
                self.ctx.FillRoundedRectangle(&done, &fg);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_today(
        &self,
        tv: &TodayView,
        l: &Layout,
        accent: (f32, f32, f32),
        a: f32,
        sh: bool,
        hover: Option<Btn>,
        pressed: Option<(Btn, f32)>,
    ) -> Result<()> {
        let dim = (1.0, 1.0, 1.0, 0.6 * a);
        let white = (1.0, 1.0, 1.0, a);
        let hv = |b: Btn| hover == Some(b);
        let bs = |b: Btn| btn_scale_for(pressed, b);
        let left = |row: R, w: f32| R::new(row.l + 4.0, row.t, row.l + 4.0 + w, row.b);
        let shown = |row: R| row.l > -1000.0;
        if ![l.pomo_row, l.water_row, l.weather_row, l.cal_row, l.todo_row].iter().any(|r| shown(*r)) {
            let t = l.today;
            return self.label("Elegí qué mostrar en Ajustes → Hoy", &self.fonts.artist, R::new(t.l + 4.0, t.t + 4.0, t.r - 4.0, t.t + 32.0), dim, sh);
        }

        // Pomodoro.
        let p = l.pomo_row;
        if shown(p) {
            self.label(tv.pomo_label, &self.fonts.artist, R::new(p.l + 4.0, p.t, l.pomo_toggle.l - 8.0, p.t + 18.0), dim, sh)?;
            self.label(&tv.pomo_time, &self.fonts.title, R::new(p.l + 4.0, p.t + 16.0, p.l + 90.0, p.b), white, sh)?;
            let by = p.t + 26.0;
            self.bar(R::new(p.l + 96.0, by - 2.0, l.pomo_toggle.l - 12.0, by + 2.0), tv.pomo_progress, (1.0, 0.45, 0.35), a)?;
            self.button(if tv.pomo_running { G_PAUSE } else { G_PLAY }, &self.fonts.icon, l.pomo_toggle, a, hv(Btn::PomoToggle), sh, bs(Btn::PomoToggle))?;
            self.button("\u{E72C}", &self.fonts.icon, l.pomo_reset, a * 0.8, hv(Btn::PomoReset), sh, bs(Btn::PomoReset))?;
        }

        // Agua: número que se desliza al sumar/restar + 24 barritas por hora que crecen escalonadas.
        let w = l.water_row;
        if shown(w) {
            self.label("Agua", &self.fonts.artist, R::new(w.l + 4.0, w.t, w.l + 114.0, w.t + 16.0), dim, sh)?;
            let cur_s = tv.water.to_string();
            let prev_s = tv.water_prev.to_string();
            let num_w = self
                .measure_text_width(&cur_s, &self.fonts.artist, 36.0)
                .max(self.measure_text_width(&prev_s, &self.fonts.artist, 36.0))
                .max(10.0)
                + 2.0;
            let num_r = R::new(w.l + 4.0, w.t + 16.0, w.l + 4.0 + num_w, w.b);
            let nt = clamp01(tv.water_num_t);
            if nt < 0.999 && tv.water_prev != tv.water {
                let nh = (num_r.b - num_r.t).max(12.0);
                let dir = if tv.water_dir >= 0.0 { 1.0 } else { -1.0 };
                let old_dy = -dir * nh * nt;
                let new_dy = dir * nh * (1.0 - nt);
                unsafe {
                    self.ctx.PushAxisAlignedClip(&d2r(num_r), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                }
                let res = (|| -> Result<()> {
                    self.label(
                        &prev_s,
                        &self.fonts.artist,
                        R::new(num_r.l, num_r.t + old_dy, num_r.r, num_r.b + old_dy),
                        (1.0, 1.0, 1.0, a * (1.0 - nt)),
                        sh,
                    )?;
                    self.label(
                        &cur_s,
                        &self.fonts.artist,
                        R::new(num_r.l, num_r.t + new_dy, num_r.r, num_r.b + new_dy),
                        (1.0, 1.0, 1.0, a * nt),
                        sh,
                    )
                })();
                unsafe {
                    self.ctx.PopAxisAlignedClip();
                }
                res?;
            } else {
                self.label(&cur_s, &self.fonts.artist, num_r, white, sh)?;
            }
            let suffix = format!("/ {} vasos", tv.goal);
            self.label(&suffix, &self.fonts.artist, R::new(num_r.r + 3.0, w.t + 16.0, w.l + 116.0, w.b), (1.0, 1.0, 1.0, 0.85 * a), sh)?;

            // Gráfico de 24 barritas por hora (crecen escalonadas de abajo hacia arriba al abrir Hoy).
            let chart_l = w.l + 118.0;
            let chart_r = l.water_sub.l - 12.0;
            if chart_r - chart_l >= 48.0 {
                let base_y = w.b - 8.5;
                let max_h = (base_y - (w.t + 3.5)).max(10.0);
                let slot = (chart_r - chart_l) / 24.0;
                let bw = (slot - 1.5).clamp(2.0, 6.5);
                let max_v = tv.water_hours.iter().copied().max().unwrap_or(0).max(2) as f32;
                let bars_t = clamp01(tv.water_bars_t);
                let idle_br = self.brush(1.0, 1.0, 1.0, 0.14 * a)?;
                let now_idle_br = self.brush(0.35, 0.72, 1.0, 0.30 * a)?;
                let fill_br = self.brush(0.35, 0.72, 1.0, 0.72 * a)?;
                let now_fill_br = self.brush(0.45, 0.80, 1.0, 0.98 * a)?;
                for (h_idx, &cnt) in tv.water_hours.iter().enumerate() {
                    let delay = (h_idx as f32 / 23.0) * 0.42;
                    let prog = clamp01((bars_t - delay) / 0.58);
                    if prog <= 0.005 {
                        continue;
                    }
                    let x0 = chart_l + h_idx as f32 * slot + (slot - bw) * 0.5;
                    let is_now = h_idx == tv.water_cur_hour;
                    let (bh, br) = if cnt == 0 {
                        (2.2 * prog, if is_now { &now_idle_br } else { &idle_br })
                    } else {
                        let frac = (cnt as f32 / max_v).clamp(0.20, 1.0);
                        let bump = match tv.water_bump {
                            Some((bh_idx, k)) if bh_idx == h_idx => k.clamp(0.6, 1.35),
                            _ => 1.0,
                        };
                        let h_px = ((3.5 + (max_h - 3.5) * frac) * prog * bump).min(max_h + 2.0);
                        (h_px, if is_now { &now_fill_br } else { &fill_br })
                    };
                    let rr = D2D1_ROUNDED_RECT {
                        rect: rect(x0, base_y - bh, x0 + bw, base_y),
                        radiusX: (bw * 0.5).min(1.5),
                        radiusY: (bw * 0.5).min(1.5),
                    };
                    unsafe { self.ctx.FillRoundedRectangle(&rr, br) };
                }
                let v = tv.water as f32 / tv.goal.max(1) as f32;
                self.bar(R::new(chart_l, w.b - 5.0, chart_r, w.b - 2.0), v, (0.35, 0.65, 1.0), a)?;
            }
            self.button("\u{E738}", &self.fonts.icon, l.water_sub, a * 0.8, hv(Btn::WaterSub), sh, bs(Btn::WaterSub))?;
            self.button("\u{E710}", &self.fonts.icon, l.water_add, a, hv(Btn::WaterAdd), sh, bs(Btn::WaterAdd))?;
        }

        // Clima.
        let c = l.weather_row;
        if shown(c) {
            self.label(tv.weather_glyph, &self.fonts.title, left(c, 24.0), white, sh)?;
            self.label(tv.weather, &self.fonts.artist, R::new(c.l + 34.0, c.t, c.r - 4.0, c.b), white, sh)?;
        }

        // Calendario.
        let k = l.cal_row;
        if shown(k) {
            self.label("\u{E787}", &self.fonts.icon, R::new(k.l + 4.0, k.t + 2.0, k.l + 28.0, k.t + 24.0), dim, sh)?;
            if tv.events.is_empty() {
                self.label(tv.cal_hint, &self.fonts.artist, R::new(k.l + 34.0, k.t + 2.0, k.r - 4.0, k.t + 24.0), dim, sh)?;
            }
            for (i, (when, title)) in tv.events.iter().take(2).enumerate() {
                let y = k.t + 2.0 + i as f32 * 22.0;
                self.label(when, &self.fonts.artist, R::new(k.l + 34.0, y, k.l + 140.0, y + 22.0), dim, sh)?;
                self.label(title, &self.fonts.artist, R::new(k.l + 144.0, y, k.r - 4.0, y + 22.0), white, sh)?;
            }
        }

        // Tareas rápidas (To-Do): pop del círculo, tachado izq→der y contador deslizante.
        if shown(l.todo_row) {
            let inp = l.todo_input;
            let bg = self.brush(1.0, 1.0, 1.0, (if sh { 0.16 } else { 0.09 }) * a)?;
            let bc = if tv.todo_focused { accent } else { (1.0, 1.0, 1.0) };
            let ba = if tv.todo_focused { 0.85 } else if hv(Btn::TodoInput) { 0.42 } else { 0.20 };
            let border = self.brush(bc.0, bc.1, bc.2, ba * a)?;
            let rr = D2D1_ROUNDED_RECT { rect: d2r(inp), radiusX: 7.0, radiusY: 7.0 };
            unsafe {
                self.ctx.FillRoundedRectangle(&rr, &bg);
                self.ctx.DrawRoundedRectangle(&rr, &border, if tv.todo_focused { 1.3 } else { 1.0 }, None);
            }
            let has_counter = tv.todo_total > 0;
            let txt_right = if has_counter { inp.r - 48.0 } else { inp.r - 8.0 };
            let pad_x = inp.l + 8.0;
            let max_w = (txt_right - pad_x).max(20.0);
            let txt_r = R::new(pad_x, inp.t, txt_right, inp.b);
            if tv.todo_input.is_empty() {
                self.label("Nueva tarea… (ej. 18:30 Llamar)", &self.fonts.artist, txt_r, (1.0, 1.0, 1.0, 0.42 * a), sh)?;
                if tv.todo_focused && tv.todo_caret {
                    let caret = self.brush(accent.0, accent.1, accent.2, 0.95 * a)?;
                    let cr = rect(pad_x, inp.t + 5.0, pad_x + 1.5, inp.b - 5.0);
                    unsafe { self.ctx.FillRectangle(&cr, &caret) };
                }
            } else {
                let display = tail_fitting(tv.todo_input, 42);
                let tw = self.measure_text_width(&display, &self.fonts.artist, max_w);
                self.label(&display, &self.fonts.artist, txt_r, (1.0, 1.0, 1.0, 0.96 * a), sh)?;
                if tv.todo_focused && tv.todo_caret {
                    let cx = (pad_x + tw + 1.5).min(txt_right);
                    let caret = self.brush(accent.0, accent.1, accent.2, 0.95 * a)?;
                    let cr = rect(cx, inp.t + 5.0, cx + 1.5, inp.b - 5.0);
                    unsafe { self.ctx.FillRectangle(&cr, &caret) };
                }
            }

            // Contador deslizante `hechas/total` (ej. 3/5 → 4/5) en el extremo derecho del cuadro.
            if has_counter {
                let cnt_r = R::new(inp.r - 44.0, inp.t + 4.0, inp.r - 5.0, inp.b - 4.0);
                let all_done = tv.todo_done == tv.todo_total;
                let cbg = if all_done {
                    self.brush(accent.0, accent.1, accent.2, 0.22 * a)?
                } else {
                    self.brush(1.0, 1.0, 1.0, 0.08 * a)?
                };
                let crr = D2D1_ROUNDED_RECT { rect: d2r(cnt_r), radiusX: 5.0, radiusY: 5.0 };
                unsafe { self.ctx.FillRoundedRectangle(&crr, &cbg) };

                let done_s = tv.todo_done.to_string();
                let prev_s = tv.todo_done_prev.to_string();
                let tot_s = format!("/{}", tv.todo_total);
                let done_w = self
                    .measure_text_width(&done_s, &self.fonts.time, 20.0)
                    .max(self.measure_text_width(&prev_s, &self.fonts.time, 20.0))
                    .max(7.0);
                let tot_w = self.measure_text_width(&tot_s, &self.fonts.time, 24.0).max(10.0);
                let total_w = done_w + tot_w;
                let sx = cnt_r.cx() - total_w * 0.5;
                let done_r = R::new(sx, cnt_r.t, sx + done_w, cnt_r.b);
                let tot_r = R::new(sx + done_w, cnt_r.t, sx + total_w + 1.0, cnt_r.b);
                let ccol = if all_done { (accent.0, accent.1, accent.2, 0.96 * a) } else { (1.0, 1.0, 1.0, 0.85 * a) };
                let ct = clamp01(tv.todo_count_t);
                if ct < 0.999 && tv.todo_done_prev != tv.todo_done {
                    let ch = (done_r.b - done_r.t).max(10.0);
                    let dir = if tv.todo_count_dir >= 0.0 { 1.0 } else { -1.0 };
                    let old_dy = -dir * ch * ct;
                    let new_dy = dir * ch * (1.0 - ct);
                    unsafe {
                        self.ctx.PushAxisAlignedClip(&d2r(done_r), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    }
                    let res = (|| -> Result<()> {
                        self.label(
                            &prev_s,
                            &self.fonts.time,
                            R::new(done_r.l, done_r.t + old_dy, done_r.r, done_r.b + old_dy),
                            (ccol.0, ccol.1, ccol.2, ccol.3 * (1.0 - ct)),
                            false,
                        )?;
                        self.label(
                            &done_s,
                            &self.fonts.time,
                            R::new(done_r.l, done_r.t + new_dy, done_r.r, done_r.b + new_dy),
                            (ccol.0, ccol.1, ccol.2, ccol.3 * ct),
                            false,
                        )
                    })();
                    unsafe {
                        self.ctx.PopAxisAlignedClip();
                    }
                    res?;
                } else {
                    self.label(&done_s, &self.fonts.time, done_r, ccol, false)?;
                }
                self.label(&tot_s, &self.fonts.time, tot_r, (1.0, 1.0, 1.0, 0.55 * a), false)?;
            }

            // Botón "+" para agregar.
            self.with_local_scale(bs(Btn::TodoAdd), l.todo_add.cx(), l.todo_add.cy(), || {
                let add_bg = self.brush(1.0, 1.0, 1.0, (if hv(Btn::TodoAdd) { 0.24 } else { 0.12 }) * a)?;
                let add_rr = D2D1_ROUNDED_RECT { rect: d2r(l.todo_add), radiusX: 7.0, radiusY: 7.0 };
                unsafe { self.ctx.FillRoundedRectangle(&add_rr, &add_bg) };
                self.label("\u{E710}", &self.fonts.icon, l.todo_add, (1.0, 1.0, 1.0, 0.9 * a), sh)
            })?;

            // Lista de tareas.
            let list = l.todo_list;
            if tv.todos.is_empty() {
                let empty_r = R::new(list.l + 2.0, list.t, list.r, list.t + ui::TODO_ROW_H);
                self.label("Sin tareas · escribí arriba y apretá Enter", &self.fonts.artist, empty_r, dim, sh)?;
            } else {
                let check_style = self.round_stroke()?;
                let lum = 0.299 * accent.0 + 0.587 * accent.1 + 0.114 * accent.2;
                for (i, item) in tv.todos.iter().enumerate() {
                    let y = list.t + i as f32 * ui::TODO_ROW_H;
                    let row = R::new(list.l, y, list.r, y + ui::TODO_ROW_H - 2.0);
                    if hv(Btn::TodoToggle(i)) {
                        let hbg = self.brush(1.0, 1.0, 1.0, if sh { 0.16 } else { 0.08 } * a)?;
                        let hrr = D2D1_ROUNDED_RECT { rect: d2r(row), radiusX: 6.0, radiusY: 6.0 };
                        unsafe { self.ctx.FillRoundedRectangle(&hrr, &hbg) };
                    }
                    let check_r = R::new(row.l + 2.0, row.t, row.l + 24.0, row.b);
                    let del_r = R::new(row.r - 22.0, row.t, row.r, row.b);
                    let mut text_right = del_r.l - 4.0;
                    if !item.due.is_empty() {
                        let due_r = R::new(del_r.l - 46.0, row.t + 2.0, del_r.l - 4.0, row.b - 2.0);
                        text_right = due_r.l - 6.0;
                        let dc = if item.done { (1.0, 1.0, 1.0) } else { accent };
                        let dbg = self.brush(dc.0, dc.1, dc.2, (if item.done { 0.10 } else { 0.22 }) * a)?;
                        let drr = D2D1_ROUNDED_RECT { rect: d2r(due_r), radiusX: 5.0, radiusY: 5.0 };
                        unsafe { self.ctx.FillRoundedRectangle(&drr, &dbg) };
                        let due_col = if item.done { (1.0, 1.0, 1.0, 0.45 * a) } else { (dc.0, dc.1, dc.2, 0.95 * a) };
                        self.label(&item.due, &self.fonts.time, due_r, due_col, sh)?;
                    }

                    // Círculo vectorial con relleno y "pop" elástico al marcar.
                    let st = tv.todo_strikes.get(i).copied().unwrap_or(if item.done { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
                    let pop = match tv.todo_pop {
                        Some((idx, sc)) if idx == i => sc,
                        _ => 1.0,
                    };
                    let (ccx, ccy) = (check_r.cx(), check_r.cy());
                    let cr = 6.5;
                    self.with_local_scale(pop, ccx, ccy, || {
                        let ring_c = (
                            1.0 + (accent.0 - 1.0) * st,
                            1.0 + (accent.1 - 1.0) * st,
                            1.0 + (accent.2 - 1.0) * st,
                        );
                        let ring_br = self.brush(ring_c.0, ring_c.1, ring_c.2, (0.50 + 0.45 * st) * a)?;
                        let ell = D2D1_ELLIPSE { point: pt(ccx, ccy), radiusX: cr, radiusY: cr };
                        unsafe { self.ctx.DrawEllipse(&ell, &ring_br, 1.4, None) };
                        if st > 0.02 {
                            let fill_br = self.brush(accent.0, accent.1, accent.2, 0.95 * st * a)?;
                            let fr = cr * st;
                            let fill_ell = D2D1_ELLIPSE { point: pt(ccx, ccy), radiusX: fr, radiusY: fr };
                            unsafe { self.ctx.FillEllipse(&fill_ell, &fill_br) };
                        }
                        if st > 0.25 {
                            let ck = clamp01((st - 0.25) / 0.75);
                            let ck_br = if lum > 0.5 {
                                self.brush(0.05, 0.05, 0.07, ck * a)?
                            } else {
                                self.brush(1.0, 1.0, 1.0, ck * a)?
                            };
                            unsafe {
                                let g = self.d2d_factory.CreatePathGeometry()?;
                                let s = g.Open()?;
                                s.BeginFigure(pt(ccx - 2.8, ccy + 0.2), D2D1_FIGURE_BEGIN_HOLLOW);
                                s.AddLine(pt(ccx - 0.7, ccy + 2.4));
                                s.AddLine(pt(ccx + 3.2, ccy - 2.2));
                                s.EndFigure(D2D1_FIGURE_END_OPEN);
                                s.Close()?;
                                self.ctx.DrawGeometry(&g, &ck_br, 1.6, &check_style);
                            }
                        }
                        Ok(())
                    })?;

                    let txt_r = R::new(row.l + 26.0, row.t, text_right, row.b);
                    let txt_col = (1.0, 1.0, 1.0, (0.92 - 0.50 * st) * a);
                    self.label(&item.text, &self.fonts.artist, txt_r, txt_col, sh)?;
                    // Tachado progresivo de izquierda a derecha.
                    if st > 0.01 {
                        let max_tw = (text_right - txt_r.l).max(0.0);
                        let tw = self.measure_text_width(&item.text, &self.fonts.artist, max_tw).min(max_tw);
                        if tw > 1.0 {
                            let sy = row.cy() + 0.5;
                            let sbr = self.brush(1.0, 1.0, 1.0, 0.58 * st * a)?;
                            let srr = D2D1_ROUNDED_RECT {
                                rect: rect(txt_r.l, sy - 0.7, txt_r.l + tw * st, sy + 0.7),
                                radiusX: 0.7,
                                radiusY: 0.7,
                            };
                            unsafe { self.ctx.FillRoundedRectangle(&srr, &sbr) };
                        }
                    }

                    self.with_local_scale(bs(Btn::TodoDelete(i)), del_r.cx(), del_r.cy(), || {
                        if hv(Btn::TodoDelete(i)) {
                            let dbg = self.brush(1.0, 0.35, 0.35, 0.25 * a)?;
                            let drr = D2D1_ROUNDED_RECT { rect: d2r(del_r), radiusX: 6.0, radiusY: 6.0 };
                            unsafe { self.ctx.FillRoundedRectangle(&drr, &dbg) };
                        }
                        let del_col = if hv(Btn::TodoDelete(i)) { (1.0, 0.55, 0.55, a) } else { (1.0, 1.0, 1.0, 0.45 * a) };
                        self.label("\u{E711}", &self.fonts.icon, del_r, del_col, sh)
                    })?;
                }
                if tv.todo_total > ui::TODO_VISIBLE {
                    let track = R::new(list.r + 3.0, list.t + 2.0, list.r + 5.5, list.b - 2.0);
                    let th_bg = self.brush(1.0, 1.0, 1.0, 0.14 * a)?;
                    let trr = D2D1_ROUNDED_RECT { rect: d2r(track), radiusX: 1.25, radiusY: 1.25 };
                    unsafe { self.ctx.FillRoundedRectangle(&trr, &th_bg) };
                    let h = (track.b - track.t).max(1.0);
                    let frac = (ui::TODO_VISIBLE as f32 / tv.todo_total as f32).clamp(0.18, 1.0);
                    let thumb_h = (h * frac).max(12.0);
                    let max_off = tv.todo_total.saturating_sub(ui::TODO_VISIBLE).max(1) as f32;
                    let pos = (tv.todo_scroll as f32 / max_off).clamp(0.0, 1.0);
                    let ty = track.t + (h - thumb_h) * pos;
                    let thumb = R::new(track.l, ty, track.r, ty + thumb_h);
                    let thumb_br = self.brush(1.0, 1.0, 1.0, 0.52 * a)?;
                    let tmr = D2D1_ROUNDED_RECT { rect: d2r(thumb), radiusX: 1.25, radiusY: 1.25 };
                    unsafe { self.ctx.FillRoundedRectangle(&tmr, &thumb_br) };
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_shelf(
        &self,
        sv: &ShelfView,
        l: &Layout,
        accent: (f32, f32, f32),
        a: f32,
        sh: bool,
        hover: Option<Btn>,
        pressed: Option<(Btn, f32)>,
    ) -> Result<()> {
        let area = l.shelf;
        if !sv.tiles.is_empty() {
            let count_txt = if sv.tiles.len() == 1 {
                "1 archivo · clic derecho quita".to_string()
            } else {
                format!("{} archivos · clic derecho quita", sv.tiles.len())
            };
            let hdr_r = R::new(area.l + 4.0, area.t + 2.0, l.shelf_clear.l - 8.0, area.t + 22.0);
            self.label(&count_txt, &self.fonts.artist, hdr_r, (1.0, 1.0, 1.0, 0.52 * a), sh)?;

            // Botón "Borrar todo".
            let clr = l.shelf_clear;
            let hv_clr = hover == Some(Btn::ShelfClear);
            self.with_local_scale(btn_scale_for(pressed, Btn::ShelfClear), clr.cx(), clr.cy(), || {
                let bg = if hv_clr {
                    self.brush(1.0, 0.38, 0.38, 0.32 * a)?
                } else {
                    self.brush(1.0, 1.0, 1.0, (if sh { 0.18 } else { 0.11 }) * a)?
                };
                let rr = D2D1_ROUNDED_RECT { rect: d2r(clr), radiusX: 7.0, radiusY: 7.0 };
                unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
                let txt_col = if hv_clr { (1.0, 0.72, 0.72, a) } else { (1.0, 1.0, 1.0, 0.85 * a) };
                self.label("Borrar todo", &self.fonts.time, clr, txt_col, sh)
            })?;
        }
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
            let msg = if sv.drop_hover { "Soltá para guardar en el estante" } else { "Arrastrá archivos acá para tenerlos a mano" };
            let txt = if sv.tiles.is_empty() { zone } else { R::new(zone.l, zone.b - 22.0, zone.r, zone.b - 2.0) };
            self.label(msg, &self.fonts.lyric, txt, (r, g, b, al * a), sh)?;
        }
        let n = sv.tiles.len();
        for (i, (glyph, name)) in sv.tiles.iter().enumerate() {
            let t = l.shelf_tile(i, n);
            let entering = i < sv.new_count && sv.enter < 0.999;
            let ep = if entering { clamp01(sv.enter) } else { 1.0 };
            let sc = (0.90 + 0.10 * ep) * btn_scale_for(pressed, Btn::ShelfItem(i));
            let ta = a * (0.12 + 0.88 * ep);
            self.with_local_scale(sc, t.cx(), t.cy(), || {
                if entering {
                    let halo = self.brush(accent.0, accent.1, accent.2, 0.22 * (1.0 - ep) * a)?;
                    let hrr = D2D1_ROUNDED_RECT { rect: d2r(t.inflate(2.5 * (1.0 - ep))), radiusX: 12.0, radiusY: 12.0 };
                    let rim = self.brush(accent.0, accent.1, accent.2, 0.55 * (1.0 - ep) * a)?;
                    let trr = D2D1_ROUNDED_RECT { rect: d2r(t), radiusX: 10.0, radiusY: 10.0 };
                    unsafe {
                        self.ctx.FillRoundedRectangle(&hrr, &halo);
                        self.ctx.DrawRoundedRectangle(&trr, &rim, 1.2, None);
                    }
                }
                if hover == Some(Btn::ShelfItem(i)) {
                    let bg = self.brush(1.0, 1.0, 1.0, if sh { 0.2 } else { 0.1 } * ta)?;
                    let rr = D2D1_ROUNDED_RECT { rect: d2r(t), radiusX: 10.0, radiusY: 10.0 };
                    unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
                }
                self.label(glyph, &self.fonts.icon_big, R::new(t.l, t.t + 4.0, t.r, t.t + 38.0), (1.0, 1.0, 1.0, 0.9 * ta), sh)?;
                self.label(name, &self.fonts.tile, R::new(t.l + 3.0, t.t + 40.0, t.r - 3.0, t.b - 4.0), (1.0, 1.0, 1.0, 0.8 * ta), sh)
            })?;
        }
        Ok(())
    }

    /// Puntitos indicadores de página al pie del panel expandido.
    #[allow(clippy::too_many_arguments)]
    fn draw_page_dots(
        &self,
        l: &Layout,
        active: usize,
        accent: (f32, f32, f32),
        a: f32,
        sh: bool,
        hover: Option<Btn>,
        pressed: Option<(Btn, f32)>,
    ) -> Result<()> {
        for i in 0..l.dots_count {
            let r = l.page_dot_rect(i, active);
            let is_cur = i == active;
            let is_hv = hover == Some(Btn::PageDot(i));
            self.with_local_scale(btn_scale_for(pressed, Btn::PageDot(i)), r.cx(), r.cy(), || {
                if sh {
                    let s = self.brush(0.0, 0.0, 0.0, 0.45 * a)?;
                    let sr = D2D1_ROUNDED_RECT { rect: d2r(r.inflate(1.0)), radiusX: 3.5, radiusY: 3.5 };
                    unsafe { self.ctx.FillRoundedRectangle(&sr, &s) };
                }
                let br = if is_cur {
                    self.brush(accent.0, accent.1, accent.2, 0.95 * a)?
                } else {
                    self.brush(1.0, 1.0, 1.0, (if is_hv { 0.65 } else { 0.28 }) * a)?
                };
                let rr = D2D1_ROUNDED_RECT { rect: d2r(r), radiusX: 2.5, radiusY: 2.5 };
                unsafe { self.ctx.FillRoundedRectangle(&rr, &br) };
                Ok(())
            })?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_mixer(
        &self,
        mv: &MixerView,
        l: &Layout,
        accent: (f32, f32, f32),
        a: f32,
        sh: bool,
        hover: Option<Btn>,
        pressed: Option<(Btn, f32)>,
    ) -> Result<()> {
        if mv.rows.is_empty() {
            return self.label("No encontré la salida de audio", &self.fonts.lyric, l.mixer, (1.0, 1.0, 1.0, 0.55 * a), sh);
        }
        for (i, (name, level, muted)) in mv.rows.iter().enumerate().take(l.mixer_n) {
            let row = l.mixer_row(i);
            let hot = mv.drag == Some(i) || (mv.drag.is_none() && matches!(hover, Some(Btn::MixerBar(j) | Btn::MixerMute(j)) if j == i));
            if hot {
                let bg = self.brush(1.0, 1.0, 1.0, if sh { 0.16 } else { 0.07 } * a)?;
                let rr = D2D1_ROUNDED_RECT { rect: d2r(row), radiusX: 8.0, radiusY: 8.0 };
                unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
            }
            let icon = R::new(row.l + 2.0, row.t, row.l + 30.0, row.b);
            let glyph = if *muted { G_MUTE } else { G_VOLUME };
            let ia = if *muted { 0.45 } else { 0.85 };
            self.with_local_scale(btn_scale_for(pressed, Btn::MixerMute(i)), icon.cx(), icon.cy(), || {
                self.label(glyph, &self.fonts.icon, icon, (1.0, 1.0, 1.0, ia * a), sh)
            })?;
            let name_r = R::new(row.l + 32.0, row.t, row.l + 128.0, row.b);
            let na = if i == 0 { 0.95 } else { 0.8 };
            self.label(name, &self.fonts.artist, name_r, (1.0, 1.0, 1.0, na * a), sh)?;
            let bar = l.mixer_bar(i);
            let v = level.clamp(0.0, 1.0);
            let c = if i == 0 { accent } else { BLUE };
            self.bar(bar, v, c, a)?;
            if hot {
                let x = bar.l + (bar.r - bar.l) * v;
                let knob = self.brush(1.0, 1.0, 1.0, 0.95 * a)?;
                let rad = if mv.drag == Some(i) { 7.0 } else { 5.5 };
                let e = D2D1_ELLIPSE { point: pt(x, bar.cy()), radiusX: rad, radiusY: rad };
                unsafe { self.ctx.FillEllipse(&e, &knob) };
            }
            let pct = format!("{}%", (v * 100.0).round() as i32);
            self.label(&pct, &self.fonts.small, R::new(bar.r + 6.0, row.t, row.r, row.b), (1.0, 1.0, 1.0, 0.7 * a), sh)?;
        }
        Ok(())
    }

    fn draw_clip(
        &self,
        cv: &ClipView,
        area: R,
        a: f32,
        sh: bool,
        hover: Option<Btn>,
        pressed: Option<(Btn, f32)>,
    ) -> Result<()> {
        if cv.items.is_empty() {
            return self.label("Copiá un texto y aparece acá", &self.fonts.lyric, area, (1.0, 1.0, 1.0, 0.55 * a), sh);
        }
        for (i, item) in cv.items.iter().take(ui::CLIP_ROWS).enumerate() {
            let t = area.t + 6.0 + i as f32 * ui::CLIP_ROW_H;
            let row = R::new(area.l, t, area.r, t + ui::CLIP_ROW_H - 2.0);
            self.with_local_scale(btn_scale_for(pressed, Btn::ClipItem(i)), row.cx(), row.cy(), || {
                if hover == Some(Btn::ClipItem(i)) {
                    let bg = self.brush(1.0, 1.0, 1.0, if sh { 0.2 } else { 0.1 } * a)?;
                    let rr = D2D1_ROUNDED_RECT { rect: d2r(row), radiusX: 8.0, radiusY: 8.0 };
                    unsafe { self.ctx.FillRoundedRectangle(&rr, &bg) };
                }
                let text_r = R::new(row.l + 10.0, row.t, row.r - 10.0, row.b);
                if cv.copied == Some(i) {
                    self.label("✓ Copiado", &self.fonts.artist, text_r, (0.55, 0.9, 0.6, a), sh)
                } else {
                    let one_line: String = item.chars().take(300).map(|c| if c.is_control() { ' ' } else { c }).collect();
                    self.label(one_line.trim(), &self.fonts.artist, text_r, (1.0, 1.0, 1.0, 0.85 * a), sh)
                }
            })?;
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
                    // Ventana de 3 líneas (anterior tenue, actual en negrita, siguiente tenue) con desplazamiento suave.
                    let lo = (*scroll - 2.2).max(0.0) as usize;
                    let hi = ((*scroll + 3.2).max(0.0) as usize).min(lines.len());
                    for i in lo..hi {
                        let d = (i as f32 - *scroll).abs();
                        if d > 1.7 {
                            continue;
                        }
                        let focus = clamp01(1.0 - d);
                        let edge = clamp01(1.0 - (d - 1.0).max(0.0) / 0.68);
                        let hov = hover == Some(Btn::LyricLine(i));
                        let al = (if hov { 0.88 } else { 0.40 + 0.60 * focus }) * edge;
                        if al <= 0.01 {
                            continue;
                        }
                        let r = line_rect(i, *scroll);
                        let cur = i == *current || focus > 0.55;
                        let font = if cur { &self.fonts.lyric_cur } else { &self.fonts.lyric };
                        let sc = 0.94 + 0.06 * focus;
                        self.with_local_scale(sc, r.cx(), r.cy(), || {
                            self.label(&lines[i].1, font, r, (1.0, 1.0, 1.0, al * a), sh)
                        })?;
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
                    s.BeginFigure(pt(ex - 4.0, ey + 2.0), D2D1_FIGURE_BEGIN_HOLLOW);
                    s.AddLine(pt(ex, ey - 2.5));
                    s.AddLine(pt(ex + 4.0, ey + 2.0));
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
                    let hh = (4.5 * eyes.open * eyes.scale).max(0.9);
                    let rr = D2D1_ROUNDED_RECT { rect: rect(ex - 3.0, ey - hh, ex + 3.0, ey + hh), radiusX: 3.0, radiusY: hh.min(3.0) };
                    self.ctx.FillRoundedRectangle(&rr, &iris);
                },
                _ => unsafe {
                    let (rx, ry) = (3.6 * eyes.scale, 6.0 * eyes.scale);
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

    /// Carátula con fundido: la anterior se desvanece mientras aparece la nueva.
    fn draw_art(&self, r: R, radius: f32, a: f32) -> Result<()> {
        let t = self.art_fade();
        if t < 1.0 {
            if let Some(prev) = &self.prev_art {
                self.draw_bitmap(prev, r, radius, a * (1.0 - t))?;
            }
        }
        match &self.art {
            Some(bmp) => self.draw_bitmap(bmp, r, radius, a * t.max(if self.prev_art.is_some() { 0.0 } else { t })),
            None => Ok(()),
        }
    }

    fn draw_bitmap(&self, bmp: &ID2D1Bitmap1, r: R, radius: f32, a: f32) -> Result<()> {
        if a <= 0.01 {
            return Ok(());
        }
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

    #[allow(clippy::too_many_arguments)]
    fn button(&self, glyph: &str, fmt: &IDWriteTextFormat, r: R, a: f32, hover: bool, sh: bool, scale: f32) -> Result<()> {
        self.with_local_scale(scale, r.cx(), r.cy(), || {
            if hover {
                let bg = self.brush(1.0, 1.0, 1.0, if sh { 0.22 } else { 0.14 } * a)?;
                let e = D2D1_ROUNDED_RECT { rect: d2r(r), radiusX: 14.0, radiusY: 14.0 };
                unsafe { self.ctx.FillRoundedRectangle(&e, &bg) };
            }
            self.label(glyph, fmt, r, (1.0, 1.0, 1.0, a), sh)
        })
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
    /// Píldora flotante: rectángulo con las 4 esquinas redondeadas (sin orejas).
    fn pill_geometry(&self, cx: f32, w: f32, h: f32, radius: f32) -> Result<ID2D1PathGeometry1> {
        let h = h.max(1.0);
        let r = if h <= 40.0 { h / 2.0 } else { radius.max(16.0) }.min(h / 2.0).min(w / 2.0).max(0.0);
        let l = cx - w / 2.0;
        let rt = cx + w / 2.0;
        unsafe {
            let geo = self.d2d_factory.CreatePathGeometry()?;
            let sink = geo.Open()?;
            sink.BeginFigure(pt(l, r), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(pt(l, h - r));
            sink.AddArc(&arc(pt(l + r, h), r));
            sink.AddLine(pt(rt - r, h));
            sink.AddArc(&arc(pt(rt, h - r), r));
            sink.AddLine(pt(rt, r));
            sink.AddArc(&arc(pt(rt - r, 0.0), r));
            sink.AddLine(pt(l + r, 0.0));
            sink.AddArc(&arc(pt(l, r), r));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            Ok(geo)
        }
    }

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

pub(crate) fn font_exists(dw: &IDWriteFactory, family: &str) -> bool {
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

pub(crate) fn tf(dw: &IDWriteFactory, family: &str, size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
    unsafe {
        let fam: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
        let f = dw.CreateTextFormat(PCWSTR(fam.as_ptr()), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("es-AR"))?;
        f.SetTextAlignment(align)?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(f)
    }
}

/// Aviva un color de carátula para que las barras se vean sobre el negro.
fn boost(c: (f32, f32, f32)) -> (f32, f32, f32) {
    let m = c.0.max(c.1).max(c.2).max(0.05);
    let k = (0.85 / m).max(1.0);
    ((c.0 * k).min(1.0), (c.1 * k).min(1.0), (c.2 * k).min(1.0))
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

fn arc_cw(end: Vector2, r: f32) -> D2D1_ARC_SEGMENT {
    D2D1_ARC_SEGMENT {
        point: end,
        size: D2D_SIZE_F { width: r, height: r },
        rotationAngle: 0.0,
        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
        arcSize: D2D1_ARC_SIZE_SMALL,
    }
}

/// Composición de matrices afines Direct2D (vectores fila: `v * a * b`).
fn mul_m3x2(a: Matrix3x2, b: Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}

fn btn_scale_for(pressed: Option<(Btn, f32)>, btn: Btn) -> f32 {
    match pressed {
        Some((b, s)) if b == btn => s,
        _ => 1.0,
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

fn tail_fitting(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let tail: String = s.chars().skip(count - max_chars).collect();
        format!("…{tail}")
    }
}

/// Color predominante calculado directamente sobre el buffer BGRA premultiplicado ya reducido.
fn dominant_from_pbgra(px: &[u8]) -> Option<(f32, f32, f32)> {
    let (mut r, mut g, mut b, mut wsum) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    // Muestreamos 1 de cada 4 píxeles (~1024 muestras en 64×64): < 10 µs.
    for p in px.chunks_exact(4).step_by(4) {
        let a = p[3] as f32 / 255.0;
        if a < 0.05 {
            continue;
        }
        let inv = 1.0 / (255.0 * a);
        let pb = (p[0] as f32 * inv).min(1.0);
        let pg = (p[1] as f32 * inv).min(1.0);
        let pr = (p[2] as f32 * inv).min(1.0);
        let mx = pr.max(pg).max(pb);
        let mn = pr.min(pg).min(pb);
        let sat = if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        let w = (0.05 + sat * sat * mx) * a;
        r += pr * w;
        g += pg * w;
        b += pb * w;
        wsum += w;
    }
    if wsum <= 1e-4 {
        return None;
    }
    let (r, g, b) = (r / wsum, g / wsum, b / wsum);
    let mx = r.max(g).max(b).max(0.05);
    let k = 0.95 / mx;
    Some(((r * k).min(1.0), (g * k).min(1.0), (b * k).min(1.0)))
}
