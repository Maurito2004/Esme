//! Medidas y distribución de la interfaz (en DIPs). La usan tanto el render como el
//! hit-testing de los clics, así lo que se ve y lo que se toca coinciden siempre.

// Lienzo fijo donde cabe todo (incluido el rebote del resorte).
pub const CANVAS_W: f32 = 520.0;
pub const CANVAS_H: f32 = 340.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dims {
    pub w: f32,
    pub h: f32,
}

pub const PILL: Dims = Dims { w: 200.0, h: 32.0 };
pub const PILL_MEDIA: Dims = Dims { w: 236.0, h: 32.0 };
pub const PILL_LYRIC: Dims = Dims { w: 460.0, h: 34.0 };
pub const MINIMAL: Dims = Dims { w: 150.0, h: 5.0 };
/// Píldora de los overlays del sistema (volumen, Bloq Mayús, idioma, cargador).
pub const PILL_OVERLAY: Dims = Dims { w: 300.0, h: 34.0 };

pub const EXP_CLOCK: Dims = Dims { w: 300.0, h: 92.0 };
pub const EXP_MEDIA: Dims = Dims { w: 400.0, h: 126.0 };
pub const LYRICS_EXTRA: f32 = 162.0;
/// Alto extra cuando solo hay un aviso ("Buscando letra…", "No encontré…").
pub const LYRICS_MSG_EXTRA: f32 = 40.0;
pub const LYRIC_LINE_H: f32 = 30.0;

pub const SHELF_TILE_W: f32 = 72.0;
pub const SHELF_TILE_H: f32 = 66.0;
pub const SHELF_COLS: usize = 5;

/// Alto extra del estante según cuántos archivos hay.
pub fn shelf_extra(n: usize) -> f32 {
    if n == 0 { 64.0 } else { n.div_ceil(SHELF_COLS) as f32 * SHELF_TILE_H + 14.0 }
}

/// Filas del panel "Hoy" (máscara de bits; TODAY_OPEN = panel abierto).
pub const TODAY_POMO: u8 = 1;
pub const TODAY_WATER: u8 = 2;
pub const TODAY_WEATHER: u8 = 4;
pub const TODAY_CAL: u8 = 8;
pub const TODAY_OPEN: u8 = 0x10;

/// Alto extra del panel "Hoy" según qué filas están visibles (todas = 168).
pub fn today_extra(mask: u8) -> f32 {
    let mut h = 2.0;
    if mask & TODAY_POMO != 0 { h += 40.0; }
    if mask & TODAY_WATER != 0 { h += 40.0; }
    if mask & TODAY_WEATHER != 0 { h += 32.0; }
    if mask & TODAY_CAL != 0 { h += 54.0; }
    if mask & 0x0F == 0 { h += 32.0; }
    h
}

/// Rectángulo fuera de pantalla para filas ocultas (nunca se dibuja ni recibe clics).
const OFF: R = R { l: -10000.0, t: -10000.0, r: -10000.0, b: -10000.0 };

/// Alto extra del panel Ask Esme (barra de modo + acciones + input; crece si hay respuesta).
pub const ASK_BASE_EXTRA: f32 = 76.0;
pub const ASK_RESP_EXTRA: f32 = 88.0;

pub const CLIP_ROW_H: f32 = 26.0;
pub const CLIP_ROWS: usize = 5;

/// Alto extra del panel del portapapeles según cuántos textos hay.
pub fn clip_extra(n: usize) -> f32 {
    if n == 0 { 40.0 } else { n.min(CLIP_ROWS) as f32 * CLIP_ROW_H + 14.0 }
}

/// Separación entre la píldora y el contenido flotante en modo transparente.
pub const FLOAT_GAP: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Prev,
    Play,
    Next,
    Lyrics,
    Progress,
    LyricLine(usize),
    Clip,
    ClipItem(usize),
    Shelf,
    ShelfItem(usize),
    Today,
    PomoToggle,
    PomoReset,
    WaterAdd,
    WaterSub,
    Ask,
    AskMode,
    AskInput,
    AskSummarize,
    AskSpellcheck,
    AskCopy,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct R {
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
}

impl R {
    pub fn new(l: f32, t: f32, r: f32, b: f32) -> Self {
        Self { l, t, r, b }
    }
    pub fn w(&self) -> f32 {
        self.r - self.l
    }

    pub fn h(&self) -> f32 {
        self.b - self.t
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.l && x <= self.r && y >= self.t && y <= self.b
    }
    pub fn cy(&self) -> f32 {
        (self.t + self.b) / 2.0
    }
    pub fn inflate(&self, d: f32) -> Self {
        Self::new(self.l - d, self.t - d, self.r + d, self.b + d)
    }
}

/// Distribución del contenido expandido.
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout {
    pub area: R,
    pub art: R,
    pub title: R,
    pub artist: R,
    pub prev: R,
    pub play: R,
    pub next: R,
    pub lyrics_btn: R,
    pub track: R,
    pub t_left: R,
    pub t_right: R,
    pub lyrics: R,
    pub clock: R,
    pub date: R,
    pub clip_btn: R,
    pub clip: R,
    pub shelf_btn: R,
    pub shelf: R,
    pub today_btn: R,
    pub today: R,
    pub pomo_row: R,
    pub water_row: R,
    pub weather_row: R,
    pub cal_row: R,
    pub pomo_toggle: R,
    pub pomo_reset: R,
    pub water_add: R,
    pub water_sub: R,
    pub ask_btn: R,
    pub ask: R,
    pub ask_mode: R,
    pub ask_summarize: R,
    pub ask_spellcheck: R,
    pub ask_input: R,
    pub ask_response: R,
    pub ask_copy: R,
}

impl Layout {
    pub fn new(
        cx: f32,
        top: f32,
        d: Dims,
        media: bool,
        lyrics: bool,
        clip: bool,
        shelf: bool,
        today: u8,
        ask: bool,
    ) -> Self {
        // El área de letras ocupa lo que sobre debajo de los controles.
        let x0 = cx - d.w / 2.0;
        let x1 = cx + d.w / 2.0;
        let y0 = top;
        let mut l = Layout { area: R::new(x0, y0, x1, y0 + d.h), ..Default::default() };
        if media {
            l.art = R::new(x0 + 16.0, y0 + 14.0, x0 + 80.0, y0 + 78.0);
            let tx = x0 + 94.0;
            l.title = R::new(tx, y0 + 15.0, x1 - 18.0, y0 + 36.0);
            l.artist = R::new(tx, y0 + 36.0, x1 - 18.0, y0 + 54.0);
            let c = tx + 52.0;
            let by = y0 + 70.0;
            let btn = |x: f32| R::new(x - 16.0, by - 14.0, x + 16.0, by + 14.0);
            l.prev = btn(c - 40.0);
            l.play = btn(c);
            l.next = btn(c + 40.0);
            l.lyrics_btn = btn(x1 - 30.0);
            l.clip_btn = btn(x1 - 64.0);
            l.shelf_btn = btn(x1 - 98.0);
            l.today_btn = btn(x1 - 132.0);
            l.ask_btn = btn(x1 - 166.0);
            let sub = R::new(x0 + 16.0, y0 + EXP_MEDIA.h - 6.0, x1 - 16.0, y0 + d.h - 8.0);
            if today != 0 {
                l.set_today(sub, today);
            }
            if clip {
                l.clip = sub;
            }
            if shelf {
                l.shelf = sub;
            }
            if ask {
                l.set_ask(sub);
            }
            let ty = y0 + 100.0;
            l.t_left = R::new(x0 + 12.0, ty - 9.0, x0 + 56.0, ty + 9.0);
            l.t_right = R::new(x1 - 56.0, ty - 9.0, x1 - 12.0, ty + 9.0);
            l.track = R::new(x0 + 60.0, ty - 2.0, x1 - 60.0, ty + 2.0);
            if lyrics {
                l.lyrics = R::new(x0 + 18.0, y0 + EXP_MEDIA.h - 6.0, x1 - 18.0, y0 + d.h - 12.0);
            }
        } else {
            l.clock = R::new(x0, y0 + 10.0, x1, y0 + 58.0);
            l.date = R::new(x0, y0 + 58.0, x1, y0 + 78.0);
            l.clip_btn = R::new(x1 - 46.0, y0 + 30.0, x1 - 12.0, y0 + 58.0);
            l.ask_btn = R::new(x1 - 84.0, y0 + 30.0, x1 - 50.0, y0 + 58.0);
            l.shelf_btn = R::new(x0 + 12.0, y0 + 30.0, x0 + 46.0, y0 + 58.0);
            l.today_btn = R::new(x0 + 50.0, y0 + 30.0, x0 + 84.0, y0 + 58.0);
            let sub = R::new(x0 + 16.0, y0 + EXP_CLOCK.h - 6.0, x1 - 16.0, y0 + d.h - 8.0);
            if today != 0 {
                l.set_today(sub, today);
            }
            if clip {
                l.clip = sub;
            }
            if shelf {
                l.shelf = sub;
            }
            if ask {
                l.set_ask(sub);
            }
        }
        l
    }

    /// Filas y botones del panel "Hoy".
    fn set_today(&mut self, a: R, mask: u8) {
        self.today = a;
        let mut y = a.t;
        let mut row = |on: bool, top: f32, h: f32, adv: f32| {
            if !on {
                return OFF;
            }
            let r = R::new(a.l, y + top, a.r, y + top + h);
            y += adv;
            r
        };
        self.pomo_row = row(mask & TODAY_POMO != 0, 4.0, 36.0, 40.0);
        self.water_row = row(mask & TODAY_WATER != 0, 4.0, 36.0, 40.0);
        self.weather_row = row(mask & TODAY_WEATHER != 0, 4.0, 28.0, 32.0);
        self.cal_row = row(mask & TODAY_CAL != 0, 2.0, 52.0, 54.0);
        let b = |row: R, k: f32| {
            if row.l <= OFF.l { OFF } else { R::new(row.r - 36.0 * k - 30.0, row.t + 4.0, row.r - 36.0 * k, row.b - 4.0) }
        };
        self.pomo_toggle = b(self.pomo_row, 1.0);
        self.pomo_reset = b(self.pomo_row, 0.0);
        self.water_sub = b(self.water_row, 1.0);
        self.water_add = b(self.water_row, 0.0);
    }

    /// Filas y controles del panel "Ask Esme" (Fase 6).
    fn set_ask(&mut self, a: R) {
        self.ask = a;
        let top_y = a.t + 4.0;
        self.ask_mode = R::new(a.l, top_y, a.l + 104.0, top_y + 22.0);
        self.ask_summarize = R::new(a.r - 196.0, top_y, a.r - 102.0, top_y + 22.0);
        self.ask_spellcheck = R::new(a.r - 96.0, top_y, a.r, top_y + 22.0);
        self.ask_input = R::new(a.l, top_y + 28.0, a.r, top_y + 60.0);
        if a.b - (top_y + 66.0) > 20.0 {
            self.ask_response = R::new(a.l, top_y + 66.0, a.r, a.b - 2.0);
            self.ask_copy = R::new(a.r - 78.0, a.b - 26.0, a.r - 4.0, a.b - 4.0);
        }
    }

    /// Rectángulo del archivo `i` del estante (centrado, hasta SHELF_COLS por fila).
    pub fn shelf_tile(&self, i: usize, n: usize) -> R {
        let cols = n.clamp(1, SHELF_COLS);
        let row_w = cols as f32 * SHELF_TILE_W;
        let x = (self.shelf.l + self.shelf.r) / 2.0 - row_w / 2.0 + (i % SHELF_COLS) as f32 * SHELF_TILE_W;
        let y = self.shelf.t + 6.0 + (i / SHELF_COLS) as f32 * SHELF_TILE_H;
        R::new(x + 2.0, y, x + SHELF_TILE_W - 2.0, y + SHELF_TILE_H - 4.0)
    }

    pub fn hit(&self, x: f32, y: f32, lyric_scroll: f32, n_lines: usize, clip_n: usize, shelf_n: usize) -> Option<Btn> {
        let pad = 4.0;
        if self.shelf_btn.inflate(pad).contains(x, y) {
            return Some(Btn::Shelf);
        }
        if self.today_btn.inflate(pad).contains(x, y) {
            return Some(Btn::Today);
        }
        if self.ask_btn.inflate(pad).contains(x, y) {
            return Some(Btn::Ask);
        }
        if self.ask.r > self.ask.l {
            if self.ask_copy.r > self.ask_copy.l && self.ask_copy.inflate(2.0).contains(x, y) {
                return Some(Btn::AskCopy);
            }
            for (r, b) in [
                (self.ask_mode, Btn::AskMode),
                (self.ask_summarize, Btn::AskSummarize),
                (self.ask_spellcheck, Btn::AskSpellcheck),
                (self.ask_input, Btn::AskInput),
            ] {
                if r.inflate(2.0).contains(x, y) {
                    return Some(b);
                }
            }
        }
        if self.today.r > self.today.l {
            for (r, b) in [
                (self.pomo_toggle, Btn::PomoToggle),
                (self.pomo_reset, Btn::PomoReset),
                (self.water_add, Btn::WaterAdd),
                (self.water_sub, Btn::WaterSub),
            ] {
                if r.inflate(2.0).contains(x, y) {
                    return Some(b);
                }
            }
        }
        for i in 0..shelf_n {
            if self.shelf_tile(i, shelf_n).contains(x, y) {
                return Some(Btn::ShelfItem(i));
            }
        }
        if self.clip_btn.inflate(pad).contains(x, y) {
            return Some(Btn::Clip);
        }
        if clip_n > 0 && self.clip.contains(x, y) {
            let i = ((y - self.clip.t - 6.0) / CLIP_ROW_H).floor();
            if i >= 0.0 && (i as usize) < clip_n.min(CLIP_ROWS) {
                return Some(Btn::ClipItem(i as usize));
            }
        }
        if self.prev.inflate(pad).contains(x, y) {
            return Some(Btn::Prev);
        }
        if self.play.inflate(pad).contains(x, y) {
            return Some(Btn::Play);
        }
        if self.next.inflate(pad).contains(x, y) {
            return Some(Btn::Next);
        }
        if self.lyrics_btn.inflate(pad).contains(x, y) {
            return Some(Btn::Lyrics);
        }
        if R::new(self.track.l, self.track.t - 8.0, self.track.r, self.track.b + 8.0).contains(x, y) {
            return Some(Btn::Progress);
        }
        if n_lines > 0 && self.lyrics.contains(x, y) {
            let off = (y - self.lyrics.cy()) / LYRIC_LINE_H;
            let idx = (lyric_scroll + off).round();
            if idx >= 0.0 && (idx as usize) < n_lines {
                return Some(Btn::LyricLine(idx as usize));
            }
        }
        None
    }
}

pub fn fmt_time(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}
