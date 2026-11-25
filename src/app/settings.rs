//! Ajustes (Fase 8): qué muestra cada sección y qué hace cada control.
//! El dibujo y la ventana están en `crate::settings`.

use crate::settings::{self as st, row, Act, Cmd, Control, Field, Group, Key, Settings};
use crate::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, TrackMouseEvent, VK_BACK, VK_CONTROL, VK_ESCAPE, VK_RETURN};

const FAMILIES: [&str; 3] = ["flash", "flash-lite", "pro"];
const TAMANOS: [u32; 3] = [100, 115, 130];

/// Ventana de Ajustes: reenvía sus mensajes a `App`.
pub(crate) unsafe extern "system" fn settings_wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(Some(r)) = with_app(|a| a.settings_msg(h, msg, wp, lp)) {
        return r;
    }
    DefWindowProcW(h, msg, wp, lp)
}

fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

impl App {
    pub(crate) fn open_settings(&mut self) {
        if let Some(s) = &self.settings {
            unsafe {
                let _ = ShowWindow(s.hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(s.hwnd);
            }
            return;
        }
        if let Some(mut s) = Settings::open(Some(settings_wndproc), self.cfg.tema_menu != "claro") {
            s.city_text = self.cfg.ciudad.clone();
            self.settings = Some(s);
            if let Some(s) = &self.settings {
                s.invalidate();
            }
        }
    }

    /// Contenido de la sección abierta.
    fn settings_groups(&self) -> Vec<Group> {
        let Some(s) = &self.settings else { return Vec::new() };
        let c = &self.cfg;
        let tog = |on: bool, k: Key| Control::Toggle(on, k);
        match s.section {
            0 => vec![
                Group {
                    title: "Inicio",
                    rows: vec![row("Iniciar con Windows", "Esme arranca sola al prender la PC", tog(config::autostart_enabled(), Key::Autostart))],
                },
                Group {
                    title: "Pantalla completa",
                    rows: vec![row(
                        "Pasar al otro monitor",
                        "Con un juego o un video en pantalla completa, el notch se muda al otro\nmonitor. Si está apagado (o tenés un solo monitor), se esconde.",
                        tog(c.mover_monitor, Key::Mover),
                    )],
                },
                Group {
                    title: "Atajos de teclado",
                    rows: vec![
                        row("Ask Esme", "Calculadora, monedas, timer y Gemini", Control::Keys("Ctrl + Alt + Espacio")),
                        row("Capturar texto (OCR)", "Elegís un área de la pantalla y copia el texto", Control::Keys("Ctrl + Alt + T")),
                    ],
                },
            ],
            1 => {
                let size = TAMANOS.iter().position(|t| *t == c.tamano).unwrap_or(0) as u32;
                let pet = match c.pet() {
                    Some(config::Pet::Esme) => 0,
                    Some(config::Pet::Gato) => 1,
                    None => 2,
                };
                vec![
                    Group {
                        title: "Notch",
                        rows: vec![
                            row("Tamaño", "Más grande se lee mejor en monitores grandes", Control::Choice(&["100 %", "115 %", "130 %"], size, Key::Tamano)),
                            row("Notch mínimo", "Una línea fina arriba cuando está cerrado", tog(c.notch_minimo, Key::Minimo)),
                            row("Fondo transparente al expandir", "Solo la píldora queda negra; el resto flota sobre tu escritorio", tog(c.transparente, Key::Transparente)),
                        ],
                    },
                    Group {
                        title: "Mascota",
                        rows: vec![row("Mascota", "Pestañea, te mira y se duerme si no usás la PC", Control::Choice(&["Esme", "Gato", "Ninguna"], pet, Key::Pet))],
                    },
                    Group {
                        title: "Tema",
                        rows: vec![row("Tema", "Colores de esta ventana y del menú de la bandeja", Control::Choice(&["Oscuro", "Claro"], (c.tema_menu == "claro") as u32, Key::Tema))],
                    },
                ]
            }
            2 => {
                let city_desc = if !s.status[0].is_empty() {
                    s.status[0].clone()
                } else if !c.ciudad.is_empty() {
                    format!("Clima de {}", c.ciudad)
                } else {
                    "Sin ciudad: se usa la ubicación de Windows (si está activada)".into()
                };
                let cal_state = if !s.status[1].is_empty() {
                    s.status[1].clone()
                } else if c.ical_url.is_empty() {
                    "Estado: sin conectar".into()
                } else {
                    "Estado: conectado ✓".into()
                };
                vec![
                    Group {
                        title: "Qué mostrar en el panel Hoy",
                        rows: vec![
                            row("Pomodoro", "", tog(c.hoy_pomodoro, Key::HoyPomo)),
                            row("Agua", "", tog(c.hoy_agua, Key::HoyAgua)),
                            row("Clima", "", tog(c.hoy_clima, Key::HoyClima)),
                            row("Calendario", "", tog(c.hoy_calendario, Key::HoyCal)),
                            row("Tareas (To-Do)", "Lista rápida con avisos por hora (HH:MM)", tog(c.hoy_todo, Key::HoyTodo)),
                        ],
                    },
                    Group {
                        title: "Clima",
                        rows: vec![row(
                            "Tu ciudad",
                            city_desc,
                            Control::Field(Field::City, "Por ejemplo: Córdoba", vec![("Buscar", Cmd::CitySearch), ("Quitar", Cmd::CityClear)]),
                        )],
                    },
                    Group {
                        title: "Calendario de Google",
                        rows: vec![row(
                            "Conectar tu calendario",
                            format!(
                                "1. Tocá «Abrir Google Calendar» y elegí tu calendario a la izquierda.\n2. Bajá hasta «Dirección secreta en formato iCal» y copiala.\n3. Volvé acá y tocá «Pegar dirección».\n{cal_state}"
                            ),
                            Control::Buttons(vec![
                                ("Abrir Google Calendar", Cmd::IcalOpenGoogle),
                                ("Pegar dirección", Cmd::IcalPaste),
                                ("Quitar", Cmd::IcalClear),
                            ]),
                        )],
                    },
                    Group {
                        title: "Pomodoro y agua",
                        rows: vec![
                            row("Foco", "Minutos de trabajo", Control::Stepper(format!("{} min", c.pomo_foco), Key::PomoFoco)),
                            row("Descanso", "Minutos de pausa", Control::Stepper(format!("{} min", c.pomo_descanso), Key::PomoDescanso)),
                            row("Meta de agua", "Vasos de 250 ml por día", Control::Stepper(format!("{} vasos", c.agua_meta), Key::AguaMeta)),
                        ],
                    },
                ]
            }
            3 => {
                let key_desc = if !s.status[2].is_empty() {
                    s.status[2].clone()
                } else if c.has_gemini_key() {
                    "Guardada y cifrada con tu usuario de Windows ✓".into()
                } else {
                    "Sin configurar. Es gratis: tocá «Conseguir key», copiala y tocá «Pegar key».".into()
                };
                let model = FAMILIES.iter().position(|m| *m == c.gemini_familia).unwrap_or(0) as u32;
                let model_desc = if c.gemini_model.is_empty() {
                    "Se elige el más nuevo disponible\nEsme cambia sola si uno se satura".to_string()
                } else {
                    format!("Usando {}\nEsme cambia sola si se satura", c.gemini_model)
                };
                vec![
                    Group {
                        title: "Gemini",
                        rows: vec![
                            row(
                                "API Key",
                                key_desc,
                                Control::Buttons(vec![("Conseguir key", Cmd::KeyGet), ("Pegar key", Cmd::KeyPaste), ("Borrar", Cmd::KeyClear)]),
                            ),
                            row("Modelo", model_desc, Control::Choice(&["Flash", "Flash-Lite", "Pro"], model, Key::Modelo)),
                        ],
                    },
                    Group {
                        title: "Privacidad",
                        rows: vec![row(
                            "Qué se envía",
                            "Solo lo que escribís en Preguntar o Traducir viaja a Google por HTTPS.\nLa calculadora, las monedas y el timer funcionan sin key y sin IA.",
                            Control::None,
                        )],
                    },
                    Group {
                        title: "Atajo",
                        rows: vec![row("Abrir Ask Esme", "Tab cambia entre Preguntar y Traducir", Control::Keys("Ctrl + Alt + Espacio"))],
                    },
                ]
            }
            4 => vec![Group {
                title: "Mostrar en la píldora",
                rows: vec![
                    row("Volumen", "Al subir, bajar o silenciar", tog(c.aviso_volumen, Key::AvisoVolumen)),
                    row("Salida de audio", "Al conectar auriculares o parlantes", tog(c.aviso_audio, Key::AvisoAudio)),
                    row("Idioma del teclado", "Al cambiar la distribución (Win + Espacio)", tog(c.aviso_teclado, Key::AvisoTeclado)),
                    row("Cargador", "Notebooks: al enchufar o desenchufar", tog(c.aviso_energia, Key::AvisoEnergia)),
                    row("Brillo", "Notebooks: al cambiar el brillo de la pantalla", tog(c.aviso_brillo, Key::AvisoBrillo)),
                    row("Batería baja", "Notebooks: al bajar de 20 % y de 10 %", tog(c.aviso_bateria, Key::AvisoBateria)),
                    row("Wi-Fi", "Al conectar o desconectar una red Wi-Fi", tog(c.aviso_wifi, Key::AvisoWifi)),
                ],
            }],
            _ => vec![
                Group {
                    title: "",
                    rows: vec![row(
                        concat!("Esme ", env!("CARGO_PKG_VERSION")),
                        "Barra dinámica para Windows 10 y 11, hecha en Rust.\nGratis, sin cuentas y 100 % local: unos 4 MB de memoria en reposo.",
                        Control::Buttons(vec![("Ver en GitHub", Cmd::OpenRepo), ("Abrir carpeta de datos", Cmd::OpenData)]),
                    )],
                },
            ],
        }
    }

    pub(crate) fn settings_msg(&mut self, h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if self.settings.as_ref()?.hwnd != h {
            return None;
        }
        let xy = |lp: LPARAM| ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
        match msg {
            WM_PAINT => unsafe {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(h, &mut ps);
                let groups = self.settings_groups();
                let dark = self.cfg.tema_menu != "claro";
                if let Some(s) = self.settings.as_mut() {
                    s.paint(&groups, dark);
                }
                let _ = EndPaint(h, &ps);
                Some(LRESULT(0))
            },
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_SIZE => {
                let s = self.settings.as_mut()?;
                s.on_size();
                s.invalidate();
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                let s = self.settings.as_mut()?;
                let r = unsafe { *(lp.0 as *const RECT) };
                s.on_dpi((wp.0 & 0xFFFF) as u32, &r);
                s.invalidate();
                Some(LRESULT(0))
            }
            WM_GETMINMAXINFO => {
                let s = self.settings.as_ref()?;
                let k = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(s.hwnd).max(96) } as f32 / 96.0;
                let mm = unsafe { &mut *(lp.0 as *mut MINMAXINFO) };
                mm.ptMinTrackSize = POINT { x: (660.0 * k) as i32, y: (460.0 * k) as i32 };
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let (x, y) = xy(lp);
                let s = self.settings.as_mut()?;
                let hv = s.hit(x, y);
                if hv != s.hover {
                    s.hover = hv;
                    s.invalidate();
                }
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: h,
                    dwHoverTime: 0,
                };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                let s = self.settings.as_mut()?;
                if s.hover.take().is_some() {
                    s.invalidate();
                }
                Some(LRESULT(0))
            }
            WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
                let s = self.settings.as_ref()?;
                let mut p = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut p);
                    let _ = ScreenToClient(h, &mut p);
                    let cur = match s.hit(p.x, p.y) {
                        Some(Act::Focus(_)) => IDC_IBEAM,
                        Some(_) => IDC_HAND,
                        None => IDC_ARROW,
                    };
                    let _ = SetCursor(LoadCursorW(None, cur).ok());
                }
                Some(LRESULT(1))
            }
            WM_LBUTTONDOWN => {
                let (x, y) = xy(lp);
                let act = self.settings.as_ref()?.hit(x, y);
                match act {
                    Some(a) => self.settings_act(a),
                    None => self.settings_focus(None),
                }
                if let Some(s) = &self.settings {
                    s.invalidate();
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let s = self.settings.as_mut()?;
                s.wheel(((wp.0 >> 16) & 0xFFFF) as i16);
                s.invalidate();
                Some(LRESULT(0))
            }
            WM_CHAR => {
                let s = self.settings.as_mut()?;
                if s.focus == Some(Field::City) {
                    if let Some(c) = char::from_u32(wp.0 as u32).filter(|c| !c.is_control()) {
                        if s.city_text.chars().count() < 60 {
                            s.city_text.push(c);
                            s.caret_on = true;
                            s.invalidate();
                        }
                    }
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                let vk = wp.0 as u16;
                let focused = self.settings.as_ref()?.focus.is_some();
                if !focused {
                    return None;
                }
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                if vk == VK_RETURN.0 {
                    self.settings_act(Act::Do(Cmd::CitySearch));
                } else if vk == VK_ESCAPE.0 {
                    self.settings_focus(None);
                } else if vk == VK_BACK.0 {
                    let s = self.settings.as_mut()?;
                    if ctrl {
                        s.city_text.clear();
                    } else {
                        s.city_text.pop();
                    }
                } else if ctrl && vk == b'V' as u16 {
                    let pasted = clipboard::read_text(h).unwrap_or_default();
                    let s = self.settings.as_mut()?;
                    let line: String = pasted.lines().next().unwrap_or("").trim().chars().take(60).collect();
                    s.city_text.push_str(&line);
                }
                if let Some(s) = &self.settings {
                    s.invalidate();
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == st::T_CARET => {
                let s = self.settings.as_mut()?;
                s.caret_on = !s.caret_on;
                s.invalidate();
                Some(LRESULT(0))
            }
            st::WM_GEO_DONE => {
                self.on_geo_done();
                Some(LRESULT(0))
            }
            st::WM_KEY_DONE => {
                self.on_key_done();
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                unsafe {
                    let _ = DestroyWindow(h);
                }
                // Al soltar `Settings` se liberan la GPU y las fuentes de la ventana.
                self.settings = None;
                // Y a los 15 s le devolvemos a Windows la memoria que usó la ventana.
                if !self.want_expanded() {
                    self.schedule_trim();
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    fn settings_focus(&mut self, f: Option<Field>) {
        let Some(s) = self.settings.as_mut() else { return };
        s.focus = f;
        s.caret_on = f.is_some();
        unsafe {
            if f.is_some() {
                SetTimer(Some(s.hwnd), st::T_CARET, 530, None);
            } else {
                let _ = KillTimer(Some(s.hwnd), st::T_CARET);
            }
        }
    }

    fn set_status(&mut self, i: usize, msg: impl Into<String>) {
        if let Some(s) = self.settings.as_mut() {
            s.status[i] = msg.into();
        }
    }

    fn settings_act(&mut self, act: Act) {
        if !matches!(act, Act::Focus(_)) && !matches!(act, Act::Do(Cmd::CitySearch)) {
            self.settings_focus(None);
        }
        match act {
            Act::Section(i) => {
                if let Some(s) = self.settings.as_mut() {
                    s.section = i;
                    s.scroll = 0.0;
                    s.status = Default::default();
                }
            }
            Act::Focus(f) => self.settings_focus(Some(f)),
            Act::Toggle(k) => self.settings_toggle(k),
            Act::Choose(k, i) => self.settings_choose(k, i as usize),
            Act::Step(k, d) => self.settings_step(k, d),
            Act::Do(c) => self.settings_cmd(c),
        }
    }

    fn settings_toggle(&mut self, k: Key) {
        let c = &mut self.cfg;
        match k {
            Key::Autostart => {
                config::set_autostart(!config::autostart_enabled());
                return;
            }
            Key::Mover => return self.on_command(CMD_MOVER),
            Key::Minimo => return self.on_command(CMD_MINIMO),
            Key::Transparente => return self.on_command(CMD_TRANSPARENTE),
            Key::HoyPomo => c.hoy_pomodoro = !c.hoy_pomodoro,
            Key::HoyAgua => c.hoy_agua = !c.hoy_agua,
            Key::HoyClima => c.hoy_clima = !c.hoy_clima,
            Key::HoyCal => c.hoy_calendario = !c.hoy_calendario,
            Key::HoyTodo => c.hoy_todo = !c.hoy_todo,
            Key::AvisoVolumen => c.aviso_volumen = !c.aviso_volumen,
            Key::AvisoAudio => c.aviso_audio = !c.aviso_audio,
            Key::AvisoTeclado => c.aviso_teclado = !c.aviso_teclado,
            Key::AvisoEnergia => c.aviso_energia = !c.aviso_energia,
            Key::AvisoBrillo => c.aviso_brillo = !c.aviso_brillo,
            Key::AvisoBateria => c.aviso_bateria = !c.aviso_bateria,
            Key::AvisoWifi => c.aviso_wifi = !c.aviso_wifi,
            _ => return,
        }
        let _ = self.cfg.save();
        if matches!(k, Key::HoyClima | Key::HoyCal) && self.today_loaded {
            self.refresh_data();
        }
        if matches!(k, Key::HoyPomo | Key::HoyAgua | Key::HoyClima | Key::HoyCal | Key::HoyTodo) {
            if k == Key::HoyTodo && !self.cfg.hoy_todo {
                self.focus_todo_input(false);
            }
            self.retarget();
            self.refresh();
        }
    }

    fn settings_choose(&mut self, k: Key, i: usize) {
        match k {
            Key::Tamano => {
                self.cfg.tamano = TAMANOS[i.min(2)];
                let _ = self.cfg.save();
                self.layout();
                self.retarget();
                self.redraw();
            }
            Key::Pet => self.on_command([CMD_PET_ESME, CMD_PET_GATO, CMD_PET_OFF][i.min(2)]),
            Key::Tema => {
                self.on_command(if i == 1 { CMD_TEMA_CLARO } else { CMD_TEMA_OSCURO });
                if let Some(s) = &self.settings {
                    st::set_titlebar(s.hwnd, i != 1);
                    // Fuerza a Windows a repintar la barra de título con el color nuevo.
                    unsafe {
                        let _ = SetWindowPos(s.hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
                    }
                }
            }
            Key::Modelo => {
                self.cfg.gemini_familia = FAMILIES[i.min(2)].into();
                // Se elige el más nuevo de la familia en la próxima consulta (o ya, si hay key).
                self.cfg.gemini_model.clear();
                let _ = self.cfg.save();
                self.check_gemini_key(false);
            }
            _ => {}
        }
    }

    fn settings_step(&mut self, k: Key, d: i32) {
        let c = &mut self.cfg;
        match k {
            Key::AguaMeta => c.agua_meta = (c.agua_meta as i32 + d).clamp(1, 20) as u32,
            Key::PomoFoco => c.pomo_foco = (c.pomo_foco as i32 + 5 * d).clamp(5, 90) as u32,
            Key::PomoDescanso => c.pomo_descanso = (c.pomo_descanso as i32 + d).clamp(1, 30) as u32,
            _ => return,
        }
        let _ = self.cfg.save();
        self.pomo.set_durations(self.cfg.pomo_foco, self.cfg.pomo_descanso);
        self.refresh();
    }

    fn settings_cmd(&mut self, c: Cmd) {
        match c {
            Cmd::CitySearch => {
                let Some(s) = self.settings.as_mut() else { return };
                let q = s.city_text.trim().to_string();
                if q.is_empty() {
                    s.status[0] = "Escribí el nombre de tu ciudad".into();
                    return;
                }
                s.status[0] = format!("Buscando «{q}»…");
                let slot = s.geo.clone();
                let hwnd = s.hwnd.0 as isize;
                self.settings_focus(None);
                std::thread::spawn(move || {
                    *slot.lock().unwrap() = Some(weather::geocode(&q));
                    unsafe {
                        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), st::WM_GEO_DONE, WPARAM(0), LPARAM(0));
                    }
                });
            }
            Cmd::CityClear => {
                self.cfg.ciudad.clear();
                self.cfg.ciudad_lat = 0.0;
                self.cfg.ciudad_lon = 0.0;
                let _ = self.cfg.save();
                if let Some(s) = self.settings.as_mut() {
                    s.city_text.clear();
                }
                self.set_status(0, "");
                if self.today_loaded {
                    self.refresh_data();
                }
            }
            Cmd::IcalOpenGoogle => open_url("https://calendar.google.com/calendar/r/settings"),
            Cmd::IcalPaste => {
                let hwnd = self.settings.as_ref().map(|s| s.hwnd).unwrap_or(self.hwnd);
                let text = clipboard::read_text(hwnd).unwrap_or_default();
                let url = text.trim();
                if url.starts_with("https://") && (url.contains(".ics") || url.contains("/ical/")) {
                    self.cfg.ical_url = url.to_string();
                    let _ = self.cfg.save();
                    self.cal_msg = "Cargando calendario…".into();
                    if self.today_loaded {
                        self.refresh_data();
                    }
                    self.set_status(1, "Estado: conectado ✓");
                } else {
                    self.set_status(1, "Eso no parece la dirección iCal: copiá la que termina en «basic.ics»");
                }
            }
            Cmd::IcalClear => {
                self.on_command(CMD_ICAL_CLEAR);
                self.set_status(1, "Estado: sin conectar");
            }
            Cmd::KeyGet => open_url("https://aistudio.google.com/apikey"),
            Cmd::KeyPaste => {
                let hwnd = self.settings.as_ref().map(|s| s.hwnd).unwrap_or(self.hwnd);
                let text = clipboard::read_text(hwnd).unwrap_or_default();
                let key = text.trim();
                if key.len() < 20 || key.contains(char::is_whitespace) {
                    self.set_status(2, "Eso no parece una API Key: copiala de nuevo desde Google AI Studio");
                } else if self.cfg.set_gemini_api_key(key) {
                    self.cfg.gemini_model.clear();
                    let _ = self.cfg.save();
                    self.check_gemini_key(true);
                } else {
                    self.set_status(2, "No pude cifrar la key con Windows");
                }
            }
            Cmd::KeyClear => {
                let _ = self.cfg.set_gemini_api_key("");
                self.set_status(2, "Key borrada");
            }
            Cmd::OpenRepo => open_url("https://github.com/Maurito2004/Esme"),
            Cmd::OpenData => {
                let dir = config::data_dir();
                let _ = std::fs::create_dir_all(&dir);
                open_url(&dir.to_string_lossy());
            }
        }
    }

    /// Comprueba la key en segundo plano pidiéndole a Google la lista de modelos,
    /// y de paso elige el más nuevo de la familia.
    fn check_gemini_key(&mut self, announce: bool) {
        let Some(key) = self.cfg.gemini_api_key() else { return };
        let family = self.cfg.gemini_familia.clone();
        let Some(s) = self.settings.as_mut() else { return };
        if announce {
            s.status[2] = "Comprobando la key con Google…".into();
        }
        let slot = s.key_check.clone();
        let hwnd = s.hwnd.0 as isize;
        std::thread::spawn(move || {
            let res = ask_esme::list_models(&key).and_then(|m| {
                ask_esme::pick_model(&m, &family).ok_or_else(|| "Tu key no tiene modelos Gemini de texto disponibles".to_string())
            });
            *slot.lock().unwrap() = Some(res);
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), st::WM_KEY_DONE, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn on_key_done(&mut self) {
        let Some(res) = self.settings.as_ref().and_then(|s| s.key_check.lock().unwrap().take()) else { return };
        match res {
            Ok(model) => {
                self.cfg.gemini_model = model.clone();
                let _ = self.cfg.save();
                self.set_status(2, format!("Key válida y cifrada ✓ · modelo {model}"));
            }
            Err(e) => self.set_status(2, format!("La key no funciona: {e}")),
        }
        if let Some(s) = &self.settings {
            s.invalidate();
        }
    }

    fn on_geo_done(&mut self) {
        let Some(res) = self.settings.as_ref().and_then(|s| s.geo.lock().unwrap().take()) else { return };
        match res {
            Ok((label, lat, lon)) => {
                self.cfg.ciudad = label.clone();
                self.cfg.ciudad_lat = lat;
                self.cfg.ciudad_lon = lon;
                let _ = self.cfg.save();
                if let Some(s) = self.settings.as_mut() {
                    s.city_text = label.clone();
                }
                self.set_status(0, format!("✓ Listo: clima de {label}"));
                if self.today_loaded {
                    self.weather_msg = "Buscando el clima…".into();
                    self.refresh_data();
                }
            }
            Err(e) => self.set_status(0, e),
        }
        if let Some(s) = &self.settings {
            s.invalidate();
        }
    }
}
