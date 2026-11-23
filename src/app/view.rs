//! Ventana, región, armado del frame y modo juego.

use crate::*;

impl App {
    // ---------- Ventana y dibujo ----------

    /// Ubica la ventana arriba al centro del monitor principal y ajusta el lienzo al DPI.
    pub(crate) fn layout(&mut self) {
        unsafe {
            let mon = self.monitor;
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            self.scale = dx as f32 / 96.0;
            self.win_w = (CANVAS_W * self.scale).ceil() as i32;
            self.win_h = (CANVAS_H * self.scale).ceil() as i32;
            let r = mi.rcMonitor;
            let x = r.left + ((r.right - r.left) - self.win_w) / 2;
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, r.top, self.win_w, self.win_h, SWP_NOACTIVATE | SWP_SHOWWINDOW);

            if let Some(rd) = self.renderer.as_mut() {
                if rd.resize(self.win_w as u32, self.win_h as u32, self.scale).is_err() {
                    self.renderer = None;
                }
            }
            if self.renderer.is_none() {
                self.create_renderer();
            }
            self.last_region = (-1, -1, -1);
        }
    }

    pub(crate) fn create_renderer(&mut self) {
        self.renderer = Renderer::new(self.hwnd, self.win_w as u32, self.win_h as u32, self.scale).ok();
        if let (Some(r), Some(m)) = (self.renderer.as_mut(), self.media.as_ref()) {
            r.set_art(m.thumb.clone());
        }
    }

    /// La región de la ventana sigue a lo visible: fuera de ella los clics y el hover
    /// pasan a las ventanas de abajo.
    pub(crate) fn update_region(&mut self, shape_w: f32, shape_h: f32, ear: f32) {
        let cx = CANVAS_W / 2.0;
        let mut l = cx - shape_w / 2.0 - ear;
        let mut r = cx + shape_w / 2.0 + ear;
        let mut b = shape_h;
        if self.cfg.transparente && (self.expand.value > 0.02 || self.expand.target > 0.5) {
            let area = self.layout_now().area;
            l = l.min(area.l);
            r = r.max(area.r);
            b = b.max(area.b);
        }
        let key = ((l * self.scale).floor() as i32, (r * self.scale).ceil() as i32, (b * self.scale).ceil() as i32 + 1);
        if key == self.last_region {
            return;
        }
        self.last_region = key;
        unsafe {
            let rgn = CreateRectRgn(key.0, 0, key.1, key.2);
            let _ = SetWindowRgn(self.hwnd, Some(rgn), false);
        }
    }

    pub(crate) fn current_line_text(&self) -> String {
        match (self.synced_lines(), self.lyric_current) {
            (Some(lines), Some(i)) => lines.get(i).map(|l| l.1.clone()).unwrap_or_default(),
            (Some(_), None) => "♪".into(),
            _ => String::new(),
        }
    }

    pub(crate) fn redraw_with(&mut self, vsync: bool) {
        DBG_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let w = self.sw.value.max(4.0);
        let h = self.sh.value.max(2.0);
        let radius = (h * 0.42).clamp(2.5, 30.0);
        let ear = (h * 0.28).clamp(2.5, 14.0);
        self.update_region(w, h, ear);

        let line = self.current_line_text();
        let eyes = self.mascot.eyes();
        let (wglyph, wtext) = self.weather_line();
        let events = self.event_lines();
        let tiles: Vec<(&'static str, String)> =
            self.shelf.items.iter().map(|p| (shelf::glyph(p), shelf::file_name(p))).collect();
        let pomo_text = self.pomo.text();
        let collapsed = if let Some(o) = &self.overlay {
            Collapsed::Overlay { glyph: o.glyph, text: &o.text, level: o.level.map(|_| self.level.value) }
        } else if let Some(t) = &self.toast {
            Collapsed::Lyric(t)
        } else if self.pomo_pill() {
            // Pomodoro: cuenta regresiva viva en la píldora (rojo tomate = foco, verde = descanso).
            let glyph = if self.pomo.mode == productivity::Mode::Focus { "\u{E916}" } else { "\u{E708}" };
            Collapsed::Overlay { glyph, text: &pomo_text, level: Some(self.pomo.progress()) }
        } else if self.lyric_pill() {
            Collapsed::Lyric(&line)
        } else if self.cfg.notch_minimo {
            Collapsed::Minimal
        } else if self.playing() {
            Collapsed::Media { time: &self.time_text }
        } else if self.cfg.pet().is_some() || self.mascot_vis.value > 0.01 {
            Collapsed::Mascot { time: &self.time_text, eyes, pet: self.cfg.pet().unwrap_or(self.last_pet) }
        } else {
            Collapsed::Clock(&self.time_text)
        };

        let (pos_s, dur_s);
        let expanded = match &self.media {
            Some(m) if !m.title.is_empty() => {
                let pos = m.position_now();
                pos_s = ui::fmt_time(pos);
                dur_s = ui::fmt_time(m.duration);
                let lyrics = if self.lyrics_on {
                    Some(match &self.lyrics {
                        None | Some(Lyrics::Loading) => LyricsView::Message("Buscando letra…"),
                        Some(Lyrics::NotFound) => LyricsView::Message("No encontré la letra de esta canción"),
                        Some(Lyrics::Mix) => LyricsView::Message("Es una mezcla larga: no tiene una letra"),
                        Some(Lyrics::Error) => LyricsView::Message("Sin conexión con lrclib.net"),
                        Some(Lyrics::Synced(lines)) => LyricsView::Synced {
                            lines,
                            scroll: self.lyric_scroll.value,
                            current: self.lyric_current.unwrap_or(usize::MAX),
                        },
                        Some(Lyrics::Plain(lines)) => LyricsView::Plain { lines, scroll: self.lyric_scroll.value },
                    })
                } else {
                    None
                };
                Expanded::Media(MediaView {
                    title: &m.title,
                    artist: &m.artist,
                    playing: m.playing,
                    can_prev: m.can_prev,
                    can_next: m.can_next,
                    progress: if m.duration > 0.5 { Some((pos / m.duration) as f32) } else { None },
                    pos: pos_s,
                    dur: dur_s,
                    lyrics,
                })
            }
            _ => Expanded::Clock { time: &self.time_text, date: &self.date_text },
        };

        let frame = Frame {
            cx: CANVAS_W / 2.0,
            shape_w: w,
            shape_h: h,
            radius,
            ear,
            expand: self.expand.value,
            transparent: self.cfg.transparente,
            collapsed,
            expanded,
            layout: self.layout_now(),
            hover: self.hover_btn,
            mascot_vis: self.mascot_vis.value,
            accent: match self.cfg.pet() {
                Some(config::Pet::Esme) => (1.0, 0.72, 0.32),
                Some(config::Pet::Gato) => (0.92, 0.94, 1.0),
                None => (0.55, 0.72, 1.0),
            },
            clip: if self.clip_on { Some(ClipView { items: &self.clip.items, copied: self.clip_copied }) } else { None },
            shelf: if self.shelf_on { Some(ShelfView { tiles: &tiles, drop_hover: self.drop_hover }) } else { None },
            today: if self.today_on {
                Some(TodayView {
                    pomo_label: if self.pomo.mode == productivity::Mode::Focus { "Pomodoro · foco" } else { "Pomodoro · descanso" },
                    pomo_time: self.pomo.text(),
                    pomo_running: self.pomo.running(),
                    pomo_progress: self.pomo.progress(),
                    water: self.water.glasses,
                    goal: self.cfg.agua_meta.max(1),
                    weather_glyph: wglyph,
                    weather: &wtext,
                    events: &events,
                    cal_hint: &self.cal_msg,
                })
            } else {
                None
            },
        };
        let ok = match self.renderer.as_mut() {
            Some(r) => r.draw(&frame, vsync).is_ok(),
            None => false,
        };
        if !ok {
            // Dispositivo perdido (driver reiniciado, etc.): se recrea.
            self.create_renderer();
        }
    }

    pub(crate) fn redraw(&mut self) {
        self.redraw_with(false);
    }

    /// Juego o video a pantalla completa en el monitor del notch: nos vamos al otro
    /// monitor (si la opción está activa y hay uno libre) o nos escondemos.
    pub(crate) fn check_fullscreen(&mut self) {
        let games: Vec<String> = fullscreen::JUEGOS_BASE
            .iter()
            .map(|s| s.to_string())
            .chain(self.cfg.juegos_extra.iter().map(|s| s.trim().to_lowercase()))
            .collect();
        let full = fullscreen::fullscreen_monitors(self.hwnd, &games);
        let mons = fullscreen::monitors();
        let Some(&primary) = mons.first() else { return };
        let is_full = |m: &HMONITOR| full.contains(&(m.0 as isize));
        let target = if !is_full(&primary) {
            Some(primary)
        } else if self.cfg.mover_monitor {
            mons.iter().skip(1).find(|m| !is_full(m)).copied()
        } else {
            None
        };
        unsafe {
            match target {
                None => {
                    if !self.hidden {
                        self.hidden = true;
                        self.hover = false;
                        self.hover_btn = None;
                        let _ = ShowWindow(self.hwnd, SW_HIDE);
                        self.retarget();
                    }
                }
                Some(m) => {
                    let moved = m.0 != self.monitor.0;
                    if moved {
                        self.monitor = m;
                        self.layout();
                    }
                    if self.hidden {
                        self.hidden = false;
                        let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                    }
                    if moved || !self.is_animating() {
                        self.redraw();
                    }
                }
            }
        }
    }

    /// Redibuja solo si no hay una animación en curso (que ya está dibujando).
    pub(crate) fn refresh(&mut self) {
        if self.hidden {
            return;
        }
        if !self.is_animating() {
            self.redraw();
        }
    }

    /// En reposo devolvemos al sistema las páginas que no se usan (sobre todo las del
    /// driver de la GPU). Vuelven solas en la próxima animación.
    pub(crate) fn schedule_trim(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), T_TRIM, TRIM_DELAY_MS, None);
        }
    }
}
