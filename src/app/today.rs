//! Panel "Hoy" (Fase 5): Pomodoro, agua, clima (MET Norway) y calendario (iCal).

use crate::*;

impl App {
    /// Canal compartido inicializado de forma diferida (0 asignaciones al arrancar).
    pub(crate) fn inbox_slot(&mut self) -> Arc<Mutex<DataInbox>> {
        self.inbox.get_or_insert_with(|| Arc::new(Mutex::new(DataInbox::default()))).clone()
    }

    /// Carga diferida de clima y calendario: recién la primera vez que se abre el panel "Hoy".
    pub(crate) fn ensure_today_loaded(&mut self) {
        if self.today_loaded {
            return;
        }
        self.today_loaded = true;
        self.weather_msg = "Buscando el clima…".into();
        self.refresh_data();
        unsafe {
            SetTimer(Some(self.hwnd), T_DATA, DATA_REFRESH_MS, None);
        }
    }

    /// Pide clima y calendario en hilos de fondo (no bloquea la UI).
    pub(crate) fn refresh_data(&mut self) {
        let hwnd = self.hwnd.0 as isize;
        let post = move || unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_DATA, WPARAM(0), LPARAM(0));
        };
        let inbox = self.inbox_slot();
        // Ciudad elegida en Ajustes; si no hay, la ubicación de Windows.
        let city = (!self.cfg.ciudad.is_empty() && (self.cfg.ciudad_lat != 0.0 || self.cfg.ciudad_lon != 0.0))
            .then_some((self.cfg.ciudad_lat, self.cfg.ciudad_lon));
        if self.cfg.hoy_clima {
            std::thread::spawn(move || {
                let pos = city.or_else(|| {
                    unsafe {
                        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
                    }
                    weather::locate()
                });
                let res = match pos {
                    None => Err("Elegí tu ciudad en Ajustes → Hoy".to_string()),
                    Some((lat, lon)) => weather::fetch(lat, lon).ok_or_else(|| "Sin conexión con el clima".to_string()),
                };
                inbox.lock().unwrap().weather = Some(res);
                post();
            });
        }
        let url = self.cfg.ical_url.clone();
        if !url.is_empty() && self.cfg.hoy_calendario {
            let inbox = self.inbox_slot();
            std::thread::spawn(move || {
                let res = ical::fetch(&url);
                inbox.lock().unwrap().events = Some(res);
                post();
            });
        } else {
            self.events.clear();
            self.cal_msg = "Conectá tu calendario en Ajustes → Hoy".into();
        }
    }

    pub(crate) fn on_data(&mut self) {
        let Some(slot) = self.inbox.as_ref() else { return };
        let mut inbox = std::mem::take(&mut *slot.lock().unwrap());
        if let Some(w) = inbox.weather.take() {
            match w {
                Ok(w) => {
                    self.weather = Some(w);
                    self.weather_msg.clear();
                }
                Err(e) => self.weather_msg = e,
            }
        }
        if let Some(ev) = inbox.events.take() {
            match ev {
                Ok(v) => {
                    self.cal_msg = if v.is_empty() { "Sin eventos próximos".into() } else { String::new() };
                    self.events = v;
                }
                Err(e) => self.cal_msg = e,
            }
        }
        if let Some(r) = inbox.rates.take() {
            self.rates = Some(r);
        }
        if self.today_on || self.ask_on {
            self.refresh();
        }
    }

    /// Texto del clima para el panel: ("☀", "24° despejado · 6 h: lluvia, 18°–25°").
    pub(crate) fn weather_line(&self) -> (&'static str, String) {
        match &self.weather {
            Some(w) => {
                let (g, now) = weather::describe(&w.now);
                let (_, next) = weather::describe(&w.next);
                let mut s = format!("{:.0}° {now}", w.temp);
                if !next.is_empty() && next != now {
                    s += &format!(" · luego {next}");
                }
                s += &format!(" · {:.0}°–{:.0}°", w.min6, w.max6);
                (g, s)
            }
            None => ("☁", self.weather_msg.clone()),
        }
    }

    /// Próximos eventos como (cuándo, título).
    pub(crate) fn event_lines(&self) -> Vec<(String, String)> {
        let now = ical::now_local();
        ical::upcoming(&self.events, now, 2).iter().map(|e| (ical::when(e, now), e.title.clone())).collect()
    }

    /// Pomodoro corriendo o pausado a mitad: se ve en la píldora contraída.
    pub(crate) fn pomo_pill(&self) -> bool {
        self.pomo.touched()
    }

    pub(crate) fn pomo_toggle(&mut self) {
        self.pomo.toggle();
        unsafe {
            if self.pomo.running() {
                SetTimer(Some(self.hwnd), T_POMO, 1000, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), T_POMO);
            }
        }
        self.retarget();
        self.refresh();
    }

    pub(crate) fn pomo_reset(&mut self) {
        self.pomo.reset();
        unsafe {
            let _ = KillTimer(Some(self.hwnd), T_POMO);
        }
        self.retarget();
        self.refresh();
    }

    /// Un segundo de Pomodoro: actualiza la cuenta y avisa al terminar cada etapa.
    pub(crate) fn on_pomo_tick(&mut self) {
        if let Some(done) = self.pomo.check_finished() {
            unsafe {
                let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONASTERISK);
            }
            let msg = match done {
                productivity::Mode::Focus => {
                    format!("¡Terminó el foco! Descanso de {} minutos", self.pomo.minutes(productivity::Mode::Break))
                }
                productivity::Mode::Break => format!("¡A enfocarse! {} minutos", self.pomo.minutes(productivity::Mode::Focus)),
            };
            self.show_toast(msg, 3500);
        }
        self.refresh();
    }

    pub(crate) fn water_add(&mut self, d: i32) {
        self.water.add(d);
        if d > 0 && self.water.glasses == self.cfg.agua_meta {
            self.show_toast("💧 ¡Llegaste a tu meta de agua de hoy!".into(), 3000);
        }
        self.refresh();
    }

    /// Bienvenida al abrir Esme: "¡Hola! Buenos días / Buenas tardes / Buenas noches".
    pub(crate) fn say_hello(&mut self) {
        let h = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() }.wHour;
        let text = match h {
            5..=11 => "¡Hola! Buenos días ☀",
            12..=19 => "¡Hola! Buenas tardes",
            _ => "¡Hola! Buenas noches ☾",
        };
        self.greet_after_toast = self.cfg.pet().is_some();
        self.show_toast(text.into(), 2600);
    }

    /// Bandeja → Calendario → Pegar URL iCal copiada.
    pub(crate) fn paste_ical(&mut self) {
        let text = clipboard::read_text(self.hwnd).unwrap_or_default();
        let url = text.trim();
        let looks_ok = url.starts_with("https://") && (url.contains(".ics") || url.contains("/ical/"));
        if !looks_ok {
            self.show_toast("Copiá primero la dirección iCal (https://…/basic.ics)".into(), 3500);
            return;
        }
        self.cfg.ical_url = url.to_string();
        let _ = self.cfg.save();
        self.cal_msg = "Cargando calendario…".into();
        self.today_loaded = true;
        self.refresh_data();
        unsafe {
            SetTimer(Some(self.hwnd), T_DATA, DATA_REFRESH_MS, None);
        }
        self.show_toast("Calendario conectado ✓".into(), 2500);
    }

    // ---------- To-Do rápido en "Hoy" (v1.0 — Bloque 1) ----------

    /// Activa o desactiva el foco de teclado sobre el campo rápido del To-Do (sin hooks globales).
    pub(crate) fn focus_todo_input(&mut self, focus: bool) {
        if !focus && !self.todo_focus {
            return;
        }
        self.todo_focus = focus;
        self.todo_caret = focus;
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            if focus {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
                let _ = SetForegroundWindow(self.hwnd);
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.hwnd));
                SetTimer(Some(self.hwnd), T_CARET, 530, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), T_CARET);
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 as isize));
            }
        }
    }

    pub(crate) fn add_todo_from_input(&mut self) {
        if self.todo.add_quick(&self.todo_input) {
            self.todo_input.clear();
            self.todo_scroll = 0;
            self.retarget();
            self.refresh();
        }
    }

    pub(crate) fn toggle_todo(&mut self, visible_row: usize) {
        let idx = self.todo_scroll + visible_row;
        if self.todo.toggle(idx) {
            self.refresh();
        }
    }

    pub(crate) fn delete_todo(&mut self, visible_row: usize) {
        let idx = self.todo_scroll + visible_row;
        if self.todo.remove(idx) {
            let max_scroll = self.todo.items.len().saturating_sub(ui::TODO_VISIBLE);
            self.todo_scroll = self.todo_scroll.min(max_scroll);
            self.retarget();
            self.refresh();
        }
    }

    /// Revisado en `T_CLOCK` (una vez por minuto, 0 timers extra): avisa en la píldora si venció una tarea.
    pub(crate) fn check_todo_due(&mut self) {
        if !self.cfg.hoy_todo {
            return;
        }
        let day = productivity::today();
        let due = self.todo.take_due_alerts(&day, &self.time_text);
        if due.is_empty() {
            return;
        }
        unsafe {
            let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONASTERISK);
        }
        let msg = if due.len() == 1 {
            format!("⏰ {} · {}", due[0].0, due[0].1)
        } else {
            let list: Vec<&str> = due.iter().map(|(_, t)| t.as_str()).collect();
            format!("⏰ {} · {}", due[0].0, list.join(" / "))
        };
        self.show_toast(msg, 4500);
    }

    /// Teclas especiales mientras el input del To-Do tiene foco. Devuelve `true` si la consumió.
    pub(crate) fn on_todo_key_down(&mut self, vk: u32) -> bool {
        if !self.today_on || !self.todo_focus {
            return false;
        }
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_BACK, VK_CONTROL, VK_ESCAPE, VK_RETURN};
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
        if ctrl && vk as u8 == b'V' {
            if let Some(clip_txt) = clipboard::read_text(self.hwnd) {
                let clean: String = clip_txt
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(80usize.saturating_sub(self.todo_input.chars().count()))
                    .collect();
                if !clean.is_empty() {
                    self.todo_input.push_str(&clean);
                    self.todo_caret = true;
                    unsafe { SetTimer(Some(self.hwnd), T_CARET, 530, None) };
                    self.refresh();
                }
            }
            return true;
        }
        match vk as u16 {
            x if x == VK_RETURN.0 => {
                self.add_todo_from_input();
                true
            }
            x if x == VK_ESCAPE.0 => {
                self.focus_todo_input(false);
                self.retarget();
                self.refresh();
                true
            }
            x if x == VK_BACK.0 => {
                if ctrl {
                    self.todo_input.clear();
                } else {
                    self.todo_input.pop();
                }
                self.todo_caret = true;
                unsafe { SetTimer(Some(self.hwnd), T_CARET, 530, None) };
                self.refresh();
                true
            }
            _ => true,
        }
    }

    /// Entrada de caracteres Unicode mientras el input del To-Do tiene foco.
    pub(crate) fn on_todo_char(&mut self, ch: u32) -> bool {
        if !self.today_on || !self.todo_focus {
            return false;
        }
        let Some(c) = char::from_u32(ch) else { return true };
        if c.is_control() {
            return true;
        }
        if self.todo_input.chars().count() < 80 {
            self.todo_input.push(c);
            self.todo_caret = true;
            unsafe { SetTimer(Some(self.hwnd), T_CARET, 530, None) };
            self.refresh();
        }
        true
    }
}
