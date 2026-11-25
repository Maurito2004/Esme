//! Ventana, región, armado del frame y modo juego.

use crate::*;

impl App {
    // ---------- Ventana y dibujo ----------

    /// Centro horizontal actual de la superficie gráfica en DIPs.
    #[inline]
    pub(crate) fn surface_cx(&self) -> f32 {
        self.surf_w / 2.0
    }

    /// Calcula el tamaño objetivo de la superficie gráfica (compacta cuando está contraído,
    /// completa `CANVAS_W × CANVAS_H` al expandir o animar).
    fn target_surface_dips(&self, full: bool) -> (f32, f32) {
        if full {
            (CANVAS_W, CANVAS_H)
        } else {
            let c = self.collapsed_dims();
            // Margen exacto para orejas cóncavas + borde de 1.5 DIP sin reservar 520×340 en VRAM.
            let w = (c.w + 36.0).clamp(180.0, CANVAS_W);
            let h = (c.h + 14.0 + self.float_gap()).clamp(24.0, CANVAS_H);
            (w, h)
        }
    }

    /// Ubica la ventana arriba al centro del monitor activo y ajusta la superficie al DPI.
    pub(crate) fn layout(&mut self) {
        let full = self.want_expanded() || self.is_animating();
        self.apply_surface_layout(full, true);
    }

    /// Cambia dinámicamente el tamaño del HWND + DXGI swapchain entre modo compacto (reposo)
    /// y modo completo (expandido/animación).
    pub(crate) fn ensure_surface(&mut self, full: bool) {
        let (tw, th) = self.target_surface_dips(full);
        let unchanged = (self.surf_w - tw).abs() < 0.5 && (self.surf_h - th).abs() < 0.5 && self.surf_compact == !full;
        if unchanged && self.renderer.is_some() {
            return;
        }
        self.apply_surface_layout(full, false);
        if !full {
            // Re-dibujamos 1 frame limpio en el nuevo tamaño compacto (0% parpadeo).
            self.redraw();
        }
    }

    fn apply_surface_layout(&mut self, full: bool, force_dpi: bool) {
        unsafe {
            let mon = self.monitor;
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            if force_dpi || self.scale <= 0.0 {
                let (mut dx, mut dy) = (96u32, 96u32);
                let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
                // Escala de Windows × tamaño elegido en Ajustes (100/115/130 %).
                self.scale = (dx as f32 / 96.0 * self.cfg.tamano.clamp(100, 130) as f32 / 100.0).max(0.5);
            }
            let (tw, th) = self.target_surface_dips(full);
            self.surf_w = tw;
            self.surf_h = th;
            self.surf_compact = !full;
            self.win_w = (tw * self.scale).ceil() as i32;
            self.win_h = (th * self.scale).ceil() as i32;
            let r = mi.rcMonitor;
            let x = r.left + ((r.right - r.left) - self.win_w) / 2;
            let flags = if self.hidden {
                SWP_NOACTIVATE
            } else {
                SWP_NOACTIVATE | SWP_SHOWWINDOW
            };
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, r.top, self.win_w, self.win_h, flags);

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
        self.renderer = Renderer::new(self.hwnd, self.win_w.max(1) as u32, self.win_h.max(1) as u32, self.scale).ok();
        // Si había carátula activa y se perdió el dispositivo GPU, pedimos re-lectura puntual a SMTC
        // (nunca retenemos el Vec<u8> original en RAM).
        if self.renderer.is_some() && self.media.as_ref().map(|m| m.thumb_id != 0).unwrap_or(false) {
            let _ = self.media_tx.send(Cmd::ResendThumb);
        }
    }

    /// La región de la ventana sigue a lo visible: fuera de ella los clics y el hover
    /// pasan a las ventanas de abajo.
    pub(crate) fn update_region(&mut self, shape_w: f32, shape_h: f32, ear: f32) {
        let cx = self.surface_cx();
        let mut l = cx - shape_w / 2.0 - ear;
        let mut r = cx + shape_w / 2.0 + ear;
        let mut b = shape_h + self.float_gap();
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
        let mut eyes = self.mascot.eyes();
        eyes.scale = self.eye_pop.value;
        let (wglyph, wtext) = self.weather_line();
        let events = self.event_lines();
        let tiles: Vec<(&'static str, String)> =
            self.shelf.items.iter().map(|p| (shelf::glyph(p), shelf::file_name(p))).collect();
        let mixer_rows: Vec<(String, f32, bool)> = self
            .mixer
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), self.mixer_vis.get(i).map(|s| s.value).unwrap_or(c.level), c.muted))
            .collect();
        let pomo_text = self.pomo.text();
        let qtimer = self.quick_timer_status();
        let qtimer_text = qtimer.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
        let inline_prev = self.ask_inline_preview();
        let collapsed = if let Some(o) = &self.overlay {
            Collapsed::Overlay { glyph: o.glyph, text: &o.text, level: o.level.map(|_| self.level.value) }
        } else if let Some(t) = &self.toast {
            Collapsed::Lyric(t)
        } else if let Some((_, prog)) = qtimer {
            Collapsed::Overlay { glyph: "\u{E916}", text: &qtimer_text, level: Some(prog) }
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

        let todo_end = (self.todo_scroll + ui::TODO_VISIBLE).min(self.todo.items.len());
        let todo_slice = if self.todo_scroll < todo_end {
            &self.todo.items[self.todo_scroll..todo_end]
        } else {
            &[]
        };

        let frame = Frame {
            cx: self.surface_cx(),
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
            accent: (self.aura[0].value, self.aura[1].value, self.aura[2].value),
            active_page: self.active_page(),
            float_gap: self.float_gap(),
            eq_t: if self.anim_timer && self.eq_live() { Some(self.anim_start.elapsed().as_secs_f32()) } else { None },
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
                    todos: todo_slice,
                    todo_total: self.todo.items.len(),
                    todo_scroll: self.todo_scroll,
                    todo_input: &self.todo_input,
                    todo_focused: self.todo_focus,
                    todo_caret: self.todo_caret,
                })
            } else {
                None
            },
            mixer: if self.mixer_on { Some(MixerView { rows: &mixer_rows, drag: self.mixer_drag }) } else { None },
            ask: if self.ask_on {
                self.ask.as_ref().map(|ask| AskView {
                    mode_label: ask.mode.label(),
                    is_translate: ask.mode == ask_esme::AskMode::Translate,
                    input: &ask.input,
                    placeholder: ask.mode.placeholder(),
                    focused: ask.focus,
                    caret_on: ask.caret_visible,
                    selected_all: ask.selected_all,
                    inline_preview: inline_prev.as_deref(),
                    loading: ask.loading,
                    response: &ask.response,
                    copied: ask.copied_feedback,
                    has_clip: !self.clip.items.is_empty(),
                    pet: self.cfg.pet().map(|p| {
                        let mut e = self.mascot.eyes();
                        e.scale = 1.0;
                        if ask.loading {
                            // Pensando: mira arriba y de lado a lado, despacito.
                            let t = self.anim_start.elapsed().as_secs_f32();
                            e.gaze = ((t * 2.2).sin() * 1.6, -1.3);
                        } else if ask.focus && !ask.input.is_empty() {
                            // Escribiendo: mira hacia el input.
                            e.gaze = (0.0, 1.6);
                        } else {
                            e.gaze = (0.0, 0.0);
                        }
                        if e.mood == Mood::Asleep {
                            e.open = 1.0;
                            e.mood = Mood::Awake;
                        }
                        (e, p)
                    }),
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
    fn game_list(&self) -> Vec<String> {
        fullscreen::JUEGOS_BASE
            .iter()
            .map(|s| s.to_string())
            .chain(self.cfg.juegos_extra.iter().map(|s| s.trim().to_lowercase()))
            .collect()
    }

    /// ¿Hay un juego o algo en pantalla completa en algún monitor? Se usa para que
    /// el atajo de Ask Esme no le robe el foco al juego (lo minimizaría).
    pub(crate) fn game_active(&self) -> bool {
        self.hidden || !fullscreen::fullscreen_monitors(self.hwnd, &self.game_list()).is_empty()
    }

    pub(crate) fn check_fullscreen(&mut self) {
        let games = self.game_list();
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

    /// Programa el recorte de memoria una sola vez tras 15 s de reposo absoluto.
    pub(crate) fn schedule_trim(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), T_TRIM, TRIM_DELAY_MS, None);
        }
    }

    /// Cancela el recorte pendiente si el usuario vuelve a interactuar antes de los 15 s.
    pub(crate) fn cancel_trim(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), T_TRIM);
        }
    }

    /// Ejecutado una sola vez a los 15 s de reposo absoluto:
    /// 1) Garantiza la superficie DXGI en tamaño compacto de píldora.
    /// 2) Libera cachés secundarios de Direct2D/WIC y ejecuta `IDXGIDevice3::Trim()`.
    /// 3) Devuelve páginas inactivas al sistema con `SetProcessWorkingSetSizeEx`.
    pub(crate) fn on_idle_trim(&mut self) {
        if self.want_expanded() {
            return;
        }
        if self.is_animating() {
            // Todavía se mueve algo (saludo, carátula nueva): lo intentamos de nuevo en un rato.
            self.schedule_trim();
            return;
        }
        self.ensure_surface(false);
        if let Some(rd) = self.renderer.as_mut() {
            rd.release_idle_caches();
        }
        unsafe {
            let _ = SetProcessWorkingSetSizeEx(
                GetCurrentProcess(),
                usize::MAX,
                usize::MAX,
                SETPROCESSWORKINGSETSIZEEX_FLAGS(0),
            );
        }
    }
}
