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
        }
        self.retarget();
        self.refresh();
    }

    pub(crate) fn on_shelf_drop(&mut self) {
        let paths = shelf::take_dropped();
        self.drop_hover = false;
        if !paths.is_empty() {
            self.shelf.add(paths);
        }
        // Lo dejamos abierto un momento para ver lo que se guardó.
        self.drop_open = true;
        unsafe {
            SetTimer(Some(self.hwnd), T_DROPCLOSE, 1600, None);
        }
        self.retarget();
        self.refresh();
    }

    /// Botón presionado sobre un archivo del estante: preparamos un posible arrastre.
    pub(crate) fn on_mouse_down(&mut self, lp: LPARAM) {
        self.press = None;
        if self.expand.value < 0.6 {
            return;
        }
        let (x, y) = self.to_dips(lp);
        if let Some(Btn::ShelfItem(i)) = self.layout_now().hit(x, y, 0.0, 0, 0, self.shelf_on_count()) {
            self.press = Some((i, x, y));
        }
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
        self.retarget();
        self.refresh();
    }
}
