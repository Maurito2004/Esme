//! Multimedia (SMTC) y letras con manejo sin retención de carátulas en RAM y canal diferido (Fase 7).

use crate::*;

impl App {
    // ---------- Multimedia y letras ----------

    pub(crate) fn on_media(&mut self) {
        DBG_MEDIA.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Extraemos la carátula cruda una sola vez con `.take()` para nunca retener buffers PNG/JPG en RAM.
        let (mut info, fresh_thumb) = {
            let mut guard = self.media_shared.lock().unwrap();
            // Sin renderer todavía, la dejamos en el estado compartido para tomarla después.
            let thumb = if self.renderer.is_some() { guard.as_mut().and_then(|m| m.thumb.take()) } else { None };
            let meta = guard.as_ref().map(|m| MediaInfo {
                title: m.title.clone(),
                artist: m.artist.clone(),
                album: m.album.clone(),
                playing: m.playing,
                position: m.position,
                stamp: m.stamp,
                duration: m.duration,
                thumb: None,
                thumb_id: m.thumb_id,
                can_prev: m.can_prev,
                can_next: m.can_next,
                can_seek: m.can_seek,
            });
            (meta, thumb)
        };
        let thumb_id = info.as_ref().map(|m| m.thumb_id).unwrap_or(0);
        if let Some(r) = self.renderer.as_mut() {
            r.set_art(thumb_id, fresh_thumb);
        }
        // Si solo cambió la posición y el notch está contraído, no hay nada que redibujar.
        let looks = |m: &Option<MediaInfo>| {
            m.as_ref().map(|m| (m.title.clone(), m.artist.clone(), m.playing, m.thumb_id, m.can_prev, m.can_next, m.duration > 0.5))
        };
        if looks(&info) == looks(&self.media) && !self.want_expanded() {
            self.media = info.take();
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
        let Some(m) = self.media.as_ref() else { return };
        let (title, artist, album, duration) = (m.title.clone(), m.artist.clone(), m.album.clone(), m.duration);
        self.lyrics_req += 1;
        let id = self.lyrics_req;
        self.lyrics = Some(Lyrics::Loading);
        let shared = self.lyrics_shared.get_or_insert_with(|| Arc::new(Mutex::new(None))).clone();
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let result = lyrics::fetch(&title, &artist, &album, duration);
            *shared.lock().unwrap() = Some((id, result));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_LYRICS, WPARAM(0), LPARAM(0));
            }
        });
    }

    pub(crate) fn on_lyrics(&mut self) {
        let got = self.lyrics_shared.as_ref().and_then(|s| s.lock().unwrap().take());
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
