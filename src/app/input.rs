//! Entrada del mouse: hover, clics, rueda.

use crate::*;

impl App {
    // ---------- Entrada del mouse ----------

    pub(crate) fn to_dips(&self, lp: LPARAM) -> (f32, f32) {
        let x = (lp.0 & 0xFFFF) as i16 as f32;
        let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32;
        (x / self.scale, y / self.scale)
    }

    pub(crate) fn on_mouse_move(&mut self, lp: LPARAM) {
        unsafe {
            // Re-armamos el seguimiento en cada movimiento: la región cambia durante la animación.
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut tme);
        }
        if !self.hover {
            self.hover = true;
            self.cancel_trim();
            self.update_gaze();
            self.pop_eyes();
            if self.mascot.mood == Mood::Asleep {
                self.wake();
            }
            unsafe {
                SetTimer(Some(self.hwnd), T_HOVERCHECK, 200, None);
                let _ = KillTimer(Some(self.hwnd), T_COLLAPSE);
                if self.expand.target < 1.0 {
                    SetTimer(Some(self.hwnd), T_EXPAND, EXPAND_DELAY_MS, None);
                }
            }
        }
        let (x, y) = self.to_dips(lp);
        let btn = if self.expand.target > 0.5 && self.expand.value > 0.6 {
            let n = match &self.lyrics {
                Some(Lyrics::Synced(v)) => v.len(),
                _ => 0,
            };
            self.layout_now().hit(x, y, self.lyric_scroll.value, n, self.clip_on_count(), self.shelf_on_count())
        } else {
            None
        };
        if btn != self.hover_btn {
            self.hover_btn = btn;
            self.refresh();
        }
    }

    pub(crate) fn on_mouse_leave(&mut self) {
        if !self.hover {
            return;
        }
        self.hover = false;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), T_HOVERCHECK);
            let _ = KillTimer(Some(self.hwnd), T_EXPAND);
            SetTimer(Some(self.hwnd), T_COLLAPSE, COLLAPSE_DELAY_MS, None);
        }
    }

    pub(crate) fn shelf_on_count(&self) -> usize {
        if self.shelf_on { self.shelf.items.len() } else { 0 }
    }
}
