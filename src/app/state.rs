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

    pub(crate) fn want_expanded(&self) -> bool {
        self.hover || self.pinned || self.drop_open
    }

    pub(crate) fn lyric_pill(&self) -> bool {
        self.lyrics_on && self.has_media() && self.synced_lines().is_some()
    }

    pub(crate) fn mascot_shown(&self) -> bool {
        self.toast.is_none() && self.overlay.is_none() && self.cfg.pet().is_some() && !self.cfg.notch_minimo && !self.lyric_pill() && !self.playing()
    }

    pub(crate) fn collapsed_dims(&self) -> Dims {
        if self.overlay.is_some() {
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
        let mut d = self.base_expanded_dims();
        if self.clip_on {
            d.h += ui::clip_extra(self.clip.items.len());
            d.w = d.w.max(360.0);
        }
        if self.shelf_on {
            d.h += ui::shelf_extra(self.shelf.items.len());
            d.w = d.w.max(400.0);
        }
        d
    }

    pub(crate) fn base_expanded_dims(&self) -> Dims {
        if self.has_media() {
            let mut d = ui::EXP_MEDIA;
            if self.lyrics_on {
                d.h += match self.lyrics {
                    Some(Lyrics::Synced(_)) | Some(Lyrics::Plain(_)) => ui::LYRICS_EXTRA,
                    _ => ui::LYRICS_MSG_EXTRA,
                };
            }
            d
        } else {
            ui::EXP_CLOCK
        }
    }

    pub(crate) fn content_top(&self) -> f32 {
        if self.cfg.transparente { self.collapsed_dims().h + ui::FLOAT_GAP } else { 0.0 }
    }

    pub(crate) fn layout_now(&self) -> Layout {
        Layout::new(CANVAS_W / 2.0, self.content_top(), self.expanded_dims(), self.has_media(), self.lyrics_on, self.clip_on, self.shelf_on)
    }

    pub(crate) fn is_animating(&self) -> bool {
        !(self.sw.is_settled()
            && self.sh.is_settled()
            && self.expand.is_settled()
            && self.mascot_vis.is_settled()
            && self.lyric_scroll.is_settled())
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
    pub(crate) fn retarget(&mut self) {
        let was = self.is_animating();
        let open = self.want_expanded();
        let shape = if open && !self.cfg.transparente { self.expanded_dims() } else { self.collapsed_dims() };
        if (self.expand.target > 0.5) != open {
            self.trim_pending = true; // recortamos memoria solo tras animaciones de hover
        }
        self.sw.set_target(shape.w);
        self.sh.set_target(shape.h);
        self.expand.set_target(if open { 1.0 } else { 0.0 });
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
        for s in [&mut self.sw, &mut self.sh, &mut self.expand, &mut self.mascot_vis, &mut self.lyric_scroll] {
            s.step(dt);
        }
        self.redraw_with(true);
        if !self.is_animating() && self.trim_pending {
            self.trim_pending = false;
            self.schedule_trim();
        }
    }
}
