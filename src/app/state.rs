//! Estado derivado: qué se muestra y con qué tamaño.

use crate::*;

impl App {
    // ---------- Estado derivado ----------

    pub(crate) fn has_media(&self) -> bool {
        self.media.as_ref().map(|m| !m.title.is_empty()).unwrap_or(false)
    }

    pub(crate) fn playing(&self) -> bool {
        self.media.as_ref().map(|m| m.playing && !m.title.is_empty()).unwrap_or(false)
    }

    pub(crate) fn synced_lines(&self) -> Option<&[(f64, String)]> {
        match &self.lyrics {
            Some(Lyrics::Synced(v)) => Some(v),
            _ => None,
        }
    }

    /// Separación (DIPs) entre el borde de arriba y la píldora flotante.
    pub(crate) fn float_gap(&self) -> f32 {
        if self.cfg.flotante { 7.0 } else { 0.0 }
    }

    /// Ecualizador vivo: música sonando y la píldora contraída mostrando la carátula.
    pub(crate) fn eq_live(&self) -> bool {
        !self.hidden
            && self.playing()
            && !self.want_expanded()
            && self.overlay.is_none()
            && self.toast.is_none()
            && self.quick_timer.is_none()
            && !self.pomo_pill()
            && !self.lyric_pill()
            && !self.cfg.notch_minimo
    }

    /// Esme "pensando" mientras Gemini responde.
    pub(crate) fn ask_thinking(&self) -> bool {
        self.ask_on && self.ask.as_ref().map(|a| a.loading).unwrap_or(false)
    }

    /// Pomodoro corriendo con la píldora cerrada (sin otro aviso tapándolo): brillo suave que respira (~4 s).
    pub(crate) fn pomo_breathing(&self) -> bool {
        !self.hidden
            && self.pomo.running()
            && (!self.want_expanded() || self.cfg.transparente)
            && self.overlay.is_none()
            && self.toast.is_none()
            && self.quick_timer.is_none()
    }

    pub(crate) fn want_expanded(&self) -> bool {
        self.hover || self.pinned || self.drop_open || self.ask_on || self.todo_focus
    }

    pub(crate) fn lyric_pill(&self) -> bool {
        self.lyrics_on && self.has_media() && self.synced_lines().is_some()
    }

    pub(crate) fn mascot_shown(&self) -> bool {
        self.toast.is_none()
            && self.overlay.is_none()
            && !self.pomo_pill()
            && self.quick_timer.is_none()
            && self.cfg.pet().is_some()
            && !self.cfg.notch_minimo
            && !self.lyric_pill()
            && !self.playing()
    }

    pub(crate) fn collapsed_dims(&self) -> Dims {
        if self.overlay.is_some() || ((self.pomo_pill() || self.quick_timer.is_some()) && self.toast.is_none()) {
            ui::PILL_OVERLAY
        } else if self.lyric_pill() || self.toast.is_some() {
            ui::PILL_LYRIC
        } else if self.cfg.notch_minimo {
            ui::MINIMAL
        } else if self.playing() {
            ui::PILL_MEDIA
        } else {
            ui::PILL
        }
    }

    pub(crate) fn expanded_dims(&self) -> Dims {
        self.expanded_dims_for_page(self.active_page())
    }

    pub(crate) fn expanded_dims_for_page(&self, page: usize) -> Dims {
        let has_media = self.has_media();
        let mut d = if has_media {
            let mut d = ui::EXP_MEDIA;
            if page == 6 {
                d.h += match self.lyrics {
                    Some(Lyrics::Synced(_)) | Some(Lyrics::Plain(_)) => ui::LYRICS_EXTRA,
                    _ => ui::LYRICS_MSG_EXTRA,
                };
            }
            d
        } else {
            ui::EXP_CLOCK
        };
        if page == 4 {
            d.h += ui::clip_extra(self.clip.items.len());
            d.w = d.w.max(360.0);
        }
        if page == 3 {
            d.h += ui::shelf_extra(self.shelf.items.len());
            d.w = d.w.max(400.0);
        }
        if page == 2 {
            d.h += ui::today_extra(self.today_mask_open(), self.todo.items.len());
            d.w = d.w.max(400.0);
        }
        if page == 5 {
            let n = if self.mixer_on { self.mixer.len() } else { self.prev_mixer_rows.len() };
            d.h += ui::mixer_extra(n);
            d.w = d.w.max(400.0);
        }
        d.h += ui::PAGE_DOTS_H;
        if page == 1 {
            d.h += ui::ASK_BASE_EXTRA;
            d.w = d.w.max(430.0);
            if let Some(ask) = self.ask.as_ref() {
                if ask.loading || !ask.response.is_empty() {
                    // El panel crece con la respuesta (sin pasarse del lienzo).
                    let text_h = self.renderer.as_ref().map(|r| r.wrapped_height(&ask.response, d.w - 48.0)).unwrap_or(0.0);
                    let room = (ui::CANVAS_H - d.h - 12.0).max(ui::ASK_RESP_EXTRA);
                    d.h += (text_h + 42.0).clamp(ui::ASK_RESP_EXTRA, room);
                }
            }
        }
        d
    }

    pub(crate) fn content_top(&self) -> f32 {
        if self.cfg.transparente { self.collapsed_dims().h + ui::FLOAT_GAP } else { 0.0 }
    }

    pub(crate) fn layout_now(&self) -> Layout {
        self.layout_for_page(self.active_page())
    }

    pub(crate) fn layout_for_page(&self, page: usize) -> Layout {
        let has_media = self.has_media();
        let mixer_n = if page == 5 {
            Some(if self.mixer_on { self.mixer.len() } else { self.prev_mixer_rows.len() })
        } else {
            None
        };
        Layout::new(
            self.surface_cx(),
            self.content_top(),
            self.expanded_dims_for_page(page),
            has_media,
            page == 6 && has_media,
            page == 4,
            page == 3,
            if page == 2 { self.today_mask_open() } else { 0 },
            page == 1,
            mixer_n,
        )
    }

    fn today_mask_open(&self) -> u8 {
        let c = &self.cfg;
        let mut m = ui::TODAY_OPEN;
        if c.hoy_pomodoro { m |= ui::TODAY_POMO; }
        if c.hoy_agua { m |= ui::TODAY_WATER; }
        if c.hoy_clima { m |= ui::TODAY_WEATHER; }
        if c.hoy_calendario { m |= ui::TODAY_CAL; }
        if c.hoy_todo { m |= ui::TODAY_TODO; }
        m
    }

    /// Estado semántico actual de la píldora contraída (para disparar el morph al cambiar).
    pub(crate) fn collapsed_key(&self) -> CollapsedKey {
        if let Some(o) = &self.overlay {
            if o.charger.is_some() {
                CollapsedKey::Charger
            } else if o.level.is_some() {
                if o.glyph == "\u{E706}" { CollapsedKey::OverlayBright } else { CollapsedKey::OverlayVol }
            } else {
                CollapsedKey::OverlayText(o.glyph)
            }
        } else if self.toast.is_some() {
            CollapsedKey::Toast
        } else if self.quick_timer.is_some() {
            CollapsedKey::QuickTimer
        } else if self.pomo_pill() {
            CollapsedKey::Pomo
        } else if self.lyric_pill() {
            CollapsedKey::Lyric
        } else if self.cfg.notch_minimo {
            CollapsedKey::Minimal
        } else if self.playing() {
            CollapsedKey::Media
        } else if self.cfg.pet().is_some() || self.mascot_vis.value > 0.01 {
            CollapsedKey::Mascot
        } else {
            CollapsedKey::Clock
        }
    }

    /// Instantánea liviana de las filas del mezclador (se usa solo durante el morph saliente).
    pub(crate) fn snapshot_mixer_rows(&self) -> Vec<(String, f32, bool)> {
        self.mixer
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), self.mixer_vis.get(i).map(|s| s.value).unwrap_or(c.level), c.muted))
            .collect()
    }

    /// Índice de la vista activa para los puntitos indicadores del panel expandido.
    /// Orden: 0 = Principal, 1 = Ask, 2 = Hoy, 3 = Estante, 4 = Portapapeles, 5 = Mezclador, 6 = Letras (con música).
    pub(crate) fn active_page(&self) -> usize {
        if self.ask_on {
            1
        } else if self.today_on {
            2
        } else if self.shelf_on {
            3
        } else if self.clip_on {
            4
        } else if self.mixer_on {
            5
        } else if self.lyrics_on && self.has_media() {
            6
        } else {
            0
        }
    }

    /// Cantidad de tareas visibles en la ventana actual del To-Do.
    pub(crate) fn todo_visible_count(&self) -> usize {
        if self.today_on && self.cfg.hoy_todo {
            self.todo.items.len().saturating_sub(self.todo_scroll).min(ui::TODO_VISIBLE)
        } else {
            0
        }
    }

    pub(crate) fn is_animating(&self) -> bool {
        !(self.sw.is_settled()
            && self.sh.is_settled()
            && self.expand.is_settled()
            && self.content_vis.is_settled()
            && self.pill_morph.is_settled()
            && self.page_morph.is_settled()
            && self.jelly.is_settled()
            && self.todo_pop.is_settled()
            && self.todo_count_anim.is_settled()
            && self.water_bars_open.is_settled()
            && self.water_bar_bump.is_settled()
            && self.water_num_anim.is_settled()
            && self.charge_pct.is_settled()
            && self.charge_ring.is_settled()
            && self.shelf_enter.is_settled()
            && self.btn_scale.is_settled()
            && self.mascot_vis.is_settled()
            && self.lyric_scroll.is_settled()
            && self.level.is_settled()
            && self.eye_pop.is_settled()
            && self.aura.iter().all(|s| s.is_settled())
            && self.mixer_vis.iter().all(|s| s.is_settled())
            && self.todo_strike.iter().all(|s| s.is_settled()))
            || self.closing_content
            || self.mascot.hopping()
            || self.renderer.as_ref().map(|r| r.animating()).unwrap_or(false)
    }

    // ---------- Animación ----------

    pub(crate) fn now(&self) -> i64 {
        let mut t = 0i64;
        unsafe {
            let _ = QueryPerformanceCounter(&mut t);
        }
        t
    }

    /// Recalcula a dónde tiene que ir cada animación según el estado actual.
    /// Color del acento: el de la carátula si hay música, si no el de los ojos de la mascota.
    pub(crate) fn accent_target(&self) -> (f32, f32, f32) {
        if self.has_media() {
            if let Some(c) = self.renderer.as_ref().and_then(|r| r.art_color()) {
                return c;
            }
        }
        match self.cfg.pet() {
            Some(config::Pet::Esme) => (1.0, 0.72, 0.32),
            Some(config::Pet::Gato) => (0.92, 0.94, 1.0),
            None => (0.55, 0.72, 1.0),
        }
    }

    /// "¡Oh!": los ojos se agrandan un instante (al pasar el mouse o al volver de un aviso).
    pub(crate) fn pop_eyes(&mut self) {
        if self.mascot_shown() {
            if self.is_animating() {
                self.eye_pop.value = 1.16;
            } else {
                self.eye_pop.value = 1.16;
                self.last_tick = self.now();
            }
        }
    }

    /// Rebote "gelatina" horizontal (~6 % de estiramiento) al llegar un aviso u overlay.
    pub(crate) fn trigger_jelly(&mut self) {
        if self.hidden || (self.want_expanded() && !self.cfg.transparente) {
            return;
        }
        let was = self.is_animating();
        // Desde reposo (0.0), velocidad 2.6 con k=480 y bounce=0.42 alcanza ~+0.06 (+6 %)
        // a los ~53 ms y vuelve con un rebote elástico corto (~320 ms).
        self.jelly.velocity = 2.6;
        self.jelly.set_target(0.0);
        if !was {
            self.last_tick = self.now();
        }
    }

    pub(crate) fn retarget(&mut self) {
        // Un solo panel a la vez: si se abrió otro, el mezclador se cierra (y suelta Core Audio).
        if self.mixer_on && (self.lyrics_on || self.clip_on || self.shelf_on || self.today_on || self.ask_on) {
            if self.want_expanded() && self.expand.value > 0.25 {
                self.prev_mixer_rows = self.snapshot_mixer_rows();
            }
            self.mixer_on = false;
            self.mixer.clear();
            self.mixer_vis.clear();
            self.mixer_drag = None;
        }
        let was = self.is_animating();
        let c = self.accent_target();
        for (s, v) in self.aura.iter_mut().zip([c.0, c.1, c.2]) {
            s.set_target(v);
        }
        let open = self.want_expanded();

        // 1) Cambio de página o de modo (reloj <-> música) con el panel abierto: morph + deslizamiento 24 px.
        let new_page = self.active_page();
        let new_has_media = self.has_media();
        if open && self.expand.value > 0.25 && !self.closing_content {
            if new_page != self.last_page {
                self.prev_page = Some(self.last_page);
                self.page_dir = if new_page > self.last_page { 1.0 } else { -1.0 };
                self.page_morph.value = 0.0;
                self.page_morph.velocity = 0.0;
                self.page_morph.set_target(1.0);
            } else if new_has_media != self.last_has_media {
                self.prev_page = None;
                self.page_dir = 0.0;
                self.page_morph.value = 0.0;
                self.page_morph.velocity = 0.0;
                self.page_morph.set_target(1.0);
            }
        } else if !open && !self.closing_content {
            self.prev_page = None;
            self.prev_mixer_rows.clear();
            self.page_morph.value = 1.0;
            self.page_morph.velocity = 0.0;
            self.page_morph.set_target(1.0);
        }
        self.last_page = new_page;
        self.last_has_media = new_has_media;

        // 1b) Al abrir Hoy, las barritas de agua por hora crecen escalonadas desde abajo.
        let hoy_visible = open && new_page == 2;
        if hoy_visible && !self.water_bars_shown {
            self.water_bars_shown = true;
            self.water_bars_open.value = 0.0;
            self.water_bars_open.velocity = 0.0;
            self.water_bars_open.set_target(1.0);
        } else if !hoy_visible && !self.closing_content {
            self.water_bars_shown = false;
        }

        // 2) Cambio de estado en la píldora contraída: morph de salida + entrada (~220 ms).
        let new_ck = self.collapsed_key();
        if new_ck != self.last_collapsed_key {
            if (!open || self.cfg.transparente) && (self.expand.value < 0.5 || self.cfg.transparente) {
                self.prev_collapsed = Some(self.last_collapsed_snap.clone());
                self.pill_morph.value = 0.0;
                self.pill_morph.velocity = 0.0;
                self.pill_morph.set_target(1.0);
            } else {
                self.prev_collapsed = None;
                self.pill_morph.value = 1.0;
                self.pill_morph.velocity = 0.0;
                self.pill_morph.set_target(1.0);
            }
            self.last_collapsed_key = new_ck;
        }

        // 3) Cierre en dos tiempos: primero se va el contenido (~120 ms) dejando la forma negra
        //    quieta y vacía un instante, y recién después se encoge hasta la píldora chica.
        let stage1_close = !open
            && !self.hidden
            && !self.cfg.transparente
            && (self.closing_content || (self.expand.value > 0.25 && self.content_vis.value > 0.06));
        let shape = if open && !self.cfg.transparente { self.expanded_dims() } else { self.collapsed_dims() };
        // Si vamos a expandir, estamos en tiempo 1 de cierre, o la forma (incl. rebote gelatina)
        // excede la superficie compacta actual, agrandamos el swapchain ANTES del primer frame.
        let need_full = open
            || stage1_close
            || shape.w * 1.08 + 32.0 > self.surf_w
            || shape.h + 8.0 > self.surf_h
            || !self.jelly.is_settled();
        if need_full {
            self.cancel_trim();
            self.ensure_surface(true);
        }
        if !open {
            self.trim_pending = true;
        }
        if stage1_close {
            self.closing_content = true;
            self.content_vis.set_target(0.0);
        } else {
            self.closing_content = false;
            self.content_vis.set_target(if open { 1.0 } else { 0.0 });
            self.sw.set_target(shape.w);
            self.sh.set_target(shape.h);
            self.expand.set_target(if open { 1.0 } else { 0.0 });
        }
        self.mascot_vis.set_target(if self.mascot_shown() { 1.0 } else { 0.0 });
        if !was {
            self.last_tick = self.now();
        }
        self.update_timers();
    }

    pub(crate) fn tick(&mut self) {
        let now = self.now();
        let dt = (((now - self.last_tick) as f64 / self.qpf as f64) as f32).clamp(0.0, 1.0 / 20.0);
        self.last_tick = now;
        for s in [
            &mut self.sw,
            &mut self.sh,
            &mut self.expand,
            &mut self.content_vis,
            &mut self.pill_morph,
            &mut self.page_morph,
            &mut self.jelly,
            &mut self.todo_pop,
            &mut self.todo_count_anim,
            &mut self.water_bars_open,
            &mut self.water_bar_bump,
            &mut self.water_num_anim,
            &mut self.charge_pct,
            &mut self.charge_ring,
            &mut self.shelf_enter,
            &mut self.btn_scale,
            &mut self.mascot_vis,
            &mut self.lyric_scroll,
            &mut self.level,
            &mut self.eye_pop,
        ] {
            s.step(dt);
        }
        for s in self.aura.iter_mut() {
            s.step(dt);
        }
        for s in self.mixer_vis.iter_mut() {
            s.step(dt);
        }
        for s in self.todo_strike.iter_mut() {
            s.step(dt);
        }
        // Tiempo 1 -> Tiempo 2 del cierre: apenas el contenido termina de irse (~120 ms), encogemos la forma.
        if self.closing_content && !self.want_expanded() && self.content_vis.value <= 0.06 {
            self.closing_content = false;
            self.content_vis.value = 0.0;
            self.content_vis.velocity = 0.0;
            self.content_vis.set_target(0.0);
            self.prev_page = None;
            self.prev_mixer_rows.clear();
            let shape = self.collapsed_dims();
            self.sw.set_target(shape.w);
            self.sh.set_target(shape.h);
            self.expand.set_target(0.0);
            self.trim_pending = true;
        }
        if self.pill_morph.is_settled() && self.prev_collapsed.is_some() {
            self.prev_collapsed = None;
        }
        if self.page_morph.is_settled() && (self.prev_page.is_some() || !self.prev_mixer_rows.is_empty()) {
            self.prev_page = None;
            self.prev_mixer_rows.clear();
        }
        if self.todo_pop.is_settled() && self.todo_pop_idx.is_some() {
            self.todo_pop_idx = None;
        }
        if self.water_bar_bump.is_settled() && self.water_bump_hour.is_some() {
            self.water_bump_hour = None;
        }
        if self.shelf_enter.is_settled() && self.shelf_new_count > 0 {
            self.shelf_new_count = 0;
        }
        if !self.btn_down && self.btn_scale.is_settled() && self.pressed_btn.is_some() {
            self.pressed_btn = None;
        }
        self.redraw_with(true);
        if !self.is_animating() && self.trim_pending {
            self.trim_pending = false;
            if !self.want_expanded() {
                // Al terminar de contraerse, compactamos la superficie gráfica y programamos
                // el Working Set Trim a 15 s de reposo absoluto.
                self.ensure_surface(false);
                self.schedule_trim();
            }
        }
    }
}
