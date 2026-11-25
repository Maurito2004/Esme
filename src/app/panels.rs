//! Estante de archivos, portapapeles, OCR y avisos.

use crate::*;

impl App {
    // ---------- Estante: arrastrar y soltar ----------

    pub(crate) fn on_shelf_drag(&mut self, entering: bool) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), T_DROPCLOSE);
        }
        self.drop_hover = entering;
        self.drop_open = entering;
        if entering {
            self.shelf_on = true;
            self.lyrics_on = false;
            self.clip_on = false;
            self.today_on = false;
            self.ask_on = false;
            self.focus_ask_input(false);
            self.focus_todo_input(false);
        }
        self.retarget();
        self.refresh();
    }

    pub(crate) fn on_shelf_drop(&mut self) {
        let paths = shelf::take_dropped();
        self.drop_hover = false;
        if !paths.is_empty() {
            let count = paths.len().min(shelf::MAX_ITEMS);
            self.shelf.add(paths);
            let was = self.is_animating();
            self.shelf_new_count = count;
            self.shelf_enter.value = 0.0;
            self.shelf_enter.velocity = 0.0;
            self.shelf_enter.set_target(1.0);
            if !was {
                self.last_tick = self.now();
            }
        }
        // Lo dejamos abierto un momento para ver lo que se guardó.
        self.drop_open = true;
        unsafe {
            SetTimer(Some(self.hwnd), T_DROPCLOSE, 1600, None);
        }
        self.retarget();
        self.refresh();
    }

    /// Botón presionado: comprime el botón a ~92 % con resorte y prepara arrastre si es ítem del estante.
    pub(crate) fn on_mouse_down(&mut self, lp: LPARAM) {
        self.press = None;
        if self.closing_content || self.expand.value < 0.6 {
            return;
        }
        let (x, y) = self.to_dips(lp);
        let n = match &self.lyrics {
            Some(Lyrics::Synced(v)) => v.len(),
            _ => 0,
        };
        let hit = self.layout_now().hit(
            x,
            y,
            self.lyric_scroll.value,
            n,
            self.clip_on_count(),
            self.shelf_on_count(),
            self.todo_visible_count(),
            self.active_page(),
        );
        if let Some(btn) = hit {
            if let Btn::ShelfItem(i) = btn {
                self.press = Some((i, x, y));
            }
            if !matches!(btn, Btn::Progress | Btn::MixerBar(_) | Btn::AskInput | Btn::TodoInput) {
                let was = self.is_animating();
                self.pressed_btn = Some(btn);
                self.btn_down = true;
                self.btn_scale.set_target(0.92);
                if !was {
                    self.last_tick = self.now();
                }
                self.refresh();
            }
        }
    }

    /// Botón soltado: vuelve de ~92 % a 100 % con resorte.
    pub(crate) fn on_mouse_up(&mut self) {
        if !self.btn_down && self.btn_scale.target >= 0.99 {
            return;
        }
        let was = self.is_animating();
        self.btn_down = false;
        // En clics muy rápidos aseguramos el golpe táctil mínimo a 92 % antes del rebote.
        if self.btn_scale.value > 0.94 {
            self.btn_scale.value = 0.92;
            self.btn_scale.velocity = 0.0;
        }
        self.btn_scale.set_target(1.0);
        if !was {
            self.last_tick = self.now();
        }
        self.refresh();
    }

    /// Si el mouse se movió con el botón apretado sobre un archivo, devuelve la ruta a arrastrar.
    pub(crate) fn drag_candidate(&mut self, wp: WPARAM, lp: LPARAM) -> Option<String> {
        let (i, px, py) = self.press?;
        if wp.0 & 0x0001 == 0 {
            self.press = None;
            return None;
        }
        let (x, y) = self.to_dips(lp);
        if (x - px).abs() + (y - py).abs() < 6.0 {
            return None;
        }
        self.press = None;
        self.shelf.items.get(i).cloned()
    }

    pub(crate) fn show_toast(&mut self, text: String, ms: u32) {
        self.toast = Some(text);
        unsafe {
            SetTimer(Some(self.hwnd), T_TOAST, ms, None);
        }
        self.trigger_jelly();
        self.retarget();
        self.refresh();
    }
}
