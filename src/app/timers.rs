//! Timers: reloj, barra de progreso, letras, mascota y avisos.

use crate::*;

impl App {
    // ---------- Timers ----------

    pub(crate) fn update_timers(&mut self) {
        unsafe {
            // Barra de progreso: solo con el panel abierto y algo sonando.
            let need_progress = self.playing() && self.want_expanded();
            if need_progress != self.progress_timer {
                self.progress_timer = need_progress;
                if need_progress {
                    SetTimer(Some(self.hwnd), T_PROGRESS, 500, None);
                } else {
                    let _ = KillTimer(Some(self.hwnd), T_PROGRESS);
                }
            }
            // Pestañeo: solo si la mascota está a la vista y despierta.
            let need_blink = self.mascot_shown() && !self.want_expanded() && self.mascot.mood == Mood::Awake;
            if need_blink && !self.blink_timer {
                SetTimer(Some(self.hwnd), T_BLINK, self.mascot.next_blink_ms(), None);
            } else if !need_blink && self.blink_timer {
                let _ = KillTimer(Some(self.hwnd), T_BLINK);
            }
            self.blink_timer = need_blink;
        }
        self.schedule_lyric();
    }

    /// Programa un timer exacto para el cambio de la próxima línea de la letra.
    pub(crate) fn schedule_lyric(&mut self) {
        let hwnd = self.hwnd;
        let pos = self.media.as_ref().map(|m| m.position_now()).unwrap_or(0.0);
        let cur = self.synced_lines().map(|lines| lines.iter().rposition(|l| l.0 <= pos + 0.12));
        let cur = cur.flatten();
        if cur != self.lyric_current {
            self.lyric_current = cur;
            let was = self.is_animating();
            self.lyric_scroll.set_target(cur.unwrap_or(0) as f32);
            if !was {
                self.last_tick = self.now();
            }
        }
        let next = match (self.synced_lines(), self.lyrics_on && self.playing()) {
            (Some(lines), true) => lines.iter().find(|l| l.0 > pos + 0.12).map(|l| l.0),
            _ => None,
        };
        unsafe {
            match next {
                Some(t) => {
                    let ms = (((t - 0.12 - pos) * 1000.0).max(15.0)) as u32;
                    SetTimer(Some(hwnd), T_LYRIC, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(hwnd), T_LYRIC);
                }
            }
        }
    }

    pub(crate) fn update_clock(&mut self) {
        const DIAS: [&str; 7] = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];
        const MESES: [&str; 12] = [
            "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre",
            "diciembre",
        ];
        unsafe {
            let t = GetLocalTime();
            self.time_text = format!("{:02}:{:02}", t.wHour, t.wMinute);
            self.date_text = format!(
                "{}, {} de {}",
                DIAS[(t.wDayOfWeek % 7) as usize],
                t.wDay,
                MESES[((t.wMonth.max(1) - 1) % 12) as usize]
            );
            // Próximo disparo justo al cambiar el minuto: un solo timer por minuto.
            let ms = (60 - t.wSecond as u32) * 1000 - t.wMilliseconds as u32 + 30;
            SetTimer(Some(self.hwnd), T_CLOCK, ms, None);
        }
    }

    pub(crate) fn on_timer(&mut self, id: usize) {
        unsafe {
            if id != T_PROGRESS && id != T_HOVERCHECK && id != T_POMO && id != T_DATA {
                let _ = KillTimer(Some(self.hwnd), id);
            }
        }
        match id {
            T_EXPAND if self.hover => self.retarget(),
            T_COLLAPSE if !self.hover => {
                self.hover_btn = None;
                self.retarget();
            }
            T_CLOCK => {
                self.update_clock();
                // Contraído en modo mínimo el reloj no se ve: no hay nada que redibujar.
                let visible = !(self.cfg.notch_minimo && !self.lyric_pill() && self.expand.value == 0.0);
                if visible {
                    self.refresh();
                }
            }
            T_TRIM if !self.is_animating() => unsafe {
                let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
            },
            T_PROGRESS => self.refresh(),
            T_LYRIC => {
                self.schedule_lyric();
                self.refresh();
            }
            T_BLINK => {
                self.blink_timer = false;
                self.update_gaze();
                if self.mascot.blink() {
                    unsafe { SetTimer(Some(self.hwnd), T_BLINK_STEP, mascot::BLINK_STEP_MS, None) };
                    self.refresh();
                }
                self.update_timers();
            }
            T_BLINK_STEP => {
                if self.mascot.blink_advance() {
                    unsafe { SetTimer(Some(self.hwnd), T_BLINK_STEP, mascot::BLINK_STEP_MS, None) };
                }
                self.refresh();
            }
            T_IDLE => self.check_idle(),
            T_FS => self.check_fullscreen(),
            T_POMO => self.on_pomo_tick(),
            T_DATA => self.refresh_data(),
            T_OVERLAY => {
                self.overlay = None;
                self.retarget();
                self.pop_eyes();
                self.refresh();
            }
            T_TOAST => {
                self.toast = None;
                if std::mem::take(&mut self.greet_after_toast) {
                    // La mascota aparece saludando (^ ^ con saltito).
                    self.mascot.greet();
                    unsafe { SetTimer(Some(self.hwnd), T_GREET, mascot::GREET_LEN.as_millis() as u32, None) };
                }
                self.retarget();
                self.refresh();
            }
            T_DROPCLOSE => {
                self.drop_open = false;
                self.retarget();
            }
            T_COPIED => {
                self.clip_copied = None;
                self.refresh();
            }
            T_HOVERCHECK => {
                if !self.cursor_inside() {
                    self.on_mouse_leave();
                }
            }
            T_GREET => {
                self.mascot.end_greet();
                self.update_timers();
                self.refresh();
            }
            _ => {}
        }
    }

    /// Mascota: se duerme tras unos minutos sin usar la PC y saluda al volver.
    /// Un chequeo barato cada 15 s (despierta) o cada 1 s (dormida).
    pub(crate) fn check_idle(&mut self) {
        if std::env::var_os("NOOSA_DEBUG").is_some() {
            use std::io::Write;
            use std::sync::atomic::Ordering::Relaxed;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("esme-debug.txt")) {
                let _ = writeln!(
                    f,
                    "frames={} media={} anim={} sw={:?} sh={:?} ex={:?} mv={:?} ls={:?}",
                    DBG_FRAMES.load(Relaxed),
                    DBG_MEDIA.load(Relaxed),
                    self.is_animating(),
                    (self.sw.value, self.sw.target, self.sw.velocity),
                    (self.sh.value, self.sh.target, self.sh.velocity),
                    (self.expand.value, self.expand.target),
                    (self.mascot_vis.value, self.mascot_vis.target),
                    (self.lyric_scroll.value, self.lyric_scroll.target),
                );
            }
        }
        let idle_ms = unsafe {
            let mut lii = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            let _ = GetLastInputInfo(&mut lii);
            GetTickCount().wrapping_sub(lii.dwTime)
        };
        let mut next = IDLE_CHECK_AWAKE_MS;
        match self.mascot.mood {
            Mood::Awake if idle_ms as u64 >= mascot::SLEEP_AFTER.as_millis() as u64 => {
                self.mascot.sleep();
                next = IDLE_CHECK_ASLEEP_MS;
                self.update_timers();
                self.refresh();
            }
            Mood::Asleep if idle_ms < 1500 => self.wake(),
            Mood::Asleep => next = IDLE_CHECK_ASLEEP_MS,
            _ => {}
        }
        unsafe {
            SetTimer(Some(self.hwnd), T_IDLE, next, None);
        }
    }

    /// Los ojitos miran sutilmente hacia el cursor cuando está cerca (sin polling: se
    /// actualiza en cada pestañeo, al despertar y al pasar el mouse por el notch).
    pub(crate) fn update_gaze(&mut self) {
        let g = unsafe {
            let mut p = POINT::default();
            let mut wr = RECT::default();
            if GetCursorPos(&mut p).is_err() || GetWindowRect(self.hwnd, &mut wr).is_err() {
                return;
            }
            let ex = wr.left as f32 + (CANVAS_W / 2.0 + self.sw.value / 2.0 - 40.0) * self.scale;
            let ey = wr.top as f32 + 16.0 * self.scale;
            let (dx, dy) = (p.x as f32 - ex, p.y as f32 - ey);
            let dist = (dx * dx + dy * dy).sqrt().max(1.0);
            let reach = 900.0 * self.scale;
            let mag = 1.8 * clamp01(1.0 - dist / reach).sqrt();
            (dx / dist * mag, (dy / dist * mag).min(1.2))
        };
        if self.mascot.look(g) {
            self.refresh();
        }
    }

    pub(crate) fn wake(&mut self) {
        if self.mascot.mood == Mood::Asleep {
            self.mascot.greet();
            unsafe {
                SetTimer(Some(self.hwnd), T_GREET, mascot::GREET_LEN.as_millis() as u32, None);
                SetTimer(Some(self.hwnd), T_IDLE, IDLE_CHECK_AWAKE_MS, None);
            }
            self.update_timers();
            self.refresh();
        }
    }
}
