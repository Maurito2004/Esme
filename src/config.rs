//! Configuración persistente en %APPDATA%\Esme\config.json y autoarranque (HKCU\...\Run).

use nanoserde::{DeJson, SerJson};
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
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
}

impl Default for Config {
    fn default() -> Self {
        Self { mascota: true, mascota_tipo: "esme".into(), notch_minimo: false, transparente: false, mover_monitor: true, juegos_extra: Vec::new(), tema_menu: "oscuro".into() }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Esme")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Primera vez como Esme: traemos los datos de %APPDATA%\Noosa y el inicio automático.
pub fn migrate_from_noosa() {
    let new = data_dir();
    if let Some(parent) = new.parent() {
        let old = parent.join("Noosa");
        if !new.exists() && old.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
    let mut size = 0u32;
    let had_old = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, OLD_RUN_VALUE, RRF_RT_REG_SZ, None, None, Some(&mut size)) == ERROR_SUCCESS
    };
    if had_old {
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, OLD_RUN_VALUE);
        }
        set_autostart(true);
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
}

pub fn autostart_enabled() -> bool {
    let mut size = 0u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, None, Some(&mut size))
            == ERROR_SUCCESS
    }
}

pub fn set_autostart(enable: bool) {
    unsafe {
        if enable {
            let Ok(exe) = std::env::current_exe() else { return };
            let cmd = format!("\"{}\"", exe.display());
            let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                RUN_VALUE,
                REG_SZ.0,
                Some(wide.as_ptr() as *const _),
                (wide.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE);
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
