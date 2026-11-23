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
