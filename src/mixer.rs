//! Mezclador por app (Fase 9): volumen del sistema + una barrita por cada app con sonido.
//! Se lee solo al abrir el panel (Core Audio: IAudioSessionManager2 / ISimpleAudioVolume):
//! sin callbacks, sin hilos y sin timers, así no cuesta nada mientras está cerrado.

use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::{CloseHandle, S_OK};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

/// Máximo de filas visibles (Sistema + apps).
pub const MAX_ROWS: usize = 6;

pub struct Channel {
    pub name: String,
    pub level: f32,
    pub muted: bool,
    /// Una app puede tener varias sesiones (Chrome, Discord): las movemos juntas.
    sessions: Vec<ISimpleAudioVolume>,
    active: bool,
}

#[derive(Default)]
pub struct Mixer {
    /// La fila 0 es "Sistema" cuando hay un dispositivo de salida.
    pub channels: Vec<Channel>,
    endpoint: Option<IAudioEndpointVolume>,
}

impl Mixer {
    /// Lee el estado actual del dispositivo de salida por defecto y de sus sesiones.
    pub fn read() -> Self {
        let mut m = Mixer::default();
        unsafe {
            let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
                return m;
            };
            let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eConsole) else {
                return m;
            };
            if let Ok(ep) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                let level = ep.GetMasterVolumeLevelScalar().unwrap_or(0.0);
                let muted = ep.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                m.channels.push(Channel { name: "Sistema".into(), level, muted, sessions: Vec::new(), active: true });
                m.endpoint = Some(ep);
            }
            let mut apps: Vec<Channel> = Vec::new();
            if let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
                if let Ok(list) = mgr.GetSessionEnumerator() {
                    let n = list.GetCount().unwrap_or(0);
                    for i in 0..n {
                        let Ok(ctl) = list.GetSession(i) else { continue };
                        let state = ctl.GetState().unwrap_or(AudioSessionStateExpired);
                        if state == AudioSessionStateExpired {
                            continue;
                        }
                        let Ok(c2) = ctl.cast::<IAudioSessionControl2>() else { continue };
                        if c2.IsSystemSoundsSession() == S_OK {
                            continue;
                        }
                        let pid = c2.GetProcessId().unwrap_or(0);
                        if pid == 0 || pid == std::process::id() {
                            continue;
                        }
                        let Ok(vol) = ctl.cast::<ISimpleAudioVolume>() else { continue };
                        let name = process_name(pid).unwrap_or_else(|| format!("App {pid}"));
                        let level = vol.GetMasterVolume().unwrap_or(1.0);
                        let muted = vol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                        let active = state == AudioSessionStateActive;
                        match apps.iter_mut().find(|c| c.name == name) {
                            Some(c) => {
                                c.sessions.push(vol);
                                c.active |= active;
                            }
                            None => apps.push(Channel { name, level, muted, sessions: vec![vol], active }),
                        }
                    }
                }
            }
            // Primero las que están sonando ahora.
            apps.sort_by_key(|c| !c.active);
            let room = MAX_ROWS.saturating_sub(m.channels.len());
            m.channels.extend(apps.into_iter().take(room));
        }
        m
    }

    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// Cambia el volumen de la fila `i` (0..1). Si estaba en silencio, lo quita.
    pub fn set_level(&mut self, i: usize, level: f32) {
        let level = level.clamp(0.0, 1.0);
        let is_master = i == 0 && self.endpoint.is_some();
        let Some(ch) = self.channels.get_mut(i) else { return };
        unsafe {
            if is_master {
                if let Some(ep) = &self.endpoint {
                    let _ = ep.SetMasterVolumeLevelScalar(level, std::ptr::null());
                    if ch.muted && level > 0.0 {
                        let _ = ep.SetMute(false, std::ptr::null());
                    }
                }
            } else {
                for s in &ch.sessions {
                    let _ = s.SetMasterVolume(level, std::ptr::null());
                    if ch.muted && level > 0.0 {
                        let _ = s.SetMute(false, std::ptr::null());
                    }
                }
            }
        }
        ch.level = level;
        if level > 0.0 {
            ch.muted = false;
        }
    }

    /// Silencia o vuelve a activar la fila `i`.
    pub fn toggle_mute(&mut self, i: usize) {
        let is_master = i == 0 && self.endpoint.is_some();
        let Some(ch) = self.channels.get_mut(i) else { return };
        let m = !ch.muted;
        unsafe {
            if is_master {
                if let Some(ep) = &self.endpoint {
                    let _ = ep.SetMute(m, std::ptr::null());
                }
            } else {
                for s in &ch.sessions {
                    let _ = s.SetMute(m, std::ptr::null());
                }
            }
        }
        ch.muted = m;
    }

    /// Suelta las interfaces COM al cerrar el panel (cero memoria retenida).
    pub fn clear(&mut self) {
        self.channels.clear();
        self.endpoint = None;
    }
}

/// Nombre lindo del proceso: "msedge.exe" → "Edge", "spotify.exe" → "Spotify".
fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let stem = std::path::Path::new(&path).file_stem()?.to_string_lossy().to_string();
        Some(pretty(&stem))
    }
}

fn pretty(stem: &str) -> String {
    let known = [
        ("msedge", "Edge"),
        ("msedgewebview2", "Edge WebView"),
        ("chrome", "Chrome"),
        ("firefox", "Firefox"),
        ("opera", "Opera"),
        ("brave", "Brave"),
        ("spotify", "Spotify"),
        ("discord", "Discord"),
        ("vlc", "VLC"),
        ("robloxplayerbeta", "Roblox"),
        ("steam", "Steam"),
        ("steamwebhelper", "Steam"),
        ("whatsapp", "WhatsApp"),
        ("teams", "Teams"),
        ("ms-teams", "Teams"),
        ("zoom", "Zoom"),
        ("obs64", "OBS"),
        ("music.ui", "Media Player"),
        ("microsoft.media.player", "Media Player"),
    ];
    let low = stem.to_lowercase();
    if let Some((_, n)) = known.iter().find(|(k, _)| *k == low) {
        return (*n).to_string();
    }
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::pretty;

    #[test]
    fn nombres_lindos() {
        assert_eq!(pretty("msedge"), "Edge");
        assert_eq!(pretty("Spotify"), "Spotify");
        assert_eq!(pretty("obs64"), "OBS");
        assert_eq!(pretty("miapp"), "Miapp");
    }
}
