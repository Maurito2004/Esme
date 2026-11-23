//! Overlays del sistema y comandos del menú.

use crate::*;

impl App {
    // ---------- Overlays del sistema ----------

    pub(crate) fn show_overlay(&mut self, glyph: &'static str, text: String, level: Option<f32>) {
        if self.hidden {
            return; // en modo juego no molestamos
        }
        self.overlay = Some(SysOverlay { glyph, text, level });
        unsafe {
            SetTimer(Some(self.hwnd), T_OVERLAY, OVERLAY_MS, None);
        }
        self.retarget();
        self.refresh();
    }

    pub(crate) fn on_sys(&mut self, kind: usize, value: isize) {
        match kind {
            system::SYS_VOLUME => {
                let pct = (value & 0xFF) as f32;
                let muted = (value >> 8) & 1 == 1;
                let glyph = if muted || pct == 0.0 {
                    "\u{E74F}"
                } else if pct < 34.0 {
                    "\u{E993}"
                } else if pct < 67.0 {
                    "\u{E994}"
                } else {
                    "\u{E995}"
                };
                let text = if muted { "Mute".to_string() } else { format!("{}", pct as i32) };
                self.show_overlay(glyph, text, Some(if muted { 0.0 } else { pct / 100.0 }));
            }
            system::SYS_AUDIO_DEVICE => {
                if let Some(a) = self.audio.as_mut() {
                    a.attach_default();
                    let name = a.device_name.clone();
                    if !name.is_empty() {
                        self.show_overlay("\u{E7F6}", name, None);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn on_power(&mut self, lp: LPARAM) {
        use windows::Win32::System::Power::{GetSystemPowerStatus, POWERBROADCAST_SETTING, SYSTEM_POWER_STATUS};
        unsafe {
            let s = &*(lp.0 as *const POWERBROADCAST_SETTING);
            if s.PowerSetting != windows::Win32::System::SystemServices::GUID_ACDC_POWER_SOURCE || s.DataLength < 4 {
                return;
            }
            let src = *(s.Data.as_ptr() as *const u32);
            let prev = self.last_ac.replace(src);
            if prev.is_none() || prev == Some(src) {
                return; // el primer aviso es el estado inicial
            }
            let mut st = SYSTEM_POWER_STATUS::default();
            let _ = GetSystemPowerStatus(&mut st);
            if st.BatteryFlag == 128 {
                return; // PC sin batería
            }
            let pct = if st.BatteryLifePercent <= 100 { format!(" · {}%", st.BatteryLifePercent) } else { String::new() };
            if src == 0 {
                self.show_overlay("\u{E945}", format!("Cargando{pct}"), None);
            } else {
                self.show_overlay("\u{E83F}", format!("Con batería{pct}"), None);
            }
        }
    }

    pub(crate) fn on_ocr_done(&mut self) {
        match ocr::take_result() {
            Some(Ok(text)) => {
                let ok = clipboard::set_text(self.hwnd, &text);
                let preview: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).take(60).collect();
                let msg = if ok { format!("✓ Copiado: {preview}") } else { "No pude copiar al portapapeles".into() };
                self.show_toast(msg, 2600);
            }
            Some(Err(e)) => self.show_toast(e, 3200),
            None => {}
        }
    }

    pub(crate) fn clip_on_count(&self) -> usize {
        if self.clip_on { self.clip.items.len() } else { 0 }
    }

    /// Windows avisa una copia nueva: la sumamos al historial.
    pub(crate) fn on_clipboard(&mut self) {
        if let Some(text) = clipboard::read_text(self.hwnd) {
            if self.clip.push(text) && self.clip_on {
                self.retarget();
                self.refresh();
            }
        }
    }

    /// ¿El cursor está dentro de la región visible de la ventana?
    pub(crate) fn cursor_inside(&self) -> bool {
        unsafe {
            let mut p = POINT::default();
            let mut wr = RECT::default();
            if GetCursorPos(&mut p).is_err() || GetWindowRect(self.hwnd, &mut wr).is_err() {
                return true;
            }
            let (l, r, b) = self.last_region;
            let x = p.x - wr.left;
            let y = p.y - wr.top;
            x >= l && x < r && y >= 0 && y < b
        }
    }

    pub(crate) fn on_click(&mut self, lp: LPARAM) {
        if self.expand.value < 0.6 {
            return;
        }
        let (x, y) = self.to_dips(lp);
        let n = match &self.lyrics {
            Some(Lyrics::Synced(v)) => v.len(),
            _ => 0,
        };
        let layout = self.layout_now();
        let Some(btn) = layout.hit(x, y, self.lyric_scroll.value, n, self.clip_on_count(), self.shelf_on_count()) else { return };
        match btn {
            Btn::Play => {
                let _ = self.media_tx.send(Cmd::PlayPause);
            }
            Btn::Prev => {
                let _ = self.media_tx.send(Cmd::Prev);
            }
            Btn::Next => {
                let _ = self.media_tx.send(Cmd::Next);
            }
            Btn::Clip => {
                self.clip_on = !self.clip_on;
                if self.clip_on {
                    self.lyrics_on = false;
                    self.shelf_on = false;
                    self.today_on = false;
                }
                self.retarget();
            }
            Btn::Today => {
                self.today_on = !self.today_on;
                if self.today_on {
                    self.lyrics_on = false;
                    self.clip_on = false;
                    self.shelf_on = false;
                    self.water = std::mem::take(&mut self.water).for_today();
                }
                self.retarget();
            }
            Btn::PomoToggle => self.pomo_toggle(),
            Btn::PomoReset => self.pomo_reset(),
            Btn::WaterAdd => self.water_add(1),
            Btn::WaterSub => self.water_add(-1),
            Btn::Shelf => {
                self.shelf_on = !self.shelf_on;
                if self.shelf_on {
                    self.lyrics_on = false;
                    self.clip_on = false;
                    self.today_on = false;
                }
                self.retarget();
            }
            Btn::ShelfItem(i) => {
                // Clic: abrir con la app predeterminada.
                if let Some(p) = self.shelf.items.get(i) {
                    unsafe {
                        ShellExecuteW(None, w!("open"), &HSTRING::from(p.as_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
                    }
                }
            }
            Btn::ClipItem(i) => {
                if let Some(text) = self.clip.items.get(i).cloned() {
                    if clipboard::set_text(self.hwnd, &text) {
                        self.clip_copied = Some(0);
                        unsafe { SetTimer(Some(self.hwnd), T_COPIED, 900, None) };
                        self.refresh();
                    }
                }
            }
            Btn::Lyrics => {
                self.lyrics_on = !self.lyrics_on;
                if self.lyrics_on {
                    self.clip_on = false;
                    self.shelf_on = false;
                    self.today_on = false;
                }
                if self.lyrics_on && self.lyrics.is_none() {
                    self.fetch_lyrics();
                }
                self.retarget();
            }
            Btn::Progress => {
                if let Some(m) = &self.media {
                    if m.duration > 0.0 && m.can_seek {
                        let t = layout.track;
                        let frac = clamp01((x - t.l) / (t.r - t.l)) as f64;
                        let _ = self.media_tx.send(Cmd::Seek(frac * m.duration));
                    }
                }
            }
            Btn::LyricLine(i) => {
                if let Some(t) = self.synced_lines().and_then(|l| l.get(i)).map(|l| l.0) {
                    if self.media.as_ref().map(|m| m.can_seek).unwrap_or(false) {
                        let _ = self.media_tx.send(Cmd::Seek(t));
                    }
                }
            }
        }
    }

    /// Rueda del mouse: desplaza la letra sin sincronizar.
    pub(crate) fn on_wheel(&mut self, wp: WPARAM) {
        if let Some(Lyrics::Plain(lines)) = &self.lyrics {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            let max = lines.len().saturating_sub(1) as f32;
            let t = (self.lyric_scroll.target - delta * 2.0).clamp(0.0, max);
            let was = self.is_animating();
            self.lyric_scroll.set_target(t);
            if !was {
                self.last_tick = self.now();
            }
        }
    }

    pub(crate) fn on_command(&mut self, cmd: u32) {
        match cmd {
            CMD_AUTOSTART => config::set_autostart(!config::autostart_enabled()),
            CMD_PET_ESME | CMD_PET_GATO | CMD_PET_OFF => {
                let pet = match cmd {
                    CMD_PET_ESME => Some(config::Pet::Esme),
                    CMD_PET_GATO => Some(config::Pet::Gato),
                    _ => None,
                };
                if let Some(p) = pet {
                    self.last_pet = p;
                }
                self.cfg.set_pet(pet);
                let _ = self.cfg.save();
                self.retarget();
                self.refresh();
            }
            CMD_MINIMO => {
                self.cfg.notch_minimo = !self.cfg.notch_minimo;
                let _ = self.cfg.save();
                self.retarget();
            }
            CMD_TRANSPARENTE => {
                self.cfg.transparente = !self.cfg.transparente;
                let _ = self.cfg.save();
                self.last_region = (-1, -1, -1);
                self.retarget();
                self.refresh();
            }
            CMD_MOVER => {
                self.cfg.mover_monitor = !self.cfg.mover_monitor;
                let _ = self.cfg.save();
                self.check_fullscreen();
            }
            CMD_OCR => ocr::start(self.hwnd),
            CMD_ICAL_PASTE => self.paste_ical(),
            CMD_ICAL_CLEAR => {
                self.cfg.ical_url.clear();
                let _ = self.cfg.save();
                self.refresh_data();
            }
            c if (CMD_WATER_GOAL + 1..=CMD_WATER_GOAL + 30).contains(&c) => {
                self.cfg.agua_meta = c - CMD_WATER_GOAL;
                let _ = self.cfg.save();
                self.refresh();
            }
            CMD_TEMA_OSCURO | CMD_TEMA_CLARO => {
                self.cfg.tema_menu = if cmd == CMD_TEMA_CLARO { "claro" } else { "oscuro" }.into();
                let _ = self.cfg.save();
            }
            CMD_SALIR => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            _ => {}
        }
    }
}
