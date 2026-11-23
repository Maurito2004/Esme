//! Multimedia (SMTC) y letras.

use crate::*;

impl App {
    // ---------- Multimedia y letras ----------

    pub(crate) fn on_media(&mut self) {
        DBG_MEDIA.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let info = self.media_shared.lock().unwrap().clone();
        // Si solo cambió la posición y el notch está contraído, no hay nada que redibujar.
        let looks = |m: &Option<MediaInfo>| {
            m.as_ref().map(|m| (m.title.clone(), m.artist.clone(), m.playing, m.thumb_id, m.can_prev, m.can_next, m.duration > 0.5))
        };
        if looks(&info) == looks(&self.media) && !self.want_expanded() {
            self.media = info;
            if self.lyrics_on {
                let before = self.lyric_current;
                self.schedule_lyric();
                if before != self.lyric_current {
                    self.refresh();
                }
            }
            return;
        }
        let key = info.as_ref().map(|m| format!("{}\u{1}{}", m.title, m.artist)).unwrap_or_default();
        let track_changed = key != self.lyrics_key;
        if let Some(r) = self.renderer.as_mut() {
            r.set_art(info.as_ref().and_then(|m| m.thumb.clone()));
        }
        self.media = info;
        if track_changed {
            self.lyrics_key = key;
            self.lyrics = None;
            self.lyric_current = None;
            self.lyric_scroll = Spring::new(0.0, 220.0, 0.0).with_eps(0.002);
            if self.lyrics_on && self.has_media() {
                self.fetch_lyrics();
            }
        }
        if !self.has_media() {
            self.lyrics_on = false;
        }
        self.retarget();
        self.refresh();
    }

    pub(crate) fn fetch_lyrics(&mut self) {
        let Some(m) = self.media.clone() else { return };
        self.lyrics_req += 1;
        let id = self.lyrics_req;
        self.lyrics = Some(Lyrics::Loading);
        let shared = self.lyrics_shared.clone();
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let result = lyrics::fetch(&m.title, &m.artist, &m.album, m.duration);
            *shared.lock().unwrap() = Some((id, result));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_LYRICS, WPARAM(0), LPARAM(0));
            }
        });
    }

    pub(crate) fn on_lyrics(&mut self) {
        let got = self.lyrics_shared.lock().unwrap().take();
        if let Some((id, l)) = got {
            if id == self.lyrics_req {
                self.lyrics = Some(l);
                self.lyric_current = None;
                self.retarget();
                self.refresh();
            }
        }
    }
}
