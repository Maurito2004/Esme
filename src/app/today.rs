//! Panel "Hoy" (Fase 5): Pomodoro, agua, clima (MET Norway) y calendario (iCal).

use crate::*;

impl App {
    /// Pide clima y calendario en hilos de fondo (no bloquea la UI).
    pub(crate) fn refresh_data(&mut self) {
        let hwnd = self.hwnd.0 as isize;
        let post = move || unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_DATA, WPARAM(0), LPARAM(0));
        };
        let inbox = self.inbox.clone();
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
            }
            let res = match weather::locate() {
                None => Err("Activá la ubicación de Windows para ver el clima".to_string()),
                Some((lat, lon)) => weather::fetch(lat, lon).ok_or_else(|| "Sin conexión con el clima".to_string()),
            };
            inbox.lock().unwrap().weather = Some(res);
            post();
        });
        if !self.rates.is_fresh() {
            let inbox = self.inbox.clone();
            std::thread::spawn(move || {
                if let Some(r) = calc::refresh_rates_if_needed() {
                    inbox.lock().unwrap().rates = Some(r);
                    post();
                }
            });
        }
        let url = self.cfg.ical_url.clone();
        if !url.is_empty() {
            let inbox = self.inbox.clone();
            std::thread::spawn(move || {
                let res = ical::fetch(&url);
                inbox.lock().unwrap().events = Some(res);
                post();
            });
        } else {
            self.events.clear();
            self.cal_msg = "Conectá tu calendario desde la bandeja".into();
        }
    }

    pub(crate) fn on_data(&mut self) {
        let mut inbox = std::mem::take(&mut *self.inbox.lock().unwrap());
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
            self.rates = r;
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
                productivity::Mode::Focus => "¡Terminó el foco! Descanso de 5 minutos",
                productivity::Mode::Break => "¡A enfocarse! 25 minutos",
            };
            self.show_toast(msg.into(), 3500);
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
        self.refresh_data();
        self.show_toast("Calendario conectado ✓".into(), 2500);
    }
}
