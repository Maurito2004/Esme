//! Aviso de Wi-Fi por eventos (v1.0 — Bloque 1), sin polling y sin tocar la UI desde el callback.
//! Usa la API nativa de Windows (`wlanapi.dll`: `WlanOpenHandle` + `WlanRegisterNotification`).
//! En PCs de escritorio sin placa Wi-Fi o con el servicio WLAN detenido, `Wifi::new` devuelve `None`
//! limpiamente para que Esme ignore el módulo y siga funcionando sin errores.

use std::ffi::c_void;
use std::sync::Mutex;
use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::NetworkManagement::WiFi::{
    wlan_notification_acm_connection_complete, wlan_notification_acm_disconnected,
    WlanCloseHandle, WlanOpenHandle, WlanRegisterNotification, L2_NOTIFICATION_DATA,
    WLAN_CONNECTION_NOTIFICATION_DATA, WLAN_NOTIFICATION_SOURCE_ACM,
    WLAN_NOTIFICATION_SOURCE_NONE,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Mensaje enviado desde el callback de Windows hacia el hilo principal de la UI.
pub const WM_WIFI: u32 = WM_APP + 11;

const WLAN_REASON_SUCCESS: u32 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WifiEvent {
    Connected(String),
    Disconnected(String),
}

/// Último evento pendiente de leer en el hilo de la UI.
static LAST_EVENT: Mutex<Option<WifiEvent>> = Mutex::new(None);

/// Toma el último evento de Wi-Fi enviado por el callback (solo llamar desde el hilo de la UI).
pub fn take_event() -> Option<WifiEvent> {
    LAST_EVENT.lock().ok().and_then(|mut slot| slot.take())
}

/// Extrae el SSID (o nombre de perfil como respaldo) desde `WLAN_CONNECTION_NOTIFICATION_DATA`.
fn extract_ssid(conn: &WLAN_CONNECTION_NOTIFICATION_DATA) -> String {
    let len = (conn.dot11Ssid.uSSIDLength as usize).min(conn.dot11Ssid.ucSSID.len());
    if len > 0 {
        let s = String::from_utf8_lossy(&conn.dot11Ssid.ucSSID[..len]).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    let prof_len = conn.strProfileName.iter().position(|&c| c == 0).unwrap_or(conn.strProfileName.len());
    String::from_utf16_lossy(&conn.strProfileName[..prof_len]).trim().to_string()
}

/// Callback ejecutado por Windows desde un hilo secundario de WLAN AutoConfig.
/// Nunca toca la UI directamente: solo guarda el evento y notifica vía `PostMessageW`.
unsafe extern "system" fn wlan_callback(data: *mut L2_NOTIFICATION_DATA, context: *mut c_void) {
    if data.is_null() || context.is_null() {
        return;
    }
    let d = &*data;
    if d.NotificationSource != WLAN_NOTIFICATION_SOURCE_ACM {
        return;
    }
    let min_size = std::mem::size_of::<WLAN_CONNECTION_NOTIFICATION_DATA>();
    let conn = if !d.pData.is_null() && (d.dwDataSize as usize) >= min_size {
        Some(&*(d.pData as *const WLAN_CONNECTION_NOTIFICATION_DATA))
    } else {
        None
    };

    let ev = if d.NotificationCode == wlan_notification_acm_connection_complete.0 as u32 {
        match conn {
            // Solo avisamos conexiones exitosas (evita ruido por intentos fallidos o roaming interno).
            Some(c) if c.wlanReasonCode == WLAN_REASON_SUCCESS => Some(WifiEvent::Connected(extract_ssid(c))),
            _ => None,
        }
    } else if d.NotificationCode == wlan_notification_acm_disconnected.0 as u32 {
        let ssid = conn.map(extract_ssid).unwrap_or_default();
        Some(WifiEvent::Disconnected(ssid))
    } else {
        None
    };

    if let Some(event) = ev {
        if let Ok(mut slot) = LAST_EVENT.lock() {
            *slot = Some(event);
        }
        let _ = PostMessageW(Some(HWND(context)), WM_WIFI, WPARAM(0), LPARAM(0));
    }
}

/// Suscripción a eventos de Wi-Fi. Se libera automáticamente al cerrar Esme.
pub struct Wifi {
    handle: HANDLE,
}

impl Wifi {
    /// Abre `WlanOpenHandle` y registra notificaciones ACM hacia `hwnd`.
    /// Devuelve `None` limpiamente si no hay adaptador Wi-Fi o el servicio WLAN no está activo.
    pub fn new(hwnd: HWND) -> Option<Self> {
        unsafe {
            let mut negotiated: u32 = 0;
            let mut handle = HANDLE::default();
            let rc = WlanOpenHandle(2, None, &mut negotiated, &mut handle);
            if rc != ERROR_SUCCESS.0 || handle.is_invalid() {
                return None;
            }
            let reg = WlanRegisterNotification(
                handle,
                WLAN_NOTIFICATION_SOURCE_ACM,
                true,
                Some(wlan_callback),
                Some(hwnd.0 as *const c_void),
                None,
                None,
            );
            if reg != ERROR_SUCCESS.0 {
                let _ = WlanCloseHandle(handle, None);
                return None;
            }
            Some(Self { handle })
        }
    }
}

impl Drop for Wifi {
    fn drop(&mut self) {
        unsafe {
            let _ = WlanRegisterNotification(
                self.handle,
                WLAN_NOTIFICATION_SOURCE_NONE,
                false,
                None,
                None,
                None,
                None,
            );
            let _ = WlanCloseHandle(self.handle, None);
        }
    }
}
