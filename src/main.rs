//! Esme — barra dinámica para Windows 10/11.
//! Fase 1: ventana transparente, render GPU (Direct2D + DirectComposition), hover y bandeja.
//! Fase 2: multimedia (SMTC), letras sincronizadas (LRCLIB) y mascota animada.

#![windows_subsystem = "windows"]

// Recursos de Windows (ícono, manifiesto, versión) generados por tools/build-res.ps1.
#[link(name = "esme_res", kind = "static", modifiers = "+whole-archive")]
extern "C" {}

mod anim;
mod clipboard;
mod config;
mod fullscreen;
mod http;
mod lyrics;
mod mascot;
mod media;
mod render;
mod tray;
mod ui;

use anim::{clamp01, Spring};
use config::Config;
use lyrics::Lyrics;
use mascot::{Mascot, Mood};
use media::{Cmd, MediaInfo};
use render::{ClipView, Collapsed, Expanded, Frame, LyricsView, MediaView, Renderer};
use std::cell::RefCell;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tray::*;
use ui::{Btn, Dims, Layout, CANVAS_H, CANVAS_W};
use windows::core::{w, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, TrackMouseEvent, LASTINPUTINFO, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_MOUSELEAVE: u32 = 0x02A3;
const NIN_SELECT: u32 = WM_USER;
const NIN_KEYSELECT: u32 = WM_USER + 1;
const WM_LYRICS: u32 = WM_APP + 3;

const T_EXPAND: usize = 1;
const T_COLLAPSE: usize = 2;
const T_CLOCK: usize = 3;
const T_TRIM: usize = 4;
const T_PROGRESS: usize = 5;
const T_LYRIC: usize = 6;
const T_BLINK: usize = 7;
const T_IDLE: usize = 8;
const T_GREET: usize = 9;
const T_BLINK_STEP: usize = 10;
/// Respaldo de WM_MOUSELEAVE (Windows a veces no lo manda si la región cambia): solo durante el hover.
const T_HOVERCHECK: usize = 11;
const T_FS: usize = 12;
const T_COPIED: usize = 13;
const WM_CLIPBOARDUPDATE: u32 = 0x031D;
const WM_APPBAR: u32 = WM_APP + 4;
const ABN_FULLSCREENAPP: usize = 2;

/// Ventana principal, para los callbacks de eventos de Windows.
static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Windows avisa: cambió la ventana en primer plano o se minimizó/restauró algo.
/// Agrupamos ráfagas de eventos con un timer corto y recién ahí revisamos pantallas completas.
unsafe extern "system" fn on_win_event(
    _: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    let h = HWND(MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed) as *mut _);
    SetTimer(Some(h), T_FS, 120, None);
}

const EXPAND_DELAY_MS: u32 = 90;
const COLLAPSE_DELAY_MS: u32 = 320;
const TRIM_DELAY_MS: u32 = 1500;
const IDLE_CHECK_AWAKE_MS: u32 = 15_000;
const IDLE_CHECK_ASLEEP_MS: u32 = 1_000;

struct App {
    hwnd: HWND,
    renderer: Option<Renderer>,
    tray: Tray,
    cfg: Config,
    // Animaciones.
    sw: Spring,
    sh: Spring,
    expand: Spring,
    mascot_vis: Spring,
    lyric_scroll: Spring,
    last_tick: i64,
    qpf: i64,
    trim_pending: bool,
    // Interacción.
    hover: bool,
    pinned: bool,
    hover_btn: Option<Btn>,
    // Ventana.
    monitor: HMONITOR,
    hidden: bool,
    scale: f32,
    win_w: i32,
    win_h: i32,
    last_region: (i32, i32, i32),
    taskbar_created: u32,
    // Reloj.
    time_text: String,
    date_text: String,
    // Multimedia.
    media_tx: Sender<Cmd>,
    media_shared: media::Shared,
    media: Option<MediaInfo>,
    progress_timer: bool,
    // Letras.
    lyrics_on: bool,
    lyrics: Option<Lyrics>,
    lyrics_key: String,
    lyrics_req: u64,
    lyrics_shared: Arc<Mutex<Option<(u64, Lyrics)>>>,
    lyric_current: Option<usize>,
    // Portapapeles.
    clip: clipboard::History,
    clip_on: bool,
    clip_copied: Option<usize>,
    // Mascota.
    mascot: Mascot,
    /// Última mascota elegida (para el fundido al desactivarla).
    last_pet: config::Pet,
    blink_timer: bool,
}

/// Contadores de diagnóstico (se vuelcan a noosa-debug.txt en TEMP si existe NOOSA_DEBUG).
static DBG_FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DBG_MEDIA: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Ejecuta `f` con el estado de la app si no está prestado (evita pánicos por reentrada).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn main() -> Result<()> {
    unsafe {
        // Una sola instancia.
        let _mutex = CreateMutexW(None, true, w!("Local\\EsmeSingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let hinst = GetModuleHandleW(None)?;
        let class = w!("EsmeNotch");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("Esme"),
            WS_POPUP,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;

        config::migrate_from_noosa();
        let cfg = Config::load();
        let mut qpf = 0i64;
        let _ = QueryPerformanceFrequency(&mut qpf);
        let (media_tx, media_shared) = media::start(hwnd);
        let start = if cfg.notch_minimo { ui::MINIMAL } else { ui::PILL };
        let app = App {
            hwnd,
            renderer: None,
            tray: Tray::new(hwnd),
            sw: Spring::new(start.w, 380.0, 0.28).with_eps(0.05),
            sh: Spring::new(start.h, 380.0, 0.28).with_eps(0.05),
            expand: Spring::new(0.0, 380.0, 0.28),
            mascot_vis: Spring::new(0.0, 200.0, 0.1).with_eps(0.002),
            lyric_scroll: Spring::new(0.0, 220.0, 0.0).with_eps(0.002),
            cfg,
            last_tick: 0,
            qpf,
            trim_pending: false,
            hover: false,
            pinned: false,
            hover_btn: None,
            monitor: MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
            hidden: false,
            scale: 1.0,
            win_w: 0,
            win_h: 0,
            last_region: (-1, -1, -1),
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            time_text: String::new(),
            date_text: String::new(),
            media_tx,
            media_shared,
            media: None,
            progress_timer: false,
            lyrics_on: false,
            lyrics: None,
            lyrics_key: String::new(),
            lyrics_req: 0,
            lyrics_shared: Arc::new(Mutex::new(None)),
            lyric_current: None,
            clip: clipboard::History::load(),
            clip_on: false,
            clip_copied: None,
            mascot: Mascot::new(),
            last_pet: config::Pet::Esme,
            blink_timer: false,
        };
        APP.with(|a| *a.borrow_mut() = Some(app));

        with_app(|a| {
            a.update_clock();
            a.layout();
            a.retarget();
            a.redraw();
            a.schedule_trim();
            SetTimer(Some(a.hwnd), T_IDLE, IDLE_CHECK_AWAKE_MS, None);
        });
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        // Modo juego / pantalla completa: eventos de Windows + aviso del shell (sin polling).
        MAIN_HWND.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
        use windows::Win32::UI::Accessibility::SetWinEventHook;
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let _ = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_win_event), 0, 0, flags);
        let _ = SetWinEventHook(EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND, None, Some(on_win_event), 0, 0, flags);
        let mut abd = windows::Win32::UI::Shell::APPBARDATA {
            cbSize: std::mem::size_of::<windows::Win32::UI::Shell::APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: WM_APPBAR,
            ..Default::default()
        };
        windows::Win32::UI::Shell::SHAppBarMessage(windows::Win32::UI::Shell::ABM_NEW, &mut abd);
        with_app(|a| a.check_fullscreen());
        let _ = windows::Win32::System::DataExchange::AddClipboardFormatListener(hwnd);

        run_loop();
        APP.with(|a| a.borrow_mut().take());
        Ok(())
    }
}

/// Bucle de mensajes orientado a eventos: bloquea en GetMessage (0% CPU) salvo
/// mientras hay una animación, donde Present(vsync) marca el ritmo de los frames.
fn run_loop() {
    let mut msg = MSG::default();
    unsafe {
        loop {
            let animating = with_app(|a| a.is_animating()).unwrap_or(false);
            if animating {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                with_app(|a| a.tick());
            } else {
                let r = GetMessageW(&mut msg, None, 0, 0);
                if r.0 <= 0 {
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

impl App {
    // ---------- Estado derivado ----------

    fn has_media(&self) -> bool {
        self.media.as_ref().map(|m| !m.title.is_empty()).unwrap_or(false)
    }

    fn playing(&self) -> bool {
        self.media.as_ref().map(|m| m.playing && !m.title.is_empty()).unwrap_or(false)
    }

    fn synced_lines(&self) -> Option<&[(f64, String)]> {
        match &self.lyrics {
            Some(Lyrics::Synced(v)) => Some(v),
            _ => None,
        }
    }

    fn want_expanded(&self) -> bool {
        self.hover || self.pinned
    }

    fn lyric_pill(&self) -> bool {
        self.lyrics_on && self.has_media() && self.synced_lines().is_some()
    }

    fn mascot_shown(&self) -> bool {
        self.cfg.pet().is_some() && !self.cfg.notch_minimo && !self.lyric_pill() && !self.playing()
    }

    fn collapsed_dims(&self) -> Dims {
        if self.lyric_pill() {
            ui::PILL_LYRIC
        } else if self.cfg.notch_minimo {
            ui::MINIMAL
        } else if self.playing() {
            ui::PILL_MEDIA
        } else {
            ui::PILL
        }
    }

    fn expanded_dims(&self) -> Dims {
        let mut d = self.base_expanded_dims();
        if self.clip_on {
            d.h += ui::clip_extra(self.clip.items.len());
            d.w = d.w.max(360.0);
        }
        d
    }

    fn base_expanded_dims(&self) -> Dims {
        if self.has_media() {
            let mut d = ui::EXP_MEDIA;
            if self.lyrics_on {
                d.h += match self.lyrics {
                    Some(Lyrics::Synced(_)) | Some(Lyrics::Plain(_)) => ui::LYRICS_EXTRA,
                    _ => ui::LYRICS_MSG_EXTRA,
                };
            }
            d
        } else {
            ui::EXP_CLOCK
        }
    }

    fn content_top(&self) -> f32 {
        if self.cfg.transparente { self.collapsed_dims().h + ui::FLOAT_GAP } else { 0.0 }
    }

    fn layout_now(&self) -> Layout {
        Layout::new(CANVAS_W / 2.0, self.content_top(), self.expanded_dims(), self.has_media(), self.lyrics_on, self.clip_on)
    }

    fn is_animating(&self) -> bool {
        !(self.sw.is_settled()
            && self.sh.is_settled()
            && self.expand.is_settled()
            && self.mascot_vis.is_settled()
            && self.lyric_scroll.is_settled())
    }

    // ---------- Animación ----------

    fn now(&self) -> i64 {
        let mut t = 0i64;
        unsafe {
            let _ = QueryPerformanceCounter(&mut t);
        }
        t
    }

    /// Recalcula a dónde tiene que ir cada animación según el estado actual.
    fn retarget(&mut self) {
        let was = self.is_animating();
        let open = self.want_expanded();
        let shape = if open && !self.cfg.transparente { self.expanded_dims() } else { self.collapsed_dims() };
        if (self.expand.target > 0.5) != open {
            self.trim_pending = true; // recortamos memoria solo tras animaciones de hover
        }
        self.sw.set_target(shape.w);
        self.sh.set_target(shape.h);
        self.expand.set_target(if open { 1.0 } else { 0.0 });
        self.mascot_vis.set_target(if self.mascot_shown() { 1.0 } else { 0.0 });
        if !was {
            self.last_tick = self.now();
        }
        self.update_timers();
    }

    fn tick(&mut self) {
        let now = self.now();
        let dt = (((now - self.last_tick) as f64 / self.qpf as f64) as f32).clamp(0.0, 1.0 / 20.0);
        self.last_tick = now;
        for s in [&mut self.sw, &mut self.sh, &mut self.expand, &mut self.mascot_vis, &mut self.lyric_scroll] {
            s.step(dt);
        }
        self.redraw_with(true);
        if !self.is_animating() && self.trim_pending {
            self.trim_pending = false;
            self.schedule_trim();
        }
    }

    // ---------- Ventana y dibujo ----------

    /// Ubica la ventana arriba al centro del monitor principal y ajusta el lienzo al DPI.
    fn layout(&mut self) {
        unsafe {
            let mon = self.monitor;
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            self.scale = dx as f32 / 96.0;
            self.win_w = (CANVAS_W * self.scale).ceil() as i32;
            self.win_h = (CANVAS_H * self.scale).ceil() as i32;
            let r = mi.rcMonitor;
            let x = r.left + ((r.right - r.left) - self.win_w) / 2;
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, r.top, self.win_w, self.win_h, SWP_NOACTIVATE | SWP_SHOWWINDOW);

            if let Some(rd) = self.renderer.as_mut() {
                if rd.resize(self.win_w as u32, self.win_h as u32, self.scale).is_err() {
                    self.renderer = None;
                }
            }
            if self.renderer.is_none() {
                self.create_renderer();
            }
            self.last_region = (-1, -1, -1);
        }
    }

    fn create_renderer(&mut self) {
        self.renderer = Renderer::new(self.hwnd, self.win_w as u32, self.win_h as u32, self.scale).ok();
        if let (Some(r), Some(m)) = (self.renderer.as_mut(), self.media.as_ref()) {
            r.set_art(m.thumb.clone());
        }
    }

    /// La región de la ventana sigue a lo visible: fuera de ella los clics y el hover
    /// pasan a las ventanas de abajo.
    fn update_region(&mut self, shape_w: f32, shape_h: f32, ear: f32) {
        let cx = CANVAS_W / 2.0;
        let mut l = cx - shape_w / 2.0 - ear;
        let mut r = cx + shape_w / 2.0 + ear;
        let mut b = shape_h;
        if self.cfg.transparente && (self.expand.value > 0.02 || self.expand.target > 0.5) {
            let area = self.layout_now().area;
            l = l.min(area.l);
            r = r.max(area.r);
            b = b.max(area.b);
        }
        let key = ((l * self.scale).floor() as i32, (r * self.scale).ceil() as i32, (b * self.scale).ceil() as i32 + 1);
        if key == self.last_region {
            return;
        }
        self.last_region = key;
        unsafe {
            let rgn = CreateRectRgn(key.0, 0, key.1, key.2);
            let _ = SetWindowRgn(self.hwnd, Some(rgn), false);
        }
    }

    fn current_line_text(&self) -> String {
        match (self.synced_lines(), self.lyric_current) {
            (Some(lines), Some(i)) => lines.get(i).map(|l| l.1.clone()).unwrap_or_default(),
            (Some(_), None) => "♪".into(),
            _ => String::new(),
        }
    }

    fn redraw_with(&mut self, vsync: bool) {
        DBG_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let w = self.sw.value.max(4.0);
        let h = self.sh.value.max(2.0);
        let radius = (h * 0.42).clamp(2.5, 30.0);
        let ear = (h * 0.28).clamp(2.5, 14.0);
        self.update_region(w, h, ear);

        let line = self.current_line_text();
        let eyes = self.mascot.eyes();
        let collapsed = if self.lyric_pill() {
            Collapsed::Lyric(&line)
        } else if self.cfg.notch_minimo {
            Collapsed::Minimal
        } else if self.playing() {
            Collapsed::Media { time: &self.time_text }
        } else if self.cfg.pet().is_some() || self.mascot_vis.value > 0.01 {
            Collapsed::Mascot { time: &self.time_text, eyes, pet: self.cfg.pet().unwrap_or(self.last_pet) }
        } else {
            Collapsed::Clock(&self.time_text)
        };

        let (pos_s, dur_s);
        let expanded = match &self.media {
            Some(m) if !m.title.is_empty() => {
                let pos = m.position_now();
                pos_s = ui::fmt_time(pos);
                dur_s = ui::fmt_time(m.duration);
                let lyrics = if self.lyrics_on {
                    Some(match &self.lyrics {
                        None | Some(Lyrics::Loading) => LyricsView::Message("Buscando letra…"),
                        Some(Lyrics::NotFound) => LyricsView::Message("No encontré la letra de esta canción"),
                        Some(Lyrics::Error) => LyricsView::Message("Sin conexión con lrclib.net"),
                        Some(Lyrics::Synced(lines)) => LyricsView::Synced {
                            lines,
                            scroll: self.lyric_scroll.value,
                            current: self.lyric_current.unwrap_or(usize::MAX),
                        },
                        Some(Lyrics::Plain(lines)) => LyricsView::Plain { lines, scroll: self.lyric_scroll.value },
                    })
                } else {
                    None
                };
                Expanded::Media(MediaView {
                    title: &m.title,
                    artist: &m.artist,
                    playing: m.playing,
                    can_prev: m.can_prev,
                    can_next: m.can_next,
                    progress: if m.duration > 0.5 { Some((pos / m.duration) as f32) } else { None },
                    pos: pos_s,
                    dur: dur_s,
                    lyrics,
                })
            }
            _ => Expanded::Clock { time: &self.time_text, date: &self.date_text },
        };

        let frame = Frame {
            cx: CANVAS_W / 2.0,
            shape_w: w,
            shape_h: h,
            radius,
            ear,
            expand: self.expand.value,
            transparent: self.cfg.transparente,
            collapsed,
            expanded,
            layout: self.layout_now(),
            hover: self.hover_btn,
            mascot_vis: self.mascot_vis.value,
            clip: if self.clip_on { Some(ClipView { items: &self.clip.items, copied: self.clip_copied }) } else { None },
        };
        let ok = match self.renderer.as_mut() {
            Some(r) => r.draw(&frame, vsync).is_ok(),
            None => false,
        };
        if !ok {
            // Dispositivo perdido (driver reiniciado, etc.): se recrea.
            self.create_renderer();
        }
    }

    fn redraw(&mut self) {
        self.redraw_with(false);
    }

    /// Juego o video a pantalla completa en el monitor del notch: nos vamos al otro
    /// monitor (si la opción está activa y hay uno libre) o nos escondemos.
    fn check_fullscreen(&mut self) {
        let games: Vec<String> = fullscreen::JUEGOS_BASE
            .iter()
            .map(|s| s.to_string())
            .chain(self.cfg.juegos_extra.iter().map(|s| s.trim().to_lowercase()))
            .collect();
        let full = fullscreen::fullscreen_monitors(self.hwnd, &games);
        let mons = fullscreen::monitors();
        let Some(&primary) = mons.first() else { return };
        let is_full = |m: &HMONITOR| full.contains(&(m.0 as isize));
        let target = if !is_full(&primary) {
            Some(primary)
        } else if self.cfg.mover_monitor {
            mons.iter().skip(1).find(|m| !is_full(m)).copied()
        } else {
            None
        };
        unsafe {
            match target {
                None => {
                    if !self.hidden {
                        self.hidden = true;
                        self.hover = false;
                        self.hover_btn = None;
                        let _ = ShowWindow(self.hwnd, SW_HIDE);
                        self.retarget();
                    }
                }
                Some(m) => {
                    let moved = m.0 != self.monitor.0;
                    if moved {
                        self.monitor = m;
                        self.layout();
                    }
                    if self.hidden {
                        self.hidden = false;
                        let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                    }
                    if moved || !self.is_animating() {
                        self.redraw();
                    }
                }
            }
        }
    }

    /// Redibuja solo si no hay una animación en curso (que ya está dibujando).
    fn refresh(&mut self) {
        if self.hidden {
            return;
        }
        if !self.is_animating() {
            self.redraw();
        }
    }

    /// En reposo devolvemos al sistema las páginas que no se usan (sobre todo las del
    /// driver de la GPU). Vuelven solas en la próxima animación.
    fn schedule_trim(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), T_TRIM, TRIM_DELAY_MS, None);
        }
    }

    // ---------- Timers ----------

    fn update_timers(&mut self) {
        unsafe {
            // Barra de progreso: solo con el panel abierto y algo sonando.
            let need_progress = self.playing() && self.want_expanded();
            if need_progress != self.progress_timer {
                self.progress_timer = need_progress;
                if need_progress {
                    SetTimer(Some(self.hwnd), T_PROGRESS, 500, None);
                } else {
                    let _ = KillTimer(Some(self.hwnd), T_PROGRESS);
                }
            }
            // Pestañeo: solo si la mascota está a la vista y despierta.
            let need_blink = self.mascot_shown() && !self.want_expanded() && self.mascot.mood == Mood::Awake;
            if need_blink && !self.blink_timer {
                SetTimer(Some(self.hwnd), T_BLINK, self.mascot.next_blink_ms(), None);
            } else if !need_blink && self.blink_timer {
                let _ = KillTimer(Some(self.hwnd), T_BLINK);
            }
            self.blink_timer = need_blink;
        }
        self.schedule_lyric();
    }

    /// Programa un timer exacto para el cambio de la próxima línea de la letra.
    fn schedule_lyric(&mut self) {
        let hwnd = self.hwnd;
        let pos = self.media.as_ref().map(|m| m.position_now()).unwrap_or(0.0);
        let cur = self.synced_lines().map(|lines| lines.iter().rposition(|l| l.0 <= pos + 0.12));
        let cur = cur.flatten();
        if cur != self.lyric_current {
            self.lyric_current = cur;
            let was = self.is_animating();
            self.lyric_scroll.set_target(cur.unwrap_or(0) as f32);
            if !was {
                self.last_tick = self.now();
            }
        }
        let next = match (self.synced_lines(), self.lyrics_on && self.playing()) {
            (Some(lines), true) => lines.iter().find(|l| l.0 > pos + 0.12).map(|l| l.0),
            _ => None,
        };
        unsafe {
            match next {
                Some(t) => {
                    let ms = (((t - 0.12 - pos) * 1000.0).max(15.0)) as u32;
                    SetTimer(Some(hwnd), T_LYRIC, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(hwnd), T_LYRIC);
                }
            }
        }
    }

    fn update_clock(&mut self) {
        const DIAS: [&str; 7] = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];
        const MESES: [&str; 12] = [
            "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre",
            "diciembre",
        ];
        unsafe {
            let t = GetLocalTime();
            self.time_text = format!("{:02}:{:02}", t.wHour, t.wMinute);
            self.date_text = format!(
                "{}, {} de {}",
                DIAS[(t.wDayOfWeek % 7) as usize],
                t.wDay,
                MESES[((t.wMonth.max(1) - 1) % 12) as usize]
            );
            // Próximo disparo justo al cambiar el minuto: un solo timer por minuto.
            let ms = (60 - t.wSecond as u32) * 1000 - t.wMilliseconds as u32 + 30;
            SetTimer(Some(self.hwnd), T_CLOCK, ms, None);
        }
    }

    fn on_timer(&mut self, id: usize) {
        unsafe {
            if id != T_PROGRESS && id != T_HOVERCHECK {
                let _ = KillTimer(Some(self.hwnd), id);
            }
        }
        match id {
            T_EXPAND if self.hover => self.retarget(),
            T_COLLAPSE if !self.hover => {
                self.hover_btn = None;
                self.retarget();
            }
            T_CLOCK => {
                self.update_clock();
                // Contraído en modo mínimo el reloj no se ve: no hay nada que redibujar.
                let visible = !(self.cfg.notch_minimo && !self.lyric_pill() && self.expand.value == 0.0);
                if visible {
                    self.refresh();
                }
            }
            T_TRIM if !self.is_animating() => unsafe {
                let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
            },
            T_PROGRESS => self.refresh(),
            T_LYRIC => {
                self.schedule_lyric();
                self.refresh();
            }
            T_BLINK => {
                self.blink_timer = false;
                self.update_gaze();
                if self.mascot.blink() {
                    unsafe { SetTimer(Some(self.hwnd), T_BLINK_STEP, mascot::BLINK_STEP_MS, None) };
                    self.refresh();
                }
                self.update_timers();
            }
            T_BLINK_STEP => {
                if self.mascot.blink_advance() {
                    unsafe { SetTimer(Some(self.hwnd), T_BLINK_STEP, mascot::BLINK_STEP_MS, None) };
                }
                self.refresh();
            }
            T_IDLE => self.check_idle(),
            T_FS => self.check_fullscreen(),
            T_COPIED => {
                self.clip_copied = None;
                self.refresh();
            }
            T_HOVERCHECK => {
                if !self.cursor_inside() {
                    self.on_mouse_leave();
                }
            }
            T_GREET => {
                self.mascot.end_greet();
                self.update_timers();
                self.refresh();
            }
            _ => {}
        }
    }

    /// Mascota: se duerme tras unos minutos sin usar la PC y saluda al volver.
    /// Un chequeo barato cada 15 s (despierta) o cada 1 s (dormida).
    fn check_idle(&mut self) {
        if std::env::var_os("NOOSA_DEBUG").is_some() {
            use std::io::Write;
            use std::sync::atomic::Ordering::Relaxed;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("esme-debug.txt")) {
                let _ = writeln!(
                    f,
                    "frames={} media={} anim={} sw={:?} sh={:?} ex={:?} mv={:?} ls={:?}",
                    DBG_FRAMES.load(Relaxed),
                    DBG_MEDIA.load(Relaxed),
                    self.is_animating(),
                    (self.sw.value, self.sw.target, self.sw.velocity),
                    (self.sh.value, self.sh.target, self.sh.velocity),
                    (self.expand.value, self.expand.target),
                    (self.mascot_vis.value, self.mascot_vis.target),
                    (self.lyric_scroll.value, self.lyric_scroll.target),
                );
            }
        }
        let idle_ms = unsafe {
            let mut lii = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            let _ = GetLastInputInfo(&mut lii);
            GetTickCount().wrapping_sub(lii.dwTime)
        };
        let mut next = IDLE_CHECK_AWAKE_MS;
        match self.mascot.mood {
            Mood::Awake if idle_ms as u64 >= mascot::SLEEP_AFTER.as_millis() as u64 => {
                self.mascot.sleep();
                next = IDLE_CHECK_ASLEEP_MS;
                self.update_timers();
                self.refresh();
            }
            Mood::Asleep if idle_ms < 1500 => self.wake(),
            Mood::Asleep => next = IDLE_CHECK_ASLEEP_MS,
            _ => {}
        }
        unsafe {
            SetTimer(Some(self.hwnd), T_IDLE, next, None);
        }
    }

    /// Los ojitos miran sutilmente hacia el cursor cuando está cerca (sin polling: se
    /// actualiza en cada pestañeo, al despertar y al pasar el mouse por el notch).
    fn update_gaze(&mut self) {
        let g = unsafe {
            let mut p = POINT::default();
            let mut wr = RECT::default();
            if GetCursorPos(&mut p).is_err() || GetWindowRect(self.hwnd, &mut wr).is_err() {
                return;
            }
            let ex = wr.left as f32 + (CANVAS_W / 2.0 + self.sw.value / 2.0 - 40.0) * self.scale;
            let ey = wr.top as f32 + 16.0 * self.scale;
            let (dx, dy) = (p.x as f32 - ex, p.y as f32 - ey);
            let dist = (dx * dx + dy * dy).sqrt().max(1.0);
            let reach = 900.0 * self.scale;
            let mag = 1.8 * clamp01(1.0 - dist / reach).sqrt();
            (dx / dist * mag, (dy / dist * mag).min(1.2))
        };
        if self.mascot.look(g) {
            self.refresh();
        }
    }

    fn wake(&mut self) {
        if self.mascot.mood == Mood::Asleep {
            self.mascot.greet();
            unsafe {
                SetTimer(Some(self.hwnd), T_GREET, mascot::GREET_LEN.as_millis() as u32, None);
                SetTimer(Some(self.hwnd), T_IDLE, IDLE_CHECK_AWAKE_MS, None);
            }
            self.update_timers();
            self.refresh();
        }
    }

    // ---------- Multimedia y letras ----------

    fn on_media(&mut self) {
        DBG_MEDIA.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let info = self.media_shared.lock().unwrap().clone();
        // Si solo cambió la posición y el notch está contraído, no hay nada que redibujar.
        let looks = |m: &Option<MediaInfo>| {
            m.as_ref().map(|m| (m.title.clone(), m.artist.clone(), m.playing, m.thumb_id, m.can_prev, m.can_next, m.duration > 0.5))
        };
        if looks(&info) == looks(&self.media) && !self.want_expanded() {
            self.media = info;
            if self.lyrics_on {
                let before = self.lyric_current;
                self.schedule_lyric();
                if before != self.lyric_current {
                    self.refresh();
                }
            }
            return;
        }
        let key = info.as_ref().map(|m| format!("{}\u{1}{}", m.title, m.artist)).unwrap_or_default();
        let track_changed = key != self.lyrics_key;
        if let Some(r) = self.renderer.as_mut() {
            r.set_art(info.as_ref().and_then(|m| m.thumb.clone()));
        }
        self.media = info;
        if track_changed {
            self.lyrics_key = key;
            self.lyrics = None;
            self.lyric_current = None;
            self.lyric_scroll = Spring::new(0.0, 220.0, 0.0).with_eps(0.002);
            if self.lyrics_on && self.has_media() {
                self.fetch_lyrics();
            }
        }
        if !self.has_media() {
            self.lyrics_on = false;
        }
        self.retarget();
        self.refresh();
    }

    fn fetch_lyrics(&mut self) {
        let Some(m) = self.media.clone() else { return };
        self.lyrics_req += 1;
        let id = self.lyrics_req;
        self.lyrics = Some(Lyrics::Loading);
        let shared = self.lyrics_shared.clone();
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let result = lyrics::fetch(&m.title, &m.artist, &m.album, m.duration);
            *shared.lock().unwrap() = Some((id, result));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_LYRICS, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn on_lyrics(&mut self) {
        let got = self.lyrics_shared.lock().unwrap().take();
        if let Some((id, l)) = got {
            if id == self.lyrics_req {
                self.lyrics = Some(l);
                self.lyric_current = None;
                self.retarget();
                self.refresh();
            }
        }
    }

    // ---------- Entrada del mouse ----------

    fn to_dips(&self, lp: LPARAM) -> (f32, f32) {
        let x = (lp.0 & 0xFFFF) as i16 as f32;
        let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32;
        (x / self.scale, y / self.scale)
    }

    fn on_mouse_move(&mut self, lp: LPARAM) {
        unsafe {
            // Re-armamos el seguimiento en cada movimiento: la región cambia durante la animación.
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut tme);
        }
        if !self.hover {
            self.hover = true;
            self.update_gaze();
            if self.mascot.mood == Mood::Asleep {
                self.wake();
            }
            unsafe {
                SetTimer(Some(self.hwnd), T_HOVERCHECK, 200, None);
                let _ = KillTimer(Some(self.hwnd), T_COLLAPSE);
                if self.expand.target < 1.0 {
                    SetTimer(Some(self.hwnd), T_EXPAND, EXPAND_DELAY_MS, None);
                }
            }
        }
        let (x, y) = self.to_dips(lp);
        let btn = if self.expand.target > 0.5 && self.expand.value > 0.6 {
            let n = match &self.lyrics {
                Some(Lyrics::Synced(v)) => v.len(),
                _ => 0,
            };
            self.layout_now().hit(x, y, self.lyric_scroll.value, n, self.clip_on_count())
        } else {
            None
        };
        if btn != self.hover_btn {
            self.hover_btn = btn;
            self.refresh();
        }
    }

    fn on_mouse_leave(&mut self) {
        if !self.hover {
            return;
        }
        self.hover = false;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), T_HOVERCHECK);
            let _ = KillTimer(Some(self.hwnd), T_EXPAND);
            SetTimer(Some(self.hwnd), T_COLLAPSE, COLLAPSE_DELAY_MS, None);
        }
    }

    fn clip_on_count(&self) -> usize {
        if self.clip_on { self.clip.items.len() } else { 0 }
    }

    /// Windows avisa una copia nueva: la sumamos al historial.
    fn on_clipboard(&mut self) {
        if let Some(text) = clipboard::read_text(self.hwnd) {
            if self.clip.push(text) && self.clip_on {
                self.retarget();
                self.refresh();
            }
        }
    }

    /// ¿El cursor está dentro de la región visible de la ventana?
    fn cursor_inside(&self) -> bool {
        unsafe {
            let mut p = POINT::default();
            let mut wr = RECT::default();
            if GetCursorPos(&mut p).is_err() || GetWindowRect(self.hwnd, &mut wr).is_err() {
                return true;
            }
            let (l, r, b) = self.last_region;
            let x = p.x - wr.left;
            let y = p.y - wr.top;
            x >= l && x < r && y >= 0 && y < b
        }
    }

    fn on_click(&mut self, lp: LPARAM) {
        if self.expand.value < 0.6 {
            return;
        }
        let (x, y) = self.to_dips(lp);
        let n = match &self.lyrics {
            Some(Lyrics::Synced(v)) => v.len(),
            _ => 0,
        };
        let layout = self.layout_now();
        let Some(btn) = layout.hit(x, y, self.lyric_scroll.value, n, self.clip_on_count()) else { return };
        match btn {
            Btn::Play => {
                let _ = self.media_tx.send(Cmd::PlayPause);
            }
            Btn::Prev => {
                let _ = self.media_tx.send(Cmd::Prev);
            }
            Btn::Next => {
                let _ = self.media_tx.send(Cmd::Next);
            }
            Btn::Clip => {
                self.clip_on = !self.clip_on;
                if self.clip_on {
                    self.lyrics_on = false;
                }
                self.retarget();
            }
            Btn::ClipItem(i) => {
                if let Some(text) = self.clip.items.get(i).cloned() {
                    if clipboard::set_text(self.hwnd, &text) {
                        self.clip_copied = Some(0);
                        unsafe { SetTimer(Some(self.hwnd), T_COPIED, 900, None) };
                        self.refresh();
                    }
                }
            }
            Btn::Lyrics => {
                self.lyrics_on = !self.lyrics_on;
                if self.lyrics_on {
                    self.clip_on = false;
                }
                if self.lyrics_on && self.lyrics.is_none() {
                    self.fetch_lyrics();
                }
                self.retarget();
            }
            Btn::Progress => {
                if let Some(m) = &self.media {
                    if m.duration > 0.0 && m.can_seek {
                        let t = layout.track;
                        let frac = clamp01((x - t.l) / (t.r - t.l)) as f64;
                        let _ = self.media_tx.send(Cmd::Seek(frac * m.duration));
                    }
                }
            }
            Btn::LyricLine(i) => {
                if let Some(t) = self.synced_lines().and_then(|l| l.get(i)).map(|l| l.0) {
                    if self.media.as_ref().map(|m| m.can_seek).unwrap_or(false) {
                        let _ = self.media_tx.send(Cmd::Seek(t));
                    }
                }
            }
        }
    }

    /// Rueda del mouse: desplaza la letra sin sincronizar.
    fn on_wheel(&mut self, wp: WPARAM) {
        if let Some(Lyrics::Plain(lines)) = &self.lyrics {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            let max = lines.len().saturating_sub(1) as f32;
            let t = (self.lyric_scroll.target - delta * 2.0).clamp(0.0, max);
            let was = self.is_animating();
            self.lyric_scroll.set_target(t);
            if !was {
                self.last_tick = self.now();
            }
        }
    }

    fn on_command(&mut self, cmd: u32) {
        match cmd {
            CMD_AUTOSTART => config::set_autostart(!config::autostart_enabled()),
            CMD_PET_ESME | CMD_PET_GATO | CMD_PET_OFF => {
                let pet = match cmd {
                    CMD_PET_ESME => Some(config::Pet::Esme),
                    CMD_PET_GATO => Some(config::Pet::Gato),
                    _ => None,
                };
                if let Some(p) = pet {
                    self.last_pet = p;
                }
                self.cfg.set_pet(pet);
                let _ = self.cfg.save();
                self.retarget();
                self.refresh();
            }
            CMD_MINIMO => {
                self.cfg.notch_minimo = !self.cfg.notch_minimo;
                let _ = self.cfg.save();
                self.retarget();
            }
            CMD_TRANSPARENTE => {
                self.cfg.transparente = !self.cfg.transparente;
                let _ = self.cfg.save();
                self.last_region = (-1, -1, -1);
                self.retarget();
                self.refresh();
            }
            CMD_MOVER => {
                self.cfg.mover_monitor = !self.cfg.mover_monitor;
                let _ = self.cfg.save();
                self.check_fullscreen();
            }
            CMD_AJUSTES => unsafe {
                let _ = self.cfg.save();
                let path = HSTRING::from(config::config_path().as_os_str());
                ShellExecuteW(None, w!("open"), w!("notepad.exe"), &path, PCWSTR::null(), SW_SHOWNORMAL);
            },
            CMD_SALIR => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            _ => {}
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE => {
                with_app(|a| a.on_mouse_move(lp));
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|a| a.on_mouse_leave());
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                with_app(|a| a.on_click(lp));
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                with_app(|a| a.on_wheel(wp));
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // Los mismos comandos del menú, también por mensaje (útil para automatizar).
            WM_COMMAND => {
                let cmd = (wp.0 & 0xFFFF) as u32;
                with_app(|a| a.on_command(cmd));
                LRESULT(0)
            }
            WM_TIMER => {
                with_app(|a| a.on_timer(wp.0));
                LRESULT(0)
            }
            media::WM_MEDIA => {
                with_app(|a| a.on_media());
                LRESULT(0)
            }
            WM_LYRICS => {
                with_app(|a| a.on_lyrics());
                LRESULT(0)
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                with_app(|a| {
                    // Los monitores pueden haber cambiado: volvemos al principal y re-evaluamos.
                    a.monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
                    a.layout();
                    a.check_fullscreen();
                    a.redraw();
                });
                LRESULT(0)
            }
            WM_CLIPBOARDUPDATE => {
                with_app(|a| a.on_clipboard());
                LRESULT(0)
            }
            WM_APPBAR => {
                if wp.0 == ABN_FULLSCREENAPP {
                    SetTimer(Some(hwnd), T_FS, 80, None);
                }
                LRESULT(0)
            }
            WM_TRAY => {
                let event = (lp.0 & 0xFFFF) as u32;
                match event {
                    WM_CONTEXTMENU | WM_RBUTTONUP => {
                        // El menú es modal (despacha mensajes): no retener el estado mientras está abierto.
                        let info = with_app(|a| {
                            (
                                a.tray.clone(),
                                MenuState {
                                    autostart: config::autostart_enabled(),
                                    pet: match a.cfg.pet() {
                                        Some(config::Pet::Esme) => 0,
                                        Some(config::Pet::Gato) => 1,
                                        None => 2,
                                    },
                                    minimo: a.cfg.notch_minimo,
                                    transparente: a.cfg.transparente,
                                    mover: a.cfg.mover_monitor,
                                },
                            )
                        });
                        if let Some((tray, state)) = info {
                            let cmd = tray.show_menu(&state);
                            if cmd != 0 {
                                with_app(|a| a.on_command(cmd));
                            }
                        }
                    }
                    NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP => {
                        with_app(|a| {
                            a.pinned = !a.pinned;
                            a.retarget();
                        });
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                let mut abd = windows::Win32::UI::Shell::APPBARDATA {
                    cbSize: std::mem::size_of::<windows::Win32::UI::Shell::APPBARDATA>() as u32,
                    hWnd: hwnd,
                    ..Default::default()
                };
                windows::Win32::UI::Shell::SHAppBarMessage(windows::Win32::UI::Shell::ABM_REMOVE, &mut abd);
                with_app(|a| a.tray.remove());
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => {
                let tc = with_app(|a| a.taskbar_created).unwrap_or(0);
                if tc != 0 && msg == tc {
                    with_app(|a| a.tray.add());
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
    }
}
