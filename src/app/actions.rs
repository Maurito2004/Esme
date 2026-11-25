//! Overlays del sistema y comandos del menú.

use crate::*;

impl App {
    // ---------- Overlays del sistema ----------

    pub(crate) fn show_overlay(&mut self, glyph: &'static str, text: String, level: Option<f32>) {
        if self.hidden {
            return; // en modo juego no molestamos
        }
        if let Some(v) = level {
            self.level.set_target(v);
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
            system::SYS_VOLUME if !self.cfg.aviso_volumen => {}
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
                    if !name.is_empty() && self.cfg.aviso_audio {
                        self.show_overlay("\u{E7F6}", name, None);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn on_wifi(&mut self) {
        let Some(ev) = wifi::take_event() else { return };
        if !self.cfg.aviso_wifi {
            return;
        }
        match ev {
            wifi::WifiEvent::Connected(ssid) => {
                let text = if ssid.is_empty() {
                    "Wi-Fi conectado".to_string()
                } else {
                    format!("Wi-Fi · {ssid}")
                };
                self.show_overlay("\u{E701}", text, None);
            }
            wifi::WifiEvent::Disconnected(ssid) => {
                let text = if ssid.is_empty() {
                    "Wi-Fi desconectado".to_string()
                } else {
                    format!("Sin Wi-Fi · {ssid}")
                };
                self.show_overlay("\u{EB5E}", text, None);
            }
        }
    }

    pub(crate) fn on_power(&mut self, lp: LPARAM) {
        use windows::Win32::System::Power::{GetSystemPowerStatus, POWERBROADCAST_SETTING, SYSTEM_POWER_STATUS};
        unsafe {
            let s = &*(lp.0 as *const POWERBROADCAST_SETTING);
            if s.DataLength < 4 {
                return;
            }
            use windows::Win32::System::SystemServices::{GUID_BATTERY_PERCENTAGE_REMAINING, GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS};
            let value = *(s.Data.as_ptr() as *const u32);
            if s.PowerSetting == GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS {
                // Brillo (notebooks): ícono de sol y barra.
                let prev = self.last_brightness.replace(value);
                if prev.is_some() && prev != Some(value) && self.cfg.aviso_brillo {
                    self.show_overlay("\u{E706}", format!("{}", value.min(100)), Some(value.min(100) as f32 / 100.0));
                }
                return;
            }
            if s.PowerSetting == GUID_BATTERY_PERCENTAGE_REMAINING {
                // Batería baja: aviso al bajar de 20 % y de 10 % (solo sin cargador).
                let prev = self.last_battery.replace(value);
                let on_battery = self.last_ac.map(|a| a != 0).unwrap_or(false);
                if let Some(p) = prev {
                    for limit in [20u32, 10] {
                        if on_battery && p > limit && value <= limit && self.cfg.aviso_bateria {
                            self.show_overlay("\u{E850}", format!("Batería baja · {value}%"), None);
                        }
                    }
                }
                return;
            }
            if s.PowerSetting != windows::Win32::System::SystemServices::GUID_ACDC_POWER_SOURCE {
                return;
            }
            let src = *(s.Data.as_ptr() as *const u32);
            let prev = self.last_ac.replace(src);
            if prev.is_none() || prev == Some(src) || !self.cfg.aviso_energia {
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
        let Some(btn) = layout.hit(
            x,
            y,
            self.lyric_scroll.value,
            n,
            self.clip_on_count(),
            self.shelf_on_count(),
            self.todo_visible_count(),
            self.active_page(),
        ) else {
            if self.todo_focus {
                self.focus_todo_input(false);
                self.refresh();
            }
            return;
        };
        if !matches!(btn, Btn::TodoInput | Btn::TodoAdd) && self.todo_focus {
            self.focus_todo_input(false);
        }
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
                    self.ask_on = false;
                    self.focus_ask_input(false);
                    self.focus_todo_input(false);
                }
                self.retarget();
            }
            Btn::Today => {
                self.today_on = !self.today_on;
                if self.today_on {
                    self.lyrics_on = false;
                    self.clip_on = false;
                    self.shelf_on = false;
                    self.ask_on = false;
                    self.focus_ask_input(false);
                    self.water = std::mem::take(&mut self.water).for_today();
                    self.ensure_today_loaded();
                } else {
                    self.focus_todo_input(false);
                }
                self.retarget();
            }
            Btn::Ask => self.toggle_ask(false),
            Btn::Mixer => self.toggle_mixer(!self.mixer_on),
            Btn::MixerBar(i) => {
                let v = self.mixer_level_at(i, x);
                self.mixer_set(i, v);
            }
            Btn::MixerMute(i) => {
                self.mixer.toggle_mute(i);
                self.mixer_follow(i);
            }
            Btn::AskMode => {
                let ask = self.ask_mut();
                ask.mode = ask.mode.toggle();
                self.focus_ask_input(true);
                self.refresh();
            }
            Btn::AskInput => {
                self.focus_ask_input(true);
                self.refresh();
            }
            Btn::AskSummarize => self.run_clip_ai_action(true),
            Btn::AskSpellcheck => self.run_clip_ai_action(false),
            Btn::AskCopy => {
                let resp = self.ask.as_ref().map(|a| a.response.clone()).unwrap_or_default();
                if !resp.is_empty() && clipboard::set_text(self.hwnd, &resp) {
                    self.trigger_ask_copied();
                }
            }
            Btn::PomoToggle => self.pomo_toggle(),
            Btn::PomoReset => self.pomo_reset(),
            Btn::WaterAdd => self.water_add(1),
            Btn::WaterSub => self.water_add(-1),
            Btn::TodoInput => {
                self.focus_todo_input(true);
                self.refresh();
            }
            Btn::TodoAdd => self.add_todo_from_input(),
            Btn::TodoToggle(i) => self.toggle_todo(i),
            Btn::TodoDelete(i) => self.delete_todo(i),
            Btn::Shelf => {
                self.shelf_on = !self.shelf_on;
                if self.shelf_on {
                    self.lyrics_on = false;
                    self.clip_on = false;
                    self.today_on = false;
                    self.ask_on = false;
                    self.focus_ask_input(false);
                    self.focus_todo_input(false);
                }
                self.retarget();
            }
            Btn::ShelfClear => {
                self.shelf.clear();
                self.retarget();
                self.refresh();
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
                    self.ask_on = false;
                    self.focus_ask_input(false);
                    self.focus_todo_input(false);
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
            Btn::PageDot(i) => self.select_page(i),
        }
    }

    /// Cambia directamente a la vista elegida desde los puntitos indicadores del panel expandido.
    /// Abre (relee las apps con sonido) o cierra el mezclador.
    pub(crate) fn toggle_mixer(&mut self, on: bool) {
        if on {
            self.mixer = mixer::Mixer::read();
            // Las barritas arrancan en 0 y "se llenan" hasta el volumen real al abrir.
            self.mixer_vis = self
                .mixer
                .channels
                .iter()
                .map(|c| {
                    let mut s = Spring::new(0.0, 260.0, 0.12).with_eps(0.002);
                    s.set_target(if c.muted { 0.0 } else { c.level });
                    s
                })
                .collect();
            self.mixer_drag = None;
            self.mixer_on = true;
            self.lyrics_on = false;
            self.clip_on = false;
            self.shelf_on = false;
            self.today_on = false;
            self.ask_on = false;
            self.focus_ask_input(false);
            self.focus_todo_input(false);
        } else {
            self.mixer_on = false;
            self.mixer.clear();
            self.mixer_vis.clear();
            self.mixer_drag = None;
        }
        self.retarget();
        self.refresh();
    }

    /// Cambia el volumen real de la fila `i` y anima su barrita hasta ahí.
    pub(crate) fn mixer_set(&mut self, i: usize, v: f32) {
        self.mixer.set_level(i, v);
        self.mixer_follow(i);
    }

    /// La barrita `i` sigue al valor real (0 si está en silencio).
    pub(crate) fn mixer_follow(&mut self, i: usize) {
        let target = self.mixer.channels.get(i).map(|c| if c.muted { 0.0 } else { c.level }).unwrap_or(0.0);
        let was = self.is_animating();
        if let Some(s) = self.mixer_vis.get_mut(i) {
            s.set_target(target);
        }
        if !was {
            self.last_tick = self.now();
        }
        self.refresh();
    }

    /// Volumen según la posición x sobre la barra de la fila `i` (con imán en 0 % y 100 %).
    pub(crate) fn mixer_level_at(&self, i: usize, x: f32) -> f32 {
        let bar = self.layout_now().mixer_bar(i);
        let v = ((x - bar.l) / (bar.r - bar.l).max(1.0)).clamp(0.0, 1.0);
        if v < 0.03 { 0.0 } else if v > 0.97 { 1.0 } else { v }
    }

    /// Botón apretado sobre una barrita: empieza a arrastrar (el mouse queda capturado).
    pub(crate) fn mixer_press(&mut self, x: f32, y: f32) -> bool {
        if !self.mixer_on || self.expand.value < 0.6 {
            return false;
        }
        let hit = self.layout_now().hit(x, y, 0.0, 0, 0, 0, 0, self.active_page());
        let Some(Btn::MixerBar(i)) = hit else { return false };
        self.mixer_drag = Some(i);
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
        }
        let v = self.mixer_level_at(i, x);
        self.mixer_set(i, v);
        true
    }

    /// Arrastrando: la barrita sigue al mouse en vivo.
    pub(crate) fn mixer_drag_move(&mut self, wp: WPARAM, lp: LPARAM) {
        let Some(i) = self.mixer_drag else { return };
        if wp.0 & 0x0001 == 0 {
            self.mixer_release();
            return;
        }
        let (x, _) = self.to_dips(lp);
        let v = self.mixer_level_at(i, x);
        if self.mixer.channels.get(i).map(|c| (c.level - v).abs() > 0.004).unwrap_or(false) {
            self.mixer_set(i, v);
        }
    }

    /// Soltó el botón: termina el arrastre. Devuelve true si estaba arrastrando.
    pub(crate) fn mixer_release(&mut self) -> bool {
        if self.mixer_drag.take().is_none() {
            return false;
        }
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
        }
        self.refresh();
        true
    }

    fn select_page(&mut self, page: usize) {
        match page {
            0 => {
                self.lyrics_on = false;
                self.clip_on = false;
                self.shelf_on = false;
                self.today_on = false;
                self.ask_on = false;
                self.focus_ask_input(false);
                self.focus_todo_input(false);
                self.retarget();
            }
            1 => self.toggle_ask(true),
            2 => {
                self.today_on = true;
                self.lyrics_on = false;
                self.clip_on = false;
                self.shelf_on = false;
                self.ask_on = false;
                self.focus_ask_input(false);
                self.water = std::mem::take(&mut self.water).for_today();
                self.ensure_today_loaded();
                self.retarget();
            }
            3 => {
                self.shelf_on = true;
                self.lyrics_on = false;
                self.clip_on = false;
                self.today_on = false;
                self.ask_on = false;
                self.focus_ask_input(false);
                self.focus_todo_input(false);
                self.retarget();
            }
            4 => {
                self.clip_on = true;
                self.lyrics_on = false;
                self.shelf_on = false;
                self.today_on = false;
                self.ask_on = false;
                self.focus_ask_input(false);
                self.focus_todo_input(false);
                self.retarget();
            }
            5 => self.toggle_mixer(true),
            6 if self.has_media() => {
                self.lyrics_on = true;
                self.clip_on = false;
                self.shelf_on = false;
                self.today_on = false;
                self.ask_on = false;
                self.focus_ask_input(false);
                self.focus_todo_input(false);
                if self.lyrics.is_none() {
                    self.fetch_lyrics();
                }
                self.retarget();
            }
            _ => {}
        }
    }

    /// Rueda del mouse: desplaza la lista To-Do en "Hoy" o la letra sin sincronizar.
    pub(crate) fn on_wheel(&mut self, wp: WPARAM) {
        let delta = ((wp.0 >> 16) & 0xFFFF) as i16;
        if self.mixer_on {
            if let Some(Btn::MixerBar(i) | Btn::MixerMute(i)) = self.hover_btn {
                if let Some(cur) = self.mixer.channels.get(i).map(|c| c.level) {
                    let step = if delta > 0 { 0.05 } else { -0.05 };
                    self.mixer_set(i, ((cur + step) * 20.0).round() / 20.0);
                }
                return;
            }
        }
        if self.today_on && self.cfg.hoy_todo && self.todo.items.len() > ui::TODO_VISIBLE {
            let max = self.todo.items.len().saturating_sub(ui::TODO_VISIBLE);
            if delta < 0 {
                self.todo_scroll = (self.todo_scroll + 1).min(max);
            } else if delta > 0 {
                self.todo_scroll = self.todo_scroll.saturating_sub(1);
            }
            self.refresh();
            return;
        }
        if let Some(Lyrics::Plain(lines)) = &self.lyrics {
            let steps = delta as f32 / 120.0;
            let max = lines.len().saturating_sub(1) as f32;
            let t = (self.lyric_scroll.target - steps * 2.0).clamp(0.0, max);
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
            CMD_SETTINGS => self.open_settings(),
            CMD_ASK => self.toggle_ask(true),
            CMD_ICAL_PASTE => self.paste_ical(),
            CMD_ICAL_CLEAR => {
                self.cfg.ical_url.clear();
                let _ = self.cfg.save();
                self.refresh_data();
            }
            CMD_GEMINI_PASTE => self.paste_gemini_key(),
            CMD_GEMINI_CLEAR => {
                let _ = self.cfg.set_gemini_api_key("");
                self.show_toast("API Key de Gemini borrada".into(), 2200);
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
