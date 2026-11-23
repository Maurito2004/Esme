//! Esme — barra dinámica para Windows 10/11.
//! Fase 1: ventana transparente, render GPU (Direct2D + DirectComposition), hover y bandeja.
//! Fase 2: multimedia (SMTC), letras sincronizadas (LRCLIB) y mascota animada.

#![windows_subsystem = "windows"]

// Recursos de Windows (ícono, manifiesto, versión) generados por tools/build-res.ps1.
#[link(name = "esme_res", kind = "static", modifiers = "+whole-archive")]
extern "C" {}

mod anim;
mod app;
mod wndproc;
mod clipboard;
mod ical;
mod ocr;
mod productivity;
mod weather;
mod shelf;
mod system;
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
use render::{ClipView, ShelfView, TodayView, Collapsed, Expanded, Frame, LyricsView, MediaView, Renderer};
use std::cell::RefCell;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tray::*;
use ui::{Btn, Dims, Layout, CANVAS_H, CANVAS_W};
use windows::core::{w, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Ole::{OleInitialize, RegisterDragDrop};
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
const T_TOAST: usize = 14;
const T_DROPCLOSE: usize = 15;
const HOTKEY_OCR: i32 = 1;
const T_OVERLAY: usize = 16;
const T_POMO: usize = 17;
const T_DATA: usize = 18;
/// Clima y calendario se actualizan cada una hora.
const DATA_REFRESH_MS: u32 = 60 * 60 * 1000;
const WM_DATA: u32 = WM_APP + 9;

/// Resultados de los hilos de clima y calendario.
#[derive(Default)]
struct DataInbox {
    weather: Option<std::result::Result<weather::Weather, String>>,
    events: Option<std::result::Result<Vec<ical::Event>, String>>,
}
const OVERLAY_MS: u32 = 1600;
const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_POWERSETTINGCHANGE: usize = 0x8013;
const HSHELL_LANGUAGE: usize = 8;

/// Overlay del sistema que se está mostrando en la píldora.
struct SysOverlay {
    glyph: &'static str,
    text: String,
    level: Option<f32>,
}
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
    // Estante de archivos.
    shelf: shelf::Shelf,
    shelf_on: bool,
    /// Hay archivos arrastrándose sobre el notch (o recién soltados).
    drop_open: bool,
    drop_hover: bool,
    /// Ítem del estante presionado: (índice, x, y) para detectar el arrastre hacia afuera.
    press: Option<(usize, f32, f32)>,
    _drop_target: Option<windows::Win32::System::Ole::IDropTarget>,
    // Aviso temporal en la píldora (ej. "Texto copiado").
    toast: Option<String>,
    // Panel "Hoy" (Fase 5).
    today_on: bool,
    pomo: productivity::Pomodoro,
    water: productivity::Water,
    weather: Option<weather::Weather>,
    weather_msg: String,
    events: Vec<ical::Event>,
    cal_msg: String,
    inbox: Arc<Mutex<DataInbox>>,
    // Overlays del sistema (Fase 4).
    overlay: Option<SysOverlay>,
    /// Nivel animado de la barra del overlay (el volumen se desliza al valor nuevo).
    level: Spring,
    /// Al terminar el saludo de bienvenida, la mascota saluda con un saltito.
    greet_after_toast: bool,
    audio: Option<system::Audio>,
    shell_msg: u32,
    last_ac: Option<u32>,
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
        // OLE (STA): drag & drop del estante, WIC para las carátulas.
        let _ = OleInitialize(None);

        let hinst = GetModuleHandleW(None)?;
        let class = w!("EsmeNotch");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc::wndproc),
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
            shelf: shelf::Shelf::load(),
            shelf_on: false,
            drop_open: false,
            drop_hover: false,
            press: None,
            _drop_target: None,
            toast: None,
            today_on: false,
            pomo: productivity::Pomodoro::new(),
            water: productivity::Water::load(),
            weather: None,
            weather_msg: "Buscando el clima…".into(),
            events: Vec::new(),
            cal_msg: String::new(),
            inbox: Arc::new(Mutex::new(DataInbox::default())),
            overlay: None,
            level: Spring::new(0.0, 320.0, 0.05).with_eps(0.002),
            greet_after_toast: false,
            audio: None,
            shell_msg: 0,
            last_ac: None,
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
        with_app(|a| a.say_hello());
        let _ = windows::Win32::System::DataExchange::AddClipboardFormatListener(hwnd);
        // Estante: el notch acepta archivos arrastrados. OCR: atajo global Ctrl+Alt+T.
        let target = shelf::DropTarget::new(hwnd);
        let _ = RegisterDragDrop(hwnd, &target);
        with_app(|a| a._drop_target = Some(target));
        use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_OCR, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'T' as u32);

        // Overlays del sistema: audio, idioma del teclado y cargador (todo por eventos).
        let audio = system::Audio::new(hwnd);
        let _ = RegisterShellHookWindow(hwnd);
        let shell_msg = RegisterWindowMessageW(w!("SHELLHOOK"));
        let _ = windows::Win32::System::Power::RegisterPowerSettingNotification(
            HANDLE(hwnd.0),
            &windows::Win32::System::SystemServices::GUID_ACDC_POWER_SOURCE,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        );
        // Clima y calendario: primera carga ahora y después cada una hora.
        with_app(|a| {
            a.refresh_data();
            SetTimer(Some(a.hwnd), T_DATA, DATA_REFRESH_MS, None);
        });
        with_app(|a| {
            a.audio = audio;
            a.shell_msg = shell_msg;
        });

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
