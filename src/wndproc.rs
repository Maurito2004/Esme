//! Procedimiento de ventana: reparte los mensajes de Windows a los métodos de `App`.

use crate::*;

pub(crate) extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE => {
                let drag = with_app(|a| {
                    a.on_mouse_move(lp);
                    a.drag_candidate(wp, lp)
                })
                .flatten();
                // DoDragDrop tiene su propio bucle de mensajes: lo llamamos sin tener el estado prestado.
                if let Some(path) = drag {
                    shelf::drag_out(&path);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                with_app(|a| a.on_mouse_down(lp));
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                // Clic derecho sobre un archivo del estante: lo quita.
                with_app(|a| {
                    let (x, y) = a.to_dips(lp);
                    if let Some(Btn::ShelfItem(i)) = a.layout_now().hit(x, y, 0.0, 0, 0, a.shelf_on_count()) {
                        a.shelf.remove(i);
                        a.retarget();
                        a.refresh();
                    }
                });
                LRESULT(0)
            }
            WM_HOTKEY => {
                if wp.0 as i32 == HOTKEY_OCR {
                    ocr::start(hwnd);
                }
                LRESULT(0)
            }
            shelf::WM_SHELF_DRAG => {
                with_app(|a| a.on_shelf_drag(wp.0 == 1));
                LRESULT(0)
            }
            shelf::WM_SHELF_DROP => {
                with_app(|a| a.on_shelf_drop());
                LRESULT(0)
            }
            ocr::WM_OCR_DONE => {
                with_app(|a| a.on_ocr_done());
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|a| a.on_mouse_leave());
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                with_app(|a| {
                    // Si hubo arrastre, `press` ya se limpió y no es un clic.
                    let pressed = a.press.take();
                    if pressed.is_some() || a.layout_now().hit(a.to_dips(lp).0, a.to_dips(lp).1, 0.0, 0, 0, a.shelf_on_count()).map(|b| !matches!(b, Btn::ShelfItem(_))).unwrap_or(true) {
                        a.on_click(lp);
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                with_app(|a| a.on_wheel(wp));
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // Menú de la bandeja dibujado por nosotros (tema oscuro/claro).
            WM_MEASUREITEM if tray::measure_item(lp) => LRESULT(1),
            WM_DRAWITEM if tray::draw_item(lp) => LRESULT(1),
            // Los mismos comandos del menú, también por mensaje (útil para automatizar).
            WM_COMMAND => {
                let cmd = (wp.0 & 0xFFFF) as u32;
                with_app(|a| a.on_command(cmd));
                LRESULT(0)
            }
            WM_TIMER => {
                with_app(|a| a.on_timer(wp.0));
                LRESULT(0)
            }
            media::WM_MEDIA => {
                with_app(|a| a.on_media());
                LRESULT(0)
            }
            WM_LYRICS => {
                with_app(|a| a.on_lyrics());
                LRESULT(0)
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                with_app(|a| {
                    // Los monitores pueden haber cambiado: volvemos al principal y re-evaluamos.
                    a.monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
                    a.layout();
                    a.check_fullscreen();
                    a.redraw();
                });
                LRESULT(0)
            }
            WM_CLIPBOARDUPDATE => {
                with_app(|a| a.on_clipboard());
                LRESULT(0)
            }
            WM_DATA => {
                with_app(|a| a.on_data());
                LRESULT(0)
            }
            system::WM_SYS => {
                with_app(|a| a.on_sys(wp.0, lp.0));
                LRESULT(0)
            }
            WM_POWERBROADCAST if wp.0 == PBT_POWERSETTINGCHANGE => {
                with_app(|a| a.on_power(lp));
                LRESULT(1)
            }
            WM_APPBAR => {
                if wp.0 == ABN_FULLSCREENAPP {
                    SetTimer(Some(hwnd), T_FS, 80, None);
                }
                LRESULT(0)
            }
            WM_TRAY => {
                let event = (lp.0 & 0xFFFF) as u32;
                match event {
                    // Con NOTIFYICON_VERSION_4 el clic derecho manda WM_RBUTTONUP y después
                    // WM_CONTEXTMENU: abrimos el menú una sola vez, con el segundo.
                    WM_CONTEXTMENU => {
                        // El menú es modal (despacha mensajes): no retener el estado mientras está abierto.
                        let info = with_app(|a| {
                            (
                                a.tray.clone(),
                                MenuState {
                                    autostart: config::autostart_enabled(),
                                    pet: match a.cfg.pet() {
                                        Some(config::Pet::Esme) => 0,
                                        Some(config::Pet::Gato) => 1,
                                        None => 2,
                                    },
                                    minimo: a.cfg.notch_minimo,
                                    transparente: a.cfg.transparente,
                                    mover: a.cfg.mover_monitor,
                                    dark: a.cfg.tema_menu != "claro",
                                    water_goal: a.cfg.agua_meta,
                                    has_ical: !a.cfg.ical_url.is_empty(),
                                },
                            )
                        });
                        if let Some((tray, state)) = info {
                            let cmd = tray.show_menu(&state);
                            if cmd != 0 {
                                with_app(|a| a.on_command(cmd));
                            }
                        }
                    }
                    NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP => {
                        with_app(|a| {
                            a.pinned = !a.pinned;
                            a.retarget();
                        });
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                let mut abd = windows::Win32::UI::Shell::APPBARDATA {
                    cbSize: std::mem::size_of::<windows::Win32::UI::Shell::APPBARDATA>() as u32,
                    hWnd: hwnd,
                    ..Default::default()
                };
                windows::Win32::UI::Shell::SHAppBarMessage(windows::Win32::UI::Shell::ABM_REMOVE, &mut abd);
                with_app(|a| a.tray.remove());
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => {
                let sm = with_app(|a| a.shell_msg).unwrap_or(0);
                if sm != 0 && msg == sm {
                    if (wp.0 & 0x7FFF) == HSHELL_LANGUAGE {
                        let name = system::layout_name(lp.0);
                        if !name.is_empty() {
                            with_app(|a| a.show_overlay("\u{E765}", name, None));
                        }
                    }
                    return LRESULT(0);
                }
                let tc = with_app(|a| a.taskbar_created).unwrap_or(0);
                if tc != 0 && msg == tc {
                    with_app(|a| a.tray.add());
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
    }
}
