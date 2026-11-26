//! Configuración persistente en %APPDATA%\Esme\config.json y autoarranque (acceso directo en Inicio).

use nanoserde::{DeJson, SerJson};
use std::path::PathBuf;
use windows::core::HSTRING;
use windows::Security::Credentials::{PasswordCredential, PasswordVault};


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
    /// Píldora flotante: separada del borde de arriba, con las 4 esquinas redondeadas.
    #[nserde(default)]
    pub flotante: bool,
    /// Línea de luz fina en el borde inferior de la píldora cerrada.
    #[nserde(default = "true")]
    pub linea_luz: bool,
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
    /// Obsoleto (hasta v1.0.1 la key iba cifrada acá). Se vacía al arrancar; ahora vive en el
    /// Administrador de credenciales de Windows.
    #[nserde(default)]
    pub gemini_key_dpapi: String,
    /// Hay una API Key de Gemini guardada en el Administrador de credenciales de Windows.
    #[nserde(default)]
    pub gemini_key_guardada: bool,

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
    #[nserde(default = "true")]
    pub hoy_todo: bool,
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
    #[nserde(default = "true")]
    pub aviso_wifi: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mascota: true,
            mascota_tipo: "esme".into(),
            notch_minimo: false,
            transparente: false,
            flotante: false,
            linea_luz: true,
            mover_monitor: true,
            juegos_extra: Vec::new(),
            tema_menu: "oscuro".into(),
            agua_meta: 8,
            ical_url: String::new(),
            gemini_model: String::new(),
            gemini_familia: "flash".into(),
            gemini_key_dpapi: String::new(),
            gemini_key_guardada: false,
            tamano: 100,
            ciudad: String::new(),
            ciudad_lat: 0.0,
            ciudad_lon: 0.0,
            hoy_pomodoro: true,
            hoy_agua: true,
            hoy_clima: true,
            hoy_calendario: true,
            hoy_todo: true,
            pomo_foco: 25,
            pomo_descanso: 5,
            aviso_volumen: true,
            aviso_audio: true,
            aviso_teclado: true,
            aviso_energia: true,
            aviso_brillo: true,
            aviso_bateria: true,
            aviso_wifi: true,
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

fn backup_path() -> PathBuf {
    data_dir().join("config.bak.json")
}

/// Lee y valida un archivo de configuración. Si existe pero está ocupado, reintenta hasta ~3 s.
fn read_config(path: &std::path::Path) -> Option<Config> {
    read_text(path).and_then(|s| Config::deserialize_json(&s).ok())
}

/// Lee un archivo de datos de Esme. Si existe pero está ocupado (antivirus, limpiador, inicio de
/// Windows), reintenta hasta ~3 s en vez de rendirse y arrancar con datos vacíos.
pub fn read_text(path: &std::path::Path) -> Option<String> {
    for _ in 0..15 {
        match std::fs::read_to_string(path) {
            Ok(s) => return Some(s.trim_start_matches('\u{feff}').to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    }
    None
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
        // Al prender la PC, el antivirus o un limpiador pueden tener el archivo ocupado un instante:
        // reintentamos y, si el archivo está dañado, usamos la copia de respaldo. Nunca la de fábrica
        // mientras exista una configuración guardada que se pueda leer.
        let cfg = read_config(&config_path())
            .or_else(|| read_config(&backup_path()))
            .unwrap_or_default();
        let mut cfg: Config = cfg;
        // La key cifrada de versiones viejas ya no se usa: hay que volver a pegarla una vez.
        if !cfg.gemini_key_dpapi.is_empty() {
            cfg.gemini_key_dpapi.clear();
            let _ = cfg.save();
        }
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

    /// Guardado seguro: escribe un archivo temporal y lo reemplaza de una sola vez, así un apagado
    /// a mitad de camino no deja `config.json` cortado. La versión anterior queda como respaldo.
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir())?;
        let path = config_path();
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.serialize_json())?;
        if read_config(&path).is_some() {
            let _ = std::fs::copy(&path, backup_path());
        }
        std::fs::rename(&tmp, &path)
    }

    /// Guarda la API Key de Gemini en el Administrador de credenciales de Windows
    /// (Credenciales web → "Esme"). Vacía = borrarla.
    pub fn set_gemini_api_key(&mut self, plain_key: &str) -> bool {
        let trimmed = plain_key.trim();
        vault_remove();
        if trimmed.is_empty() {
            self.gemini_key_guardada = false;
            let _ = self.save();
            return true;
        }
        if vault_set(trimmed) {
            self.gemini_key_guardada = true;
            let _ = self.save();
            true
        } else {
            false
        }
    }

    /// Lee la API Key de Gemini del Administrador de credenciales (solo en memoria, al usarla).
    pub fn gemini_api_key(&self) -> Option<String> {
        if !self.gemini_key_guardada {
            return None;
        }
        vault_get()
    }

    pub fn has_gemini_key(&self) -> bool {
        self.gemini_key_guardada
    }
}

// ---------- Administrador de credenciales de Windows (Windows.Security.Credentials) ----------

const VAULT_RESOURCE: &str = "Esme";
const VAULT_USER: &str = "Gemini API Key";

fn vault_set(key: &str) -> bool {
    let res = (|| -> windows::core::Result<()> {
        let vault = PasswordVault::new()?;
        let cred = PasswordCredential::CreatePasswordCredential(
            &HSTRING::from(VAULT_RESOURCE),
            &HSTRING::from(VAULT_USER),
            &HSTRING::from(key),
        )?;
        vault.Add(&cred)
    })();
    res.is_ok()
}

fn vault_get() -> Option<String> {
    let vault = PasswordVault::new().ok()?;
    let cred = vault.Retrieve(&HSTRING::from(VAULT_RESOURCE), &HSTRING::from(VAULT_USER)).ok()?;
    cred.RetrievePassword().ok()?;
    let key = cred.Password().ok()?.to_string_lossy();
    (!key.is_empty()).then_some(key)
}

fn vault_remove() {
    if let Ok(vault) = PasswordVault::new() {
        if let Ok(cred) = vault.Retrieve(&HSTRING::from(VAULT_RESOURCE), &HSTRING::from(VAULT_USER)) {
            let _ = vault.Remove(&cred);
        }
    }
}

/// Acceso directo en la carpeta Inicio del usuario (shell:startup).
fn startup_link() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(base.join("Microsoft\\Windows\\Start Menu\\Programs\\Startup\\Esme.lnk"))
}

pub fn autostart_enabled() -> bool {
    startup_link().map(|p| p.exists()).unwrap_or(false)
}

/// Si el inicio con Windows está activado pero el acceso directo apunta a otro exe (porque el
/// usuario movió o actualizó Esme en otra carpeta), lo corrige. Si ya apunta acá, no toca nada.
pub fn fix_autostart_target() {
    let Some(link) = startup_link() else { return };
    if !link.exists() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    use windows::core::Interface;
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    let target = unsafe {
        let Ok(sl) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return };
        let Ok(pf) = sl.cast::<IPersistFile>() else { return };
        if pf.Load(&HSTRING::from(link.as_os_str()), STGM_READ).is_err() {
            return;
        }
        let mut buf = [0u16; 520];
        if sl.GetPath(&mut buf, std::ptr::null_mut(), 0).is_err() {
            return;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n])
    };
    if !target.eq_ignore_ascii_case(&exe.to_string_lossy()) {
        set_autostart(true);
    }
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
    fn guarda_y_lee_con_respaldo() {
        let dir = std::env::temp_dir().join(format!("esme-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("c.json");
        assert!(read_config(&p).is_none());
        std::fs::write(&p, "{\"ciudad\":\"Córd").unwrap(); // archivo cortado
        assert!(read_config(&p).is_none());
        let mut c = Config::default();
        c.ciudad = "Córdoba".into();
        std::fs::write(&p, c.serialize_json()).unwrap();
        assert_eq!(read_config(&p).unwrap().ciudad, "Córdoba");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lee_config_completa() {
        let j = r#"{"mascota":true,"mascota_tipo":"esme","notch_minimo":false,"transparente":false,"mover_monitor":true,"juegos_extra":[],"tema_menu":"claro"}"#;
        let c = Config::deserialize_json(j).expect("config válida");
        assert_eq!(c.tema_menu, "claro");
    }
}
