//! Aviso de Wi-Fi por eventos, sin polling y sin tocar la UI desde el callback.
//! Usa la API moderna de Windows (`Windows.Networking.Connectivity.NetworkInformation`): Windows
//! avisa cuando cambia la red y Esme solo pregunta si la conexión a internet actual es Wi-Fi y
//! cómo se llama. No lee perfiles ni contraseñas de redes.

use windows::Networking::Connectivity::{NetworkInformation, NetworkStatusChangedEventHandler};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Mensaje enviado desde el aviso de Windows hacia el hilo principal de la UI.
pub const WM_WIFI: u32 = WM_APP + 11;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WifiEvent {
    Connected(String),
    Disconnected(String),
}

/// Red Wi-Fi actual (Some(nombre)) o None si internet no va por Wi-Fi.
fn current_wifi() -> Option<String> {
    let profile = NetworkInformation::GetInternetConnectionProfile().ok()?;
    if !profile.IsWlanConnectionProfile().unwrap_or(false) {
        return None;
    }
    Some(profile.ProfileName().map(|n| n.to_string_lossy()).unwrap_or_default())
}

/// Suscripción a cambios de red. Se libera automáticamente al cerrar Esme.
pub struct Wifi {
    token: i64,
    last: Option<String>,
}

impl Wifi {
    pub fn new(hwnd: HWND) -> Option<Self> {
        let h = hwnd.0 as isize;
        let handler = NetworkStatusChangedEventHandler::new(move |_| {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_WIFI, WPARAM(0), LPARAM(0));
            }
            Ok(())
        });
        let token = NetworkInformation::NetworkStatusChanged(&handler).ok()?;
        Some(Self { token, last: current_wifi() })
    }

    /// Llamar desde el hilo de la UI al recibir `WM_WIFI`: devuelve qué cambió, si algo cambió.
    pub fn take_event(&mut self) -> Option<WifiEvent> {
        let now = current_wifi();
        if now == self.last {
            return None;
        }
        let prev = std::mem::replace(&mut self.last, now.clone());
        match (prev, now) {
            (_, Some(name)) => Some(WifiEvent::Connected(name)),
            (Some(old), None) => Some(WifiEvent::Disconnected(old)),
            (None, None) => None,
        }
    }
}

impl Drop for Wifi {
    fn drop(&mut self) {
        let _ = NetworkInformation::RemoveNetworkStatusChanged(self.token);
    }
}
