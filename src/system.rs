//! Overlays del sistema (Fase 4), todo por eventos de Windows, sin polling:
//! - Volumen y silencio: callback de Core Audio (IAudioEndpointVolumeCallback).
//! - Cambio de salida de audio (ej. conectar auriculares Bluetooth): IMMNotificationClient.
//! - Idioma del teclado: shell hook (HSHELL_LANGUAGE).
//! - Cargador conectado / desconectado: notificación de energía (GUID_ACDC_POWER_SOURCE).

use windows::core::{implement, Result, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{HWND, LPARAM, PROPERTYKEY, WPARAM};
use windows::Win32::Media::Audio::Endpoints::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// wParam: 1 = volumen (lParam = porcentaje | silenciado << 8), 2 = cambió la salida de audio.
pub const WM_SYS: u32 = WM_APP + 8;
pub const SYS_VOLUME: usize = 1;
pub const SYS_AUDIO_DEVICE: usize = 2;

fn post(hwnd: isize, kind: usize, value: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_SYS, WPARAM(kind), LPARAM(value));
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCb {
    hwnd: isize,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCb_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if let Some(d) = unsafe { data.as_ref() } {
            let pct = (d.fMasterVolume * 100.0).round().clamp(0.0, 100.0) as isize;
            let muted = d.bMuted.as_bool() as isize;
            post(self.hwnd, SYS_VOLUME, pct | (muted << 8));
        }
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceCb {
    hwnd: isize,
}

impl IMMNotificationClient_Impl for DeviceCb_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> Result<()> {
        if flow == eRender && role == eConsole {
            post(self.hwnd, SYS_AUDIO_DEVICE, 0);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

/// Suscripciones de audio. Se re-crean cuando cambia la salida predeterminada.
pub struct Audio {
    hwnd: isize,
    enumerator: IMMDeviceEnumerator,
    endpoint: Option<IAudioEndpointVolume>,
    volume_cb: IAudioEndpointVolumeCallback,
    device_cb: IMMNotificationClient,
    pub device_name: String,
}

impl Audio {
    pub fn new(hwnd: HWND) -> Option<Self> {
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let h = hwnd.0 as isize;
            let device_cb: IMMNotificationClient = DeviceCb { hwnd: h }.into();
            let _ = enumerator.RegisterEndpointNotificationCallback(&device_cb);
            let mut a = Self {
                hwnd: h,
                enumerator,
                endpoint: None,
                volume_cb: VolumeCb { hwnd: h }.into(),
                device_cb,
                device_name: String::new(),
            };
            a.attach_default();
            Some(a)
        }
    }

    /// Engancha el callback de volumen a la salida predeterminada actual.
    pub fn attach_default(&mut self) {
        unsafe {
            if let Some(ep) = self.endpoint.take() {
                let _ = ep.UnregisterControlChangeNotify(&self.volume_cb);
            }
            let Ok(dev) = self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else { return };
            self.device_name = friendly_name(&dev).unwrap_or_default();
            if let Ok(ep) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                let _ = ep.RegisterControlChangeNotify(&self.volume_cb);
                self.endpoint = Some(ep);
            }
        }
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        unsafe {
            if let Some(ep) = self.endpoint.take() {
                let _ = ep.UnregisterControlChangeNotify(&self.volume_cb);
            }
            let _ = self.enumerator.UnregisterEndpointNotificationCallback(&self.device_cb);
        }
        let _ = self.hwnd;
    }
}

fn friendly_name(dev: &IMMDevice) -> Option<String> {
    unsafe {
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let v = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let p = PropVariantToStringAlloc(&v).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// Nombre del idioma de un HKL (ej. "Español (Argentina)").
pub fn layout_name(hkl: isize) -> String {
    use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME};
    unsafe {
        let lcid = (hkl as usize & 0xFFFF) as u32;
        let mut name = [0u16; 85];
        let n = LCIDToLocaleName(lcid, Some(&mut name), 0);
        if n == 0 {
            return String::new();
        }
        let mut out = [0u16; 128];
        let m = GetLocaleInfoEx(PCWSTR(name.as_ptr()), LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut out));
        String::from_utf16_lossy(&out[..(m.max(1) - 1) as usize])
    }
}
