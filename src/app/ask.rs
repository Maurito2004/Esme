//! Comportamiento de Fase 6 (Ask Esme, Traductor Rápido, Calculadora/Conversor Inline,
//! Temporizador Rápido y acciones de portapapeles con IA), con inicialización diferida (Fase 7).

use crate::ask_esme::{self, PromptTask};
use crate::calc::{self, InlineAction};
use crate::*;
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_BACK, VK_CONTROL, VK_ESCAPE, VK_RETURN, VK_TAB,
};

const CARET_BLINK_MS: u32 = 530;


impl App {
    /// Inicializa `AskState` de forma diferida (solo al abrir Ask Esme por primera vez).
    pub(crate) fn ask_mut(&mut self) -> &mut ask_esme::AskState {
        self.ask.get_or_insert_with(ask_esme::AskState::default)
    }

    /// Carga tasas de divisas desde caché local la primera vez que se abre Ask Esme
    /// y las refresca en segundo plano (máx. 1 vez/día) si vencieron.
    fn ensure_rates(&mut self) {
        if self.rates.is_none() {
            self.rates = Some(calc::RatesCache::load());
        }
        let needs_refresh = self.rates.as_ref().map(|r| !r.is_fresh()).unwrap_or(false);
        if needs_refresh {
            let hwnd = self.hwnd.0 as isize;
            let inbox = self.inbox_slot();
            std::thread::spawn(move || {
                if let Some(r) = calc::refresh_rates_if_needed() {
                    inbox.lock().unwrap().rates = Some(r);
                    unsafe {
                        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_DATA, WPARAM(0), LPARAM(0));
                    }
                }
            });
        }
    }

    /// Abre o cierra Ask Esme (desde `Ctrl+Alt+Space`, bandeja o botón del panel).
    pub(crate) fn toggle_ask(&mut self, force_open: bool) {
        let open = if force_open { true } else { !self.ask_on };
        self.ask_on = open;
        if open {
            let _ = self.ask_mut();
            self.ensure_rates();
            self.lyrics_on = false;
            self.clip_on = false;
            self.shelf_on = false;
            self.today_on = false;
            self.focus_ask_input(true);
        } else {
            self.focus_ask_input(false);
        }
        self.retarget();
        self.refresh();
    }

    /// Activa o desactiva el foco de teclado sobre la ventana del notch y el cursor parpadeante.
    /// Solo cuando el input tiene foco quitamos temporalmente `WS_EX_NOACTIVATE` y encendemos `T_CARET`.
    pub(crate) fn focus_ask_input(&mut self, focus: bool) {
        if let Some(ask) = self.ask.as_mut() {
            ask.focus = focus;
            ask.caret_visible = focus;
            if !focus {
                ask.selected_all = false;
            }
        } else if focus {
            let ask = self.ask_mut();
            ask.focus = true;
            ask.caret_visible = true;
        }
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            if focus {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.hwnd));
                SetTimer(Some(self.hwnd), T_CARET, CARET_BLINK_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), T_CARET);
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 as isize));
            }
        }
    }

    /// Vista previa inline en vivo mientras el usuario escribe (`= 4725`, `= 118000.00 ARS`, `⏱ Enter: 10 min`).
    pub(crate) fn ask_inline_preview(&self) -> Option<String> {
        let ask = self.ask.as_ref()?;
        let rates = self.rates.as_ref()?;
        match calc::evaluate_input(&ask.input, rates) {
            Some(InlineAction::InstantResult(res)) => Some(format!(" {res}")),
            Some(InlineAction::QuickTimer(_, label)) => Some(format!("⏱ Enter → {label}")),
            None => None,
        }
    }

    /// Teclas especiales en el cuadro de texto Direct2D (`Tab`, `Enter`, `Esc`, `Backspace`, `Ctrl+V/C/A`).
    pub(crate) fn on_key_down(&mut self, vk: u32) {
        if !self.ask_on || !self.ask.as_ref().map(|a| a.focus).unwrap_or(false) {
            return;
        }
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
        if ctrl {
            match vk as u8 {
                b'V' => {
                    if let Some(clip_txt) = clipboard::read_text(self.hwnd) {
                        let ask = self.ask_mut();
                        if ask.selected_all {
                            ask.input.clear();
                            ask.selected_all = false;
                        }
                        let clean: String = clip_txt
                            .chars()
                            .map(|c| if c == '\n' || c == '\r' || c.is_control() { ' ' } else { c })
                            .take(300)
                            .collect();
                        ask.input.push_str(clean.trim());
                        self.reset_caret_blink();
                        self.refresh();
                    }
                    return;
                }
                b'A' => {
                    let changed = if let Some(ask) = self.ask.as_mut() {
                        if !ask.input.is_empty() {
                            ask.selected_all = true;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if changed {
                        self.refresh();
                    }
                    return;
                }
                b'C' => {
                    let target = if let Some(ask) = self.ask.as_ref() {
                        if ask.selected_all && !ask.input.is_empty() {
                            ask.input.clone()
                        } else if !ask.response.is_empty() {
                            ask.response.clone()
                        } else {
                            ask.input.clone()
                        }
                    } else {
                        String::new()
                    };
                    if !target.is_empty() && clipboard::set_text(self.hwnd, &target) {
                        self.trigger_ask_copied();
                    }
                    return;
                }
                _ => {}
            }
        }

        match vk as u16 {
            x if x == VK_TAB.0 => {
                let ask = self.ask_mut();
                ask.mode = ask.mode.toggle();
                self.reset_caret_blink();
                self.refresh();
            }
            x if x == VK_ESCAPE.0 => {
                self.toggle_ask(false);
            }
            x if x == VK_BACK.0 => {
                let ask = self.ask_mut();
                if ask.selected_all {
                    ask.input.clear();
                    ask.selected_all = false;
                } else {
                    ask.input.pop();
                }
                self.reset_caret_blink();
                self.refresh();
            }
            x if x == VK_RETURN.0 => {
                self.submit_ask_input();
            }
            _ => {}
        }
    }

    /// Entrada de caracteres Unicode (`WM_CHAR`) en el cuadro de texto Direct2D.
    pub(crate) fn on_char(&mut self, ch: u32) {
        if !self.ask_on || !self.ask.as_ref().map(|a| a.focus).unwrap_or(false) {
            return;
        }
        let Some(c) = char::from_u32(ch) else { return };
        if c.is_control() {
            return;
        }
        let pushed = {
            let ask = self.ask_mut();
            if ask.selected_all {
                ask.input.clear();
                ask.selected_all = false;
            }
            if ask.input.chars().count() < 320 {
                ask.input.push(c);
                true
            } else {
                false
            }
        };
        if pushed {
            self.reset_caret_blink();
            self.refresh();
        }
    }

    fn reset_caret_blink(&mut self) {
        if let Some(ask) = self.ask.as_mut() {
            ask.caret_visible = true;
        }
        unsafe {
            SetTimer(Some(self.hwnd), T_CARET, CARET_BLINK_MS, None);
        }
    }

    /// Ejecuta la acción del input al apretar `Enter`:
    /// 1. Si es cálculo/conversión con `=`, lo copia y lo muestra en el área de respuesta.
    /// 2. Si es `timer 10m`, inicia el temporizador rápido sin tocar la red.
    /// 3. Si es consulta o traducción, dispara el hilo `WinHTTP` contra Gemini.
    pub(crate) fn submit_ask_input(&mut self) {
        let Some((text, mode)) = self.ask.as_ref().map(|a| (a.input.trim().to_string(), a.mode)) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.ensure_rates();
        if let Some(rates) = self.rates.as_ref() {
            if let Some(action) = calc::evaluate_input(&text, rates) {
                match action {
                    InlineAction::InstantResult(res) => {
                        let _ = clipboard::set_text(self.hwnd, &res);
                        let ask = self.ask_mut();
                        ask.loading = false;
                        ask.response = format!("{text} {res}  (copiado al portapapeles)");
                        self.retarget();
                        self.refresh();
                        return;
                    }
                    InlineAction::QuickTimer(secs, label) => {
                        self.start_quick_timer(secs, label);
                        if let Some(ask) = self.ask.as_mut() {
                            ask.input.clear();
                        }
                        self.toggle_ask(false);
                        return;
                    }
                }
            }
        }
        self.run_gemini_task(PromptTask::Mode(mode), text);
    }

    /// Botones rápidos sobre el último texto del portapapeles ("✨ Resumir clip" y "✎ Corregir").
    pub(crate) fn run_clip_ai_action(&mut self, summarize: bool) {
        let Some(last) = self.clip.items.first().cloned().or_else(|| clipboard::read_text(self.hwnd)) else {
            self.ask_mut().response = "El portapapeles está vacío: copiá un texto primero.".into();
            self.retarget();
            self.refresh();
            return;
        };
        let task = if summarize {
            PromptTask::SummarizeClip
        } else {
            PromptTask::SpellcheckClip
        };
        self.run_gemini_task(task, last);
    }

    fn run_gemini_task(&mut self, task: PromptTask, user_text: String) {
        let Some(api_key) = self.cfg.gemini_api_key() else {
            let ask = self.ask_mut();
            ask.loading = false;
            ask.response = "Copiá tu API Key de Gemini y elegí Bandeja → Configuración → IA Gemini → Pegar API Key.".into();
            self.retarget();
            self.refresh();
            return;
        };
        let (req_id, shared) = {
            let ask = self.ask_mut();
            ask.req_id = ask.req_id.wrapping_add(1);
            ask.loading = true;
            ask.copied_feedback = false;
            (ask.req_id, ask.shared.clone())
        };
        self.retarget();
        self.refresh();
        ask_esme::spawn_query(
            self.hwnd,
            req_id,
            api_key,
            self.cfg.gemini_model.clone(),
            task,
            user_text,
            shared,
        );
    }

    /// Recepción de `WM_ASK_DONE` desde el hilo de WinHTTP.
    pub(crate) fn on_ask_done(&mut self) {
        let res = self.ask.as_ref().and_then(|a| a.shared.lock().unwrap().take());
        let Some((id, outcome)) = res else { return };
        let Some(ask) = self.ask.as_mut() else { return };
        if id != ask.req_id {
            return;
        }
        ask.loading = false;
        match outcome {
            Ok(txt) => ask.response = txt,
            Err(err) => ask.response = err,
        }
        self.retarget();
        self.refresh();
    }

    pub(crate) fn trigger_ask_copied(&mut self) {
        if let Some(ask) = self.ask.as_mut() {
            ask.copied_feedback = true;
        }
        unsafe {
            SetTimer(Some(self.hwnd), T_COPIED, 1100, None);
        }
        self.refresh();
    }

    /// Bandeja → Configuración → IA Gemini → Pegar API Key copiada (cifrada con DPAPI).
    pub(crate) fn paste_gemini_key(&mut self) {
        let text = clipboard::read_text(self.hwnd).unwrap_or_default();
        let key = text.trim();
        if key.len() < 20 || key.contains(char::is_whitespace) {
            self.show_toast("Copiá primero una API Key válida de Gemini".into(), 3200);
            return;
        }
        if self.cfg.set_gemini_api_key(key) {
            self.show_toast("🔒 API Key de Gemini guardada con cifrado DPAPI ✓".into(), 2800);
        } else {
            self.show_toast("No pude cifrar la API Key con Windows DPAPI".into(), 3200);
        }
    }

    // ---------- Temporizador rápido (`timer 10m`) ----------

    pub(crate) fn start_quick_timer(&mut self, secs: u64, label: String) {
        let dur = Duration::from_secs(secs);
        self.quick_timer = Some((Instant::now() + dur, dur, label.clone()));
        unsafe {
            SetTimer(Some(self.hwnd), T_QTIMER, 1000, None);
        }
        self.show_toast(format!("⏱ Temporizador de {label} iniciado"), 2200);
    }

    pub(crate) fn quick_timer_status(&self) -> Option<(String, f32)> {
        let (end_at, total, _) = self.quick_timer.as_ref()?;
        let rem = end_at.saturating_duration_since(Instant::now());
        let s = rem.as_secs();
        let txt = if s >= 3600 {
            format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
        } else {
            format!("{:02}:{:02}", s / 60, s % 60)
        };
        let prog = 1.0 - (rem.as_secs_f32() / total.as_secs_f32().max(1.0));
        Some((txt, prog.clamp(0.0, 1.0)))
    }

    pub(crate) fn on_quick_timer_tick(&mut self) {
        let finished = self
            .quick_timer
            .as_ref()
            .map(|(end_at, _, _)| Instant::now() >= *end_at)
            .unwrap_or(false);
        if finished {
            let label = self.quick_timer.take().map(|(_, _, l)| l).unwrap_or_default();
            unsafe {
                let _ = KillTimer(Some(self.hwnd), T_QTIMER);
                let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONASTERISK);
            }
            self.show_toast(format!("🔔 ¡Terminó el temporizador ({label})!"), 4000);
        } else {
            self.refresh();
        }
    }
}
