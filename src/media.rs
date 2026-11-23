//! Integración con los controles multimedia de Windows (SMTC): Spotify, navegadores, etc.
//! Corre en su propio hilo (apartamento MTA), 100 % por eventos: duerme en `recv()` hasta que
//! Windows avisa un cambio o la UI pide un comando.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use windows::core::Result;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::*;
use windows::Storage::Streams::DataReader;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_MEDIA: u32 = WM_APP + 2;

#[derive(Clone, Default, Debug)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Posición (s) medida en `stamp` (FILETIME en ticks de 100 ns).
    pub position: f64,
    pub stamp: i64,
    pub duration: f64,
    pub thumb: Option<Arc<Vec<u8>>>,
    /// Cambia cada vez que cambia la carátula.
    pub thumb_id: u64,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_seek: bool,
}

impl MediaInfo {
    pub fn position_now(&self) -> f64 {
        let mut p = self.position;
        if self.playing {
            p += (now_filetime() - self.stamp) as f64 / 1e7;
        }
        if self.duration > 0.0 {
            p = p.clamp(0.0, self.duration);
        }
        p.max(0.0)
    }
}

pub enum Cmd {
    Refresh { thumb: bool },
    PlayPause,
    Next,
    Prev,
    Seek(f64),
}

pub type Shared = Arc<Mutex<Option<MediaInfo>>>;

pub fn now_filetime() -> i64 {
    unsafe {
        let ft = windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime();
        ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
    }
}

/// Arranca el hilo multimedia. Devuelve el canal de comandos y el estado compartido.
pub fn start(hwnd: HWND) -> (Sender<Cmd>, Shared) {
    let shared: Shared = Arc::new(Mutex::new(None));
    let (tx, rx) = channel::<Cmd>();
    let hwnd_raw = hwnd.0 as isize;
    let sh = shared.clone();
    let tx2 = tx.clone();
    std::thread::Builder::new()
        .name("esme-media".into())
        .spawn(move || {
            unsafe {
                let _ = RoInitialize(RO_INIT_MULTITHREADED);
            }
            let notify = move || unsafe {
                let _ = PostMessageW(Some(HWND(hwnd_raw as *mut _)), WM_MEDIA, WPARAM(0), LPARAM(0));
            };
            let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get())
            else {
                return;
            };
            let t = tx2.clone();
            let _ = manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                let _ = t.send(Cmd::Refresh { thumb: true });
                Ok(())
            }));

            let mut session: Option<GlobalSystemMediaTransportControlsSession> = None;
            let mut tokens: Vec<i64> = Vec::new();
            let mut last_key = String::new();
            let mut thumb_id = 0u64;
            let _ = tx2.send(Cmd::Refresh { thumb: true });

            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::Refresh { thumb } => {
                        // ¿Cambió la sesión actual? Re-suscribimos sus eventos.
                        let current = manager.GetCurrentSession().ok();
                        let changed = match (&session, &current) {
                            (Some(a), Some(b)) => a.SourceAppUserModelId().ok() != b.SourceAppUserModelId().ok(),
                            (None, None) => false,
                            _ => true,
                        };
                        if changed {
                            if let Some(s) = &session {
                                unsubscribe(s, &tokens);
                            }
                            tokens.clear();
                            if let Some(s) = &current {
                                tokens = subscribe(s, &tx2);
                            }
                            session = current;
                        }
                        let got = session.as_ref().and_then(|s| {
                            read_info(s, thumb || changed, &mut last_key, &mut thumb_id, &sh).ok()
                        });
                        let new_track = got.as_ref().map(|g| g.1).unwrap_or(false);
                        *sh.lock().unwrap() = got.map(|g| g.0);
                        notify();
                        if new_track {
                            // La miniatura del video nuevo a veces llega tarde: revisamos de nuevo.
                            let t = tx2.clone();
                            std::thread::spawn(move || {
                                for ms in [800u64, 1700] {
                                    std::thread::sleep(std::time::Duration::from_millis(ms));
                                    let _ = t.send(Cmd::Refresh { thumb: true });
                                }
                            });
                        }
                    }
                    other => {
                        if let Some(s) = &session {
                            let _ = run_cmd(s, other);
                        }
                    }
                }
            }
        })
        .expect("no se pudo crear el hilo multimedia");
    (tx, shared)
}

fn subscribe(s: &GlobalSystemMediaTransportControlsSession, tx: &Sender<Cmd>) -> Vec<i64> {
    let mut v = Vec::new();
    let t = tx.clone();
    if let Ok(k) = s.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Cmd::Refresh { thumb: true });
        Ok(())
    })) {
        v.push(k);
    }
    let t = tx.clone();
    if let Ok(k) = s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Cmd::Refresh { thumb: false });
        Ok(())
    })) {
        v.push(k);
    }
    let t = tx.clone();
    if let Ok(k) = s.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t.send(Cmd::Refresh { thumb: false });
        Ok(())
    })) {
        v.push(k);
    }
    v
}

fn unsubscribe(s: &GlobalSystemMediaTransportControlsSession, tokens: &[i64]) {
    if let [a, b, c] = tokens {
        let _ = s.RemoveMediaPropertiesChanged(*a);
        let _ = s.RemovePlaybackInfoChanged(*b);
        let _ = s.RemoveTimelinePropertiesChanged(*c);
    }
}

fn read_info(
    s: &GlobalSystemMediaTransportControlsSession,
    want_thumb: bool,
    last_key: &mut String,
    thumb_id: &mut u64,
    shared: &Shared,
) -> Result<(MediaInfo, bool)> {
    let prev = shared.lock().unwrap().clone();
    // Avisos de solo posición/estado: reusamos título y carátula (pedirlos es lo caro).
    let props = match (&prev, want_thumb) {
        (Some(_), false) => None,
        _ => Some(s.TryGetMediaPropertiesAsync()?.get()?),
    };
    let mut info = match &props {
        Some(props) => MediaInfo {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            album: props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default(),
            ..Default::default()
        },
        None => {
            let p = prev.clone().unwrap();
            MediaInfo { title: p.title, artist: p.artist, album: p.album, thumb: p.thumb, thumb_id: p.thumb_id, ..Default::default() }
        }
    };

    if let Ok(pb) = s.GetPlaybackInfo() {
        info.playing = pb.PlaybackStatus()? == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
        if let Ok(c) = pb.Controls() {
            info.can_prev = c.IsPreviousEnabled().unwrap_or(false);
            info.can_next = c.IsNextEnabled().unwrap_or(false);
            info.can_seek = c.IsPlaybackPositionEnabled().unwrap_or(false);
        }
    }

    if let Ok(tl) = s.GetTimelineProperties() {
        let start = tl.StartTime().map(|t| t.Duration).unwrap_or(0);
        let end = tl.EndTime().map(|t| t.Duration).unwrap_or(0);
        let pos = tl.Position().map(|t| t.Duration).unwrap_or(0);
        info.duration = ((end - start).max(0)) as f64 / 1e7;
        info.position = ((pos - start).max(0)) as f64 / 1e7;
        let upd = tl.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0);
        let now = now_filetime();
        info.stamp = if upd > 0 && upd <= now { upd } else { now };
    } else {
        info.stamp = now_filetime();
    }

    // Carátula: se relee en cada aviso de "cambió el contenido" (Brave/YouTube a veces cambian la
    // miniatura sin cambiar el título, o la mandan tarde). Solo se re-decodifica si los bytes cambian.
    let key = format!("{}\u{1}{}\u{1}{}", info.title, info.artist, info.album);
    let Some(props) = props else { return Ok((info, false)) };
    let changed_track = key != *last_key;
    *last_key = key;
    let fresh = read_thumb(&props).ok().flatten();
    let same_bytes = match (&fresh, prev.as_ref().and_then(|p| p.thumb.as_ref())) {
        (Some(a), Some(b)) => a.as_slice() == b.as_slice(),
        _ => false,
    };
    if !changed_track && (same_bytes || fresh.is_none()) {
        // Misma canción y misma miniatura (o todavía no llegó la nueva): mantenemos la actual.
        if let Some(p) = prev {
            info.thumb = p.thumb;
            info.thumb_id = p.thumb_id;
        }
    } else {
        // Canción nueva o miniatura nueva: nunca mostramos la anterior (sin imagen, va el ícono).
        info.thumb = fresh.map(Arc::new);
        *thumb_id += 1;
        info.thumb_id = *thumb_id;
    }
    Ok((info, changed_track))
}

fn read_thumb(props: &GlobalSystemMediaTransportControlsSessionMediaProperties) -> Result<Option<Vec<u8>>> {
    let Ok(r) = props.Thumbnail() else { return Ok(None) };
    let stream = r.OpenReadAsync()?.get()?;
    let size = stream.Size()? as u32;
    if size == 0 || size > 8 * 1024 * 1024 {
        return Ok(None);
    }
    let reader = DataReader::CreateDataReader(&stream)?;
    reader.LoadAsync(size)?.get()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf)?;
    Ok(Some(buf))
}

fn run_cmd(s: &GlobalSystemMediaTransportControlsSession, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::PlayPause => {
            s.TryTogglePlayPauseAsync()?.get()?;
        }
        Cmd::Next => {
            s.TrySkipNextAsync()?.get()?;
        }
        Cmd::Prev => {
            s.TrySkipPreviousAsync()?.get()?;
        }
        Cmd::Seek(secs) => {
            let start = s.GetTimelineProperties().and_then(|t| t.StartTime()).map(|t| t.Duration).unwrap_or(0);
            s.TryChangePlaybackPositionAsync(start + (secs * 1e7) as i64)?.get()?;
        }
        Cmd::Refresh { .. } => {}
    }
    Ok(())
}
