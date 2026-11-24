//! Configuración persistente en %APPDATA%\Esme\config.json y autoarranque (HKCU\...\Run).

use nanoserde::{DeJson, SerJson};
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Esme");
/// Nombre anterior de la app (para migrar el inicio automático).
const OLD_RUN_VALUE: PCWSTR = w!("Noosa");

#[derive(SerJson, DeJson, Clone, Debug)]
pub struct Config {
    /// Mascota activada (compatibilidad con configs viejas; la elección real es `mascota_tipo`).
    #[nserde(default = "true")]
    pub mascota: bool,
    /// "esme" (perrito, por defecto), "gato" u "off".
    #[nserde(default)]
    pub mascota_tipo: String,
    /// Modo "notch mínimo": una línea fina en el borde superior cuando está contraído.
    #[nserde(default)]
    pub notch_minimo: bool,
    /// Al expandir, solo la píldora de arriba queda negra y el contenido flota transparente.
    #[nserde(default)]
    pub transparente: bool,
    /// En pantalla completa (juego, video): pasar al otro monitor en vez de solo esconderse.
    #[nserde(default = "true")]
    pub mover_monitor: bool,
    /// Ejecutables extra que cuentan como juego (ej. "minecraft.exe"). Roblox ya viene incluido.
    #[nserde(default)]
    pub juegos_extra: Vec<String>,
    /// Tema del menú de la bandeja: "oscuro" (por defecto) o "claro".
    #[nserde(default)]
    pub tema_menu: String,
    /// Meta diaria de vasos de agua (250 ml).
    #[nserde(default = "8")]
    pub agua_meta: u32,
    /// URL privada iCal (ej. Google Calendar → "Dirección secreta en formato iCal").
    #[nserde(default)]
    pub ical_url: String,
    /// Modelo concreto de Gemini que se está usando (vacío = elegir el más nuevo disponible).
    /// Esme lo actualiza sola si Google retira el modelo.
    #[nserde(default)]
    pub gemini_model: String,
    /// Familia elegida en Ajustes: "flash" (por defecto), "flash-lite" o "pro".
    #[nserde(default)]
    pub gemini_familia: String,
    /// API Key de Gemini cifrada con Windows DPAPI (en hexadecimal, atada al usuario actual de Windows).
    #[nserde(default)]
    pub gemini_key_dpapi: String,

    // ---------- Fase 8: Ajustes ----------
    /// Tamaño del notch en % (100, 115 o 130) sobre la escala de Windows.
    #[nserde(default = "100")]
    pub tamano: u32,
    /// Ciudad elegida para el clima (vacía = usar la ubicación de Windows).
    #[nserde(default)]
    pub ciudad: String,
    #[nserde(default)]
    pub ciudad_lat: f64,
    #[nserde(default)]
    pub ciudad_lon: f64,
    /// Qué filas muestra el panel "Hoy".
    #[nserde(default = "true")]
    pub hoy_pomodoro: bool,
    #[nserde(default = "true")]
    pub hoy_agua: bool,
    #[nserde(default = "true")]
    pub hoy_clima: bool,
    #[nserde(default = "true")]
    pub hoy_calendario: bool,
    /// Minutos de foco y de descanso del Pomodoro.
    #[nserde(default = "25")]
    pub pomo_foco: u32,
    #[nserde(default = "5")]
    pub pomo_descanso: u32,
    /// Avisos del sistema en la píldora.
    #[nserde(default = "true")]
    pub aviso_volumen: bool,
    #[nserde(default = "true")]
    pub aviso_audio: bool,
    #[nserde(default = "true")]
    pub aviso_teclado: bool,
    #[nserde(default = "true")]
    pub aviso_energia: bool,
    #[nserde(default = "true")]
    pub aviso_brillo: bool,
    #[nserde(default = "true")]
    pub aviso_bateria: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mascota: true,
            mascota_tipo: "esme".into(),
            notch_minimo: false,
            transparente: false,
            mover_monitor: true,
            juegos_extra: Vec::new(),
            tema_menu: "oscuro".into(),
            agua_meta: 8,
            ical_url: String::new(),
            gemini_model: String::new(),
            gemini_familia: "flash".into(),
            gemini_key_dpapi: String::new(),
            tamano: 100,
            ciudad: String::new(),
            ciudad_lat: 0.0,
            ciudad_lon: 0.0,
            hoy_pomodoro: true,
            hoy_agua: true,
            hoy_clima: true,
            hoy_calendario: true,
            pomo_foco: 25,
            pomo_descanso: 5,
            aviso_volumen: true,
            aviso_audio: true,
            aviso_teclado: true,
            aviso_energia: true,
            aviso_brillo: true,
            aviso_bateria: true,
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Esme")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Al arrancar: traemos los datos de %APPDATA%\Noosa y el inicio automático.
pub fn migrate_from_noosa() {
    let new = data_dir();
    if let Some(parent) = new.parent() {
        let old = parent.join("Noosa");
        if !new.exists() && old.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
    // Versiones anteriores usaban la clave Run del Registro: pasamos a un acceso directo.
    for name in [OLD_RUN_VALUE, RUN_VALUE] {
        if had_run_value(name) {
            unsafe {
                let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, name);
            }
            set_autostart(true);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pet {
    Esme,
    Gato,
}

impl Config {
    /// La mascota elegida, o None si está desactivada.
    pub fn pet(&self) -> Option<Pet> {
        match self.mascota_tipo.as_str() {
            "off" => None,
            "gato" => Some(Pet::Gato),
            _ => Some(Pet::Esme),
        }
    }

    pub fn set_pet(&mut self, p: Option<Pet>) {
        self.mascota_tipo = match p {
            None => "off",
            Some(Pet::Gato) => "gato",
            Some(Pet::Esme) => "esme",
        }
        .into();
        self.mascota = p.is_some();
    }

    pub fn load() -> Self {
        let cfg = std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| Config::deserialize_json(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default();
        let mut cfg: Config = cfg;
        if !cfg.mascota {
            cfg.mascota_tipo = "off".into();
        }
        // El modelo fijo de versiones anteriores ya no está disponible para keys nuevas:
        // lo dejamos vacío y Esme elige el más nuevo que tenga la key.
        if cfg.gemini_model.starts_with("gemini-2.5") {
            if cfg.gemini_familia.is_empty() {
                cfg.gemini_familia = if cfg.gemini_model.contains("lite") {
                    "flash-lite"
                } else if cfg.gemini_model.contains("pro") {
                    "pro"
                } else {
                    "flash"
                }
                .into();
            }
            cfg.gemini_model.clear();
        }
        if !matches!(cfg.gemini_familia.as_str(), "flash" | "flash-lite" | "pro") {
            cfg.gemini_familia = "flash".into();
        }
        if !matches!(cfg.tamano, 100 | 115 | 130) {
            cfg.tamano = 100;
        }
        cfg.pomo_foco = cfg.pomo_foco.clamp(5, 90);
        cfg.pomo_descanso = cfg.pomo_descanso.clamp(1, 30);
        cfg.agua_meta = cfg.agua_meta.clamp(1, 20);
        // Asegura que el archivo exista para que "Ajustes" pueda abrirlo.
        if !config_path().exists() {
            let _ = Self::save(&cfg);
        }
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir())?;
        let json = self.serialize_json();
        std::fs::write(config_path(), json)
    }

    /// Cifra y guarda la API Key de Gemini con Windows DPAPI (`CryptProtectData`).
    pub fn set_gemini_api_key(&mut self, plain_key: &str) -> bool {
        let trimmed = plain_key.trim();
        if trimmed.is_empty() {
            self.gemini_key_dpapi.clear();
            let _ = self.save();
            return true;
        }
        if let Some(enc_hex) = dpapi_encrypt(trimmed.as_bytes()) {
            self.gemini_key_dpapi = enc_hex;
            let _ = self.save();
            true
        } else {
            false
        }
    }

    /// Descifra en memoria la API Key de Gemini con Windows DPAPI (`CryptUnprotectData`).
    pub fn gemini_api_key(&self) -> Option<String> {
        if self.gemini_key_dpapi.trim().is_empty() {
            return None;
        }
        let raw = dpapi_decrypt(&self.gemini_key_dpapi)?;
        String::from_utf8(raw).ok()
    }

    pub fn has_gemini_key(&self) -> bool {
        !self.gemini_key_dpapi.trim().is_empty()
    }
}

// ---------- Windows DPAPI (CryptProtectData / CryptUnprotectData en crypt32.dll) ----------

#[repr(C)]
struct CryptDataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(
        p_data_in: *const CryptDataBlob,
        sz_data_descr: PCWSTR,
        p_optional_entropy: *const CryptDataBlob,
        pv_reserved: *mut core::ffi::c_void,
        p_prompt_struct: *mut core::ffi::c_void,
        dw_flags: u32,
        p_data_out: *mut CryptDataBlob,
    ) -> i32;

    fn CryptUnprotectData(
        p_data_in: *const CryptDataBlob,
        ppsz_data_descr: *mut PWSTR,
        p_optional_entropy: *const CryptDataBlob,
        pv_reserved: *mut core::ffi::c_void,
        p_prompt_struct: *mut core::ffi::c_void,
        dw_flags: u32,
        p_data_out: *mut CryptDataBlob,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(h_mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

type PWSTR = *mut u16;

fn dpapi_encrypt(plain: &[u8]) -> Option<String> {
    if plain.is_empty() {
        return None;
    }
    unsafe {
        let mut buf = plain.to_vec();
        let input = CryptDataBlob {
            cb_data: buf.len() as u32,
            pb_data: buf.as_mut_ptr(),
        };
        let mut output = CryptDataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = CryptProtectData(
            &input,
            w!("EsmeGeminiKey"),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 || output.pb_data.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(output.pb_data, output.cb_data as usize);
        let mut hex = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            hex.push_str(&format!("{b:02x}"));
        }
        let _ = LocalFree(output.pb_data as *mut _);
        Some(hex)
    }
}

fn dpapi_decrypt(hex_str: &str) -> Option<Vec<u8>> {
    let hex = hex_str.trim();
    if hex.is_empty() || hex.len() % 2 != 0 {
        return None;
    }
    let mut enc = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let b = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
        enc.push(b);
    }
    unsafe {
        let input = CryptDataBlob {
            cb_data: enc.len() as u32,
            pb_data: enc.as_mut_ptr(),
        };
        let mut output = CryptDataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 || output.pb_data.is_null() {
            return None;
        }
        let plain = std::slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec();
        let _ = LocalFree(output.pb_data as *mut _);
        Some(plain)
    }
}

/// Acceso directo en la carpeta Inicio del usuario (shell:startup).
fn startup_link() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(base.join("Microsoft\\Windows\\Start Menu\\Programs\\Startup\\Esme.lnk"))
}

/// Valor que ponían en el Registro las versiones anteriores ("Esme" o "Noosa").
fn had_run_value(name: PCWSTR) -> bool {
    let mut size = 0u32;
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, name, RRF_RT_REG_SZ, None, None, Some(&mut size)) == ERROR_SUCCESS }
}

pub fn autostart_enabled() -> bool {
    startup_link().map(|p| p.exists()).unwrap_or(false)
}

/// Iniciar con Windows mediante un acceso directo en la carpeta Inicio (como la mayoría de las apps).
pub fn set_autostart(enable: bool) {
    let Some(link) = startup_link() else { return };
    if !enable {
        let _ = std::fs::remove_file(&link);
        return;
    }
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    unsafe {
        let Ok(exe) = std::env::current_exe() else { return };
        let Ok(sl) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return };
        let _ = sl.SetPath(&HSTRING::from(exe.as_os_str()));
        if let Some(dir) = exe.parent() {
            let _ = sl.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()));
        }
        let _ = sl.SetDescription(&HSTRING::from("Esme - barra dinámica"));
        if let Ok(pf) = sl.cast::<IPersistFile>() {
            let _ = pf.Save(&HSTRING::from(link.as_os_str()), true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_config_completa() {
        let j = r#"{"mascota":true,"mascota_tipo":"esme","notch_minimo":false,"transparente":false,"mover_monitor":true,"juegos_extra":[],"tema_menu":"claro"}"#;
        let c = Config::deserialize_json(j).expect("config válida");
        assert_eq!(c.tema_menu, "claro");
    }
}
