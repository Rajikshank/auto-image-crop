//! Native overlay. Capture/inference never execute inside the window procedure.
use crate::settings::{Hotkey, Settings};
use crate::{
    Candidates, Frame, Mask, Session, extract,
    platform::{self, Desktop},
    worker::{Event, Worker},
};
use anyhow::{Context, Result};
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::DataExchange::GetClipboardSequenceNumber,
        System::LibraryLoader::*,
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

thread_local! {static APP:RefCell<Option<App>>=const {RefCell::new(None)};}
struct MergeIntent {
    revision: u64,
    add: bool,
    region: Option<crate::roi::Region>,
}
type SelectionSnapshot = (Mask, Vec<(f32, f32, bool)>);
type ExportResult = (u64, u64, Result<platform::PreparedClipboard, String>);
enum ExportDestination {
    Clipboard,
    File {
        path: PathBuf,
        format: crate::file_export::Format,
        replace: bool,
    },
}
struct App {
    hwnd: HWND,
    worker: Worker,
    session: Session,
    desktop: Option<Desktop>,
    candidates: Candidates,
    selected: Option<Mask>,
    editor: Option<crate::editor::Editor>,
    controls: crate::native_dock::NativeDock,
    dock_anchor: Option<(i32, i32)>,
    dock_drag: Option<((u32, u32), (i32, i32))>,
    inspection: Option<crate::editor::Background>,
    smart: bool,
    merge: Option<MergeIntent>,
    replace_revision: Option<u64>,
    edge_tool: bool,
    brush_radius: f32,
    brush_alpha: u8,
    brush_soft: bool,
    view: crate::editor::View,
    screen_pointer: (u32, u32),
    lens: bool,
    viewport: Option<(i32, i32, u32, u32)>,
    monitor_dpi: u32,
    hover: Option<Arc<Mask>>,
    presented_hover: Option<Arc<Mask>>,
    pressed_hover: Option<Arc<Mask>>,
    hover_key: usize,
    cycle: usize,
    pointer: (u32, u32),
    points: Vec<(f32, f32, bool)>,
    history: Vec<SelectionSnapshot>,
    stroke: Vec<(u32, u32)>,
    dragging: bool,
    dock_pressed: Option<crate::dock::Action>,
    brush: bool,
    status: String,
    provider: String,
    loading: bool,
    busy: bool,
    quit: bool,
    capture_rx: Option<mpsc::Receiver<Result<Desktop, String>>>,
    canvas: Vec<u8>,
    presenter: crate::presentation::Presenter,
    gdi_presenter: crate::presentation::CachedGdi,
    canvas_revision: u64,
    transition: Option<Instant>,
    preview_color: [u8; 3],
    preview_strength: u8,
    mask_view: bool,
    once: bool,
    started: Instant,
    copy_on_result: bool,
    fixture: Option<Frame>,
    smoke: bool,
    smoke_done: bool,
    smoke_phase: u8,
    smoke_settled: bool,
    smoke_at: Instant,
    smoke_expected: Option<Frame>,
    smoke_hover: Option<(Arc<Mask>, u64)>,
    model_error: Option<String>,
    settings: Settings,
    models: PathBuf,
    hotkey_id: i32,
    export_rx: Option<mpsc::Receiver<ExportResult>>,
    file_job: Option<crate::file_export::FileJob>,
    save_notice_until: Option<Instant>,
}

pub fn run(
    models: PathBuf,
    settings: Settings,
    once: bool,
    fixture: Option<Frame>,
    smoke: bool,
) -> Result<()> {
    let mut settings = settings;
    if smoke {
        settings.quick_copy = false;
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: w!("ObjectLiftOverlay"),
            hCursor: LoadCursorW(None, IDC_CROSS)?,
            hIcon: app_icon(),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            anyhow::bail!("Cannot register overlay window");
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT,
            w!("ObjectLiftOverlay"),
            w!("ObjectLift"),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        if let Err(e) = RegisterHotKey(
            Some(hwnd),
            1,
            HOT_KEY_MODIFIERS(settings.hotkey.modifier_bits()) | MOD_NOREPEAT,
            settings.hotkey.key(),
        ) {
            DestroyWindow(hwnd)?;
            return Err(e).context("ObjectLift shortcut is already in use. Close the conflicting app or edit %LOCALAPPDATA%/ObjectLift/settings.json.");
        }
        let worker = Worker::spawn(models.clone(), !settings.cpu_only);
        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                hwnd,
                worker,
                session: Session::default(),
                desktop: None,
                candidates: Candidates::default(),
                selected: None,
                controls: crate::native_dock::NativeDock::default(),
                dock_anchor: None,
                dock_drag: None,
                editor: None,
                inspection: None,
                smart: false,
                merge: None,
                replace_revision: None,
                edge_tool: false,
                brush_radius: 10.,
                brush_alpha: 255,
                brush_soft: false,
                view: crate::editor::View::default(),
                screen_pointer: (0, 0),
                lens: false,
                viewport: None,
                monitor_dpi: 96,
                hover: None,
                presented_hover: None,
                pressed_hover: None,
                hover_key: 0,
                cycle: 0,
                pointer: (0, 0),
                points: Vec::new(),
                history: Vec::new(),
                stroke: Vec::new(),
                dragging: false,
                dock_pressed: None,
                brush: false,
                status: "Loading local modelsâ€¦".into(),
                provider: String::new(),
                loading: true,
                busy: false,
                quit: false,
                capture_rx: None,
                canvas: Vec::new(),
                presenter: crate::presentation::Presenter::default(),
                gdi_presenter: crate::presentation::CachedGdi::default(),
                canvas_revision: 0,
                transition: None,
                preview_color: [137, 151, 255],
                preview_strength: 89,
                mask_view: false,
                once,
                started: Instant::now(),
                copy_on_result: false,
                fixture,
                smoke,
                smoke_done: false,
                smoke_phase: 0,
                smoke_settled: false,
                smoke_at: Instant::now(),
                smoke_expected: None,
                smoke_hover: None,
                model_error: None,
                settings,
                models,
                hotkey_id: 1,
                export_rx: None,
                file_job: None,
                save_notice_until: None,
            })
        });
        tray(hwnd, true);
        println!(
            "ObjectLift running. Tray: double-click to snip; right-click for shortcut, CPU mode, and Exit."
        );
        let clipboard_sequence = GetClipboardSequenceNumber();
        if smoke {
            let _ = PostMessageW(Some(hwnd), WM_HOTKEY, WPARAM(1), LPARAM(0));
        } else if once {
            APP.with(|a| a.borrow_mut().as_mut().unwrap().activate());
        }
        loop {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break;
                }
                if msg.message == WM_KEYDOWN
                    && msg.wParam.0 == 9
                    && GetKeyState(0x11) >= 0
                    && IsDialogMessageW(hwnd, &msg).as_bool()
                {
                    continue;
                }
                if msg.message == WM_KEYDOWN
                    && msg.hwnd != hwnd
                    && !matches!(msg.wParam.0, 9 | 32 | 37 | 38 | 39 | 40)
                {
                    let _ = PostMessageW(Some(hwnd), WM_KEYDOWN, msg.wParam, msg.lParam);
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let quit = APP.with(|a| {
                let mut a = a.borrow_mut();
                let app = a.as_mut().unwrap();
                app.poll();
                app.quit
            });
            if quit {
                break;
            }
            let timeout = APP.with(|a| {
                a.borrow()
                    .as_ref()
                    .map(|app| {
                        if app.desktop.is_some() || app.capture_rx.is_some() {
                            12
                        } else {
                            250
                        }
                    })
                    .unwrap_or(250)
            });
            MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
        let passed = APP.with(|a| a.borrow().as_ref().is_some_and(|a| a.smoke_done))
            && (!smoke
                || (!IsWindowVisible(hwnd).as_bool()
                    && GetClipboardSequenceNumber() == clipboard_sequence));
        tray(hwnd, false);
        let _ = UnregisterHotKey(Some(hwnd), 1);
        let _ = UnregisterHotKey(Some(hwnd), 2);
        APP.with(|a| {
            a.borrow_mut().take();
        });
        DestroyWindow(hwnd)?;
        if smoke && !passed {
            anyhow::bail!("Native overlay smoke test did not complete");
        }
    }
    Ok(())
}

fn app_icon() -> HICON {
    unsafe {
        GetModuleHandleW(None)
            .and_then(|module| LoadIconW(Some(module.into()), PCWSTR(101usize as *const u16)))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
    }
}

unsafe fn tray(hwnd: HWND, add: bool) {
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_APP + 1,
            hIcon: app_icon(),
            ..Default::default()
        };
        let tip: Vec<u16> = "ObjectLift Â· Ctrl+Shift+O".encode_utf16().collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = Shell_NotifyIconW(if add { NIM_ADD } else { NIM_DELETE }, &data);
    }
}

unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if !matches!(
            msg,
            WM_HOTKEY
                | WM_PAINT
                | WM_ERASEBKGND
                | WM_MOUSEMOVE
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_CANCELMODE
                | WM_CAPTURECHANGED
                | WM_MOUSEWHEEL
                | WM_KEYDOWN
                | WM_CLOSE
                | WM_DISPLAYCHANGE
                | WM_COMMAND
                | WM_DRAWITEM
                | WM_HSCROLL
                | WM_NOTIFY
        ) && msg != WM_APP + 1
            && msg != WM_APP + 11
            && msg != WM_APP + 12
        {
            return DefWindowProcW(hwnd, msg, w, l);
        }
        if msg == WM_ERASEBKGND {
            return LRESULT(1);
        }
        // These paint callbacks need no application state and can be reentrant
        // while native controls are being laid out or the parent is painting.
        if msg == WM_NOTIFY && crate::native_dock::NativeDock::tooltip(l) {
            return LRESULT(0);
        }
        if msg == WM_NOTIFY
            && let Some(result) = crate::native_dock::NativeDock::draw_slider(l)
        {
            return result;
        }
        if msg == WM_DRAWITEM {
            crate::native_dock::NativeDock::draw(
                &*(l.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT),
            );
            return LRESULT(1);
        }
        APP.with(|a| {
            let Ok(mut cell) = a.try_borrow_mut() else {
                return DefWindowProcW(hwnd, msg, w, l);
            };
            let Some(app) = cell.as_mut() else {
                return DefWindowProcW(hwnd, msg, w, l);
            };
            match msg {
                value if value == WM_APP + 12 && app.smoke => {
                    if let Some((parent, _)) = &app.smoke_hover {
                        let mut alpha = vec![0; parent.alpha.len()];
                        alpha[(app.pointer.1 * parent.width + app.pointer.0) as usize] = 255;
                        app.candidates
                            .insert(Mask::new(parent.width, parent.height, alpha).unwrap());
                        app.update_hover();
                    }
                }
                value if value == WM_APP + 11 && app.smoke => {
                    let mode = match app.settings.quality_mode {
                        crate::settings::QualityMode::Fast => 0,
                        crate::settings::QualityMode::Balanced => 1,
                        crate::settings::QualityMode::Quality => 2,
                    };
                    if app.brush != (w.0 != 0)
                        || mode != l.0
                        || app.selected.is_some()
                        || !app.points.is_empty()
                    {
                        eprintln!("FAIL: native toolbar state or image-selection isolation");
                        app.smoke_done = false;
                        app.quit = true;
                    } else {
                        println!("PASS: native toolbar action and image-selection isolation");
                    }
                }
                WM_COMMAND => {
                    let id = w.0 & 0xffff;
                    if id == 805 {
                        app.redo();
                    }
                    if (900..906).contains(&id)
                        && let Some((action, _)) = app.dock().buttons().get(id - 900)
                    {
                        app.dock_action(*action);
                    }
                }
                WM_HSCROLL => {
                    let source = HWND(l.0 as *mut _);
                    let code = w.0 & 0xffff;
                    for (opacity, actual) in app.controls.slider_values(source) {
                        let previous = if opacity {
                            app.brush_alpha as i32
                        } else {
                            app.brush_radius.round() as i32
                        };
                        if source.0.is_null() && actual == previous {
                            continue;
                        }
                        let value = if code == 4 || code == 5 {
                            ((w.0 >> 16) & 0xffff) as i32
                        } else {
                            actual
                        };
                        if opacity {
                            app.brush_alpha = value.clamp(0, 255) as u8;
                        } else {
                            app.brush_radius = value.clamp(1, 64) as f32;
                        }
                        app.controls.set_slider_value(opacity, value);
                    }
                    app.controls.value_names(app.brush_radius, app.brush_alpha);
                    let _ = InvalidateRect(Some(app.hwnd), None, false);
                }
                WM_HOTKEY => app.activate(),
                WM_PAINT => app.paint(),
                WM_MOUSEMOVE if !app.smoke => {
                    let previous = app.screen_pointer;
                    app.set_pointer((
                        (l.0 as i16).max(0) as u32,
                        ((l.0 >> 16) as i16).max(0) as u32,
                    ));
                    if let Some((start, origin)) = app.dock_drag {
                        app.dock_anchor = Some((
                            origin.0 + app.screen_pointer.0 as i32 - start.0 as i32,
                            origin.1 + app.screen_pointer.1 as i32 - start.1 as i32,
                        ));
                        app.invalidate();
                    } else if app.dragging {
                        app.stroke.push(app.pointer);
                        app.invalidate();
                    } else {
                        app.update_hover();
                        if app.brush {
                            app.invalidate_pointer(previous);
                        }
                    }
                }
                WM_CANCELMODE | WM_CAPTURECHANGED => {
                    app.dragging = false;
                    app.pressed_hover = None;
                    app.dock_pressed = None;
                    app.dock_drag = None;
                    app.stroke.clear();
                    if msg == WM_CANCELMODE {
                        let _ = ReleaseCapture();
                    }
                    app.invalidate();
                }
                WM_LBUTTONDOWN => {
                    app.set_pointer((
                        (l.0 as i16).max(0) as u32,
                        ((l.0 >> 16) as i16).max(0) as u32,
                    ));
                    let dock = app.dock();
                    if dock.contains(app.screen_pointer.0 as i32, app.screen_pointer.1 as i32) {
                        app.dock_pressed =
                            dock.hit(app.screen_pointer.0 as i32, app.screen_pointer.1 as i32);
                        app.dragging = false;
                        if app.dock_pressed.is_none() {
                            app.dock_drag = Some((app.screen_pointer, (dock.left, dock.top)));
                        }
                    } else {
                        app.dragging = true;
                        app.stroke = vec![app.pointer];
                        app.pressed_hover = app
                            .presented_hover
                            .as_ref()
                            .filter(|mask| mask.contains(app.pointer.0, app.pointer.1))
                            .cloned();
                    }
                    SetCapture(hwnd);
                }
                WM_LBUTTONUP => {
                    app.set_pointer((
                        (l.0 as i16).max(0) as u32,
                        ((l.0 >> 16) as i16).max(0) as u32,
                    ));
                    let _ = ReleaseCapture();
                    app.dock_drag = None;
                    if let Some(action) = app.dock_pressed.take() {
                        if app
                            .dock()
                            .hit(app.screen_pointer.0 as i32, app.screen_pointer.1 as i32)
                            == Some(action)
                        {
                            app.dock_action(action);
                        }
                    } else if app.dragging {
                        app.dragging = false;
                        app.select();
                    }
                    app.pressed_hover = None;
                }
                WM_MOUSEWHEEL if GetKeyState(0x11) < 0 => {
                    app.zoom(if (w.0 >> 16) as i16 > 0 {
                        app.view.scale * 2.
                    } else {
                        app.view.scale / 2.
                    });
                }
                WM_MOUSEWHEEL => {
                    app.cycle = app.cycle.wrapping_add(1);
                    app.update_hover();
                }
                WM_KEYDOWN => match w.0 as u16 {
                    0x70 => app.set_refinement(false),
                    0x71 => app.set_refinement(true),
                    0x72 => app.set_quality(),
                    0x75 => app.next_object(),
                    0x56 | 0x4c => app.dock_action(crate::dock::Action::Select),
                    0x53 if GetKeyState(0x11) < 0 => app.save_png(),
                    0x1b => app.escape(),
                    0x49 => app.inspect(),
                    0x4d => {
                        app.lens = !app.lens;
                        app.invalidate();
                    }
                    0x59 if GetKeyState(0x11) < 0 => app.redo(),
                    0x41 => {
                        app.brush = true;
                        app.smart = false;
                        app.edge_tool = false;
                        app.brush_alpha = 255;
                        app.brush_soft = false;
                        app.invalidate();
                    }
                    0x45 => {
                        app.brush = true;
                        app.smart = false;
                        app.edge_tool = false;
                        app.brush_alpha = 0;
                        app.brush_soft = false;
                        app.invalidate();
                    }
                    0x09 => {
                        app.cycle = app.cycle.wrapping_add(1);
                        app.update_hover();
                    }
                    0x0d => app.copy(),
                    0xbb => app.zoom(app.view.scale * 2.),
                    0xbd => app.zoom(app.view.scale / 2.),
                    0x30 => app.zoom(1.),
                    0xdb => {
                        app.brush_radius = (app.brush_radius - 1.).max(0.5);
                        app.invalidate();
                    }
                    0xdd => {
                        app.brush_radius = (app.brush_radius + 1.).min(64.);
                        app.invalidate();
                    }
                    0x42 => {
                        app.brush = !app.brush;
                        app.status = if app.brush {
                            "Brush: paint to add Â· Alt to remove"
                        } else {
                            "Hover to preview Â· Click to select Â· Drag to refine"
                        }
                        .into();
                        app.invalidate();
                    }
                    0x52 => {
                        app.export_rx = None;
                        app.selected = None;
                        app.editor = None;
                        app.inspection = None;
                        app.points.clear();
                        app.history.clear();
                        app.busy = false;
                        app.copy_on_result = false;
                        app.revise();
                        app.status = "Selection reset".into();
                        app.update_hover();
                        app.invalidate();
                    }
                    0x5a if GetKeyState(0x11) < 0 => app.undo(),
                    _ => {}
                },
                WM_CLOSE => app.dismiss(),
                WM_DISPLAYCHANGE => {
                    app.dismiss();
                    app.status = "Display changed. Press the shortcut to capture again.".into();
                }
                value if value == WM_APP + 1 => {
                    if l.0 as u32 == WM_LBUTTONDBLCLK {
                        app.activate();
                    } else if l.0 as u32 == WM_RBUTTONUP {
                        app.menu();
                    }
                }
                _ => {}
            }
            LRESULT(0)
        })
    }
}

impl App {
    fn menu(&mut self) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            let _ = AppendMenuW(menu, MF_STRING, 1, w!("Snip an object"));
            let _ = AppendMenuW(
                menu,
                MF_STRING
                    | if self.settings.cpu_only {
                        MF_CHECKED
                    } else {
                        MF_UNCHECKED
                    },
                2,
                w!("Use CPU only"),
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            for (id, key) in [
                (3, Hotkey::CtrlShiftO),
                (4, Hotkey::CtrlAltO),
                (5, Hotkey::CtrlShiftSpace),
            ] {
                let text: Vec<u16> = format!("Shortcut: {}\0", key.label())
                    .encode_utf16()
                    .collect();
                let _ = AppendMenuW(
                    menu,
                    MF_STRING
                        | if self.settings.hotkey == key {
                            MF_CHECKED
                        } else {
                            MF_UNCHECKED
                        },
                    id,
                    PCWSTR(text.as_ptr()),
                );
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, 6, w!("Exit ObjectLift"));
            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);
            let _ = SetForegroundWindow(self.hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                Some(0),
                self.hwnd,
                None,
            )
            .0;
            let _ = DestroyMenu(menu);
            match command {
                1 => self.activate(),
                2 => {
                    self.dismiss();
                    self.quit = false;
                    self.settings.cpu_only = !self.settings.cpu_only;
                    self.worker = Worker::spawn(self.models.clone(), !self.settings.cpu_only);
                    self.loading = true;
                    self.model_error = None;
                }
                3..=5 => {
                    let key = match command {
                        3 => Hotkey::CtrlShiftO,
                        4 => Hotkey::CtrlAltO,
                        _ => Hotkey::CtrlShiftSpace,
                    };
                    // Reserve the new binding before releasing the old binding.
                    if key != self.settings.hotkey {
                        let new_id = if self.hotkey_id == 1 { 2 } else { 1 };
                        match RegisterHotKey(
                            Some(self.hwnd),
                            new_id,
                            HOT_KEY_MODIFIERS(key.modifier_bits()) | MOD_NOREPEAT,
                            key.key(),
                        ) {
                            Ok(()) => {
                                let _ = UnregisterHotKey(Some(self.hwnd), self.hotkey_id);
                                self.hotkey_id = new_id;
                                self.settings.hotkey = key;
                            }
                            Err(_) => {
                                MessageBoxW(
                                    Some(self.hwnd),
                                    w!(
                                        "That shortcut is already in use. Your current shortcut is unchanged."
                                    ),
                                    w!("ObjectLift"),
                                    MB_OK | MB_ICONINFORMATION,
                                );
                            }
                        }
                    }
                }
                6 => {
                    self.dismiss();
                    self.quit = true;
                }
                _ => {}
            }
            if let Err(e) = if self.smoke {
                Ok(())
            } else {
                self.settings.save(&Settings::path())
            } {
                eprintln!("Could not save settings: {e}");
            }
        }
    }
    fn invalidate_pointer(&self, previous: (u32, u32)) {
        unsafe {
            let r = (self.brush_radius * self.view.scale) as i32 + 4;
            let rect = RECT {
                left: (previous.0.min(self.screen_pointer.0) as i32 - r).max(0),
                top: (previous.1.min(self.screen_pointer.1) as i32 - r).max(0),
                right: self.screen_pointer.0.max(previous.0) as i32 + r,
                bottom: self.screen_pointer.1.max(previous.1) as i32 + r,
            };
            let _ = InvalidateRect(
                Some(self.hwnd),
                if self.lens { None } else { Some(&rect) },
                false,
            );
        }
    }
    fn update_monitor(&mut self) {
        if self.fixture.is_some() {
            return;
        }
        if let Some(d) = &self.desktop {
            unsafe {
                let point = POINT {
                    x: d.left + self.screen_pointer.0 as i32,
                    y: d.top + self.screen_pointer.1 as i32,
                };
                let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut info).as_bool() {
                    let x = (info.rcWork.left - d.left).max(0);
                    let y = (info.rcWork.top - d.top).max(0);
                    let right = (info.rcWork.right - d.left).min(d.frame.width as i32);
                    let bottom = (info.rcWork.bottom - d.top).min(d.frame.height as i32);
                    if right > x && bottom > y {
                        self.viewport = Some((x, y, (right - x) as u32, (bottom - y) as u32));
                    }
                }
                let mut x = 96;
                let mut y = 96;
                let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
                self.monitor_dpi = x;
            }
        }
    }
    fn set_pointer(&mut self, screen: (u32, u32)) {
        self.screen_pointer = screen;
        let p = self.view.image_point((screen.0 as f32, screen.1 as f32));
        self.pointer = (p.0.max(0.) as u32, p.1.max(0.) as u32);
        if let Some(d) = &self.desktop {
            self.pointer.0 = self.pointer.0.min(d.frame.width - 1);
            self.pointer.1 = self.pointer.1.min(d.frame.height - 1);
        }
    }
    fn invalidate(&mut self) {
        if self.desktop.is_some() {
            if self.brush && self.selected.is_some() && self.dock_anchor.is_none() {
                let dock = self.dock();
                self.dock_anchor = Some((dock.left, dock.top));
            }
            let dock = self.dock();
            self.controls.sync(
                self.hwnd,
                &dock,
                self.selected
                    .as_ref()
                    .is_some_and(|m| m.alpha.iter().any(|&a| a > 0)),
                self.busy,
                (
                    self.editor.as_ref().is_some_and(|e| e.can_undo()),
                    self.editor.as_ref().is_some_and(|e| e.can_redo()),
                    if self.brush {
                        Some(if self.edge_tool {
                            crate::dock::Action::Edge
                        } else if self.brush_alpha == 0 {
                            crate::dock::Action::Remove
                        } else {
                            crate::dock::Action::Add
                        })
                    } else {
                        None
                    },
                ),
                (self.brush_radius, self.brush_alpha, self.brush_soft),
            );
            self.controls
                .value_names(self.brush_radius, self.brush_alpha);
        }
        unsafe {
            // Native child controls can request owner painting synchronously while
            // APP is borrowed by this update. Queue a fresh child paint after the
            // mutation returns so labels and custom sliders reflect current state.
            let _ = RedrawWindow(
                Some(self.hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN,
            );
        }
    }
    fn activate(&mut self) {
        if self.capture_rx.is_some() {
            self.dismiss();
            return;
        }
        if self.desktop.is_some() {
            self.dismiss();
            return;
        }
        self.started = Instant::now();
        self.session.begin();
        self.candidates = Candidates::default();
        self.selected = None;
        self.dock_anchor = None;
        self.viewport = None;
        self.editor = None;
        self.inspection = None;
        self.view = crate::editor::View::default();
        self.hover = None;
        self.presented_hover = None;
        self.pressed_hover = None;
        self.hover_key = 0;
        self.points.clear();
        self.history.clear();
        self.cycle = 0;
        self.brush = false;
        self.busy = false;
        self.copy_on_result = false;
        let (tx, rx) = mpsc::channel();
        self.capture_rx = Some(rx);
        let fixture = self.fixture.clone();
        std::thread::spawn(move || {
            let result = fixture
                .map(|frame| {
                    Ok(Desktop {
                        frame,
                        left: 0,
                        top: 0,
                    })
                })
                .unwrap_or_else(|| platform::capture().map_err(|e| format!("{e:#}")));
            let _ = tx.send(result);
        });
    }
    fn dismiss(&mut self) {
        self.dock_pressed = None;
        self.dock_drag = None;
        unsafe {
            let _ = ReleaseCapture();
        }
        self.session.cancel();
        self.worker.cancel();
        self.capture_rx = None;
        self.export_rx = None;
        self.desktop = None;
        self.selected = None;
        self.dock_anchor = None;
        self.viewport = None;
        self.editor = None;
        self.inspection = None;
        self.view = crate::editor::View::default();
        self.hover = None;
        self.presented_hover = None;
        self.pressed_hover = None;
        self.candidates = Candidates::default();
        self.canvas.clear();
        self.presenter.reset();
        self.gdi_presenter.reset();
        self.transition = None;
        self.stroke.clear();
        self.history.clear();
        self.points.clear();
        self.dragging = false;
        self.busy = false;
        self.copy_on_result = false;
        unsafe {
            let _ = ReleaseCapture();
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        if self.once {
            self.quit = true;
        }
    }
    fn poll(&mut self) {
        if self
            .save_notice_until
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.save_notice_until = None;
            if self.status == "Saved cutout" {
                self.status = "Selected".into();
                self.invalidate();
            }
        }

        if let Some(start) = self.transition {
            if start.elapsed().as_millis() >= if self.selected.is_some() { 160 } else { 120 } {
                self.transition = None;
            }
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
        }

        // UIA RangeValue can update a native trackbar without WM_HSCROLL.
        // Reconcile visible controls before handling the next input/model result.
        if self.brush && !self.smoke {
            let mut changed = false;
            for (opacity, value) in self.controls.slider_values(HWND::default()) {
                if opacity && self.brush_alpha != value as u8 {
                    self.brush_alpha = value.clamp(0, 255) as u8;
                    changed = true;
                } else if !opacity && self.brush_radius.round() as i32 != value {
                    self.brush_radius = value.clamp(1, 64) as f32;
                    changed = true;
                }
            }
            if changed {
                self.controls
                    .value_names(self.brush_radius, self.brush_alpha);
                unsafe {
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                }
            }
        }
        if let Some((id, rev, result)) = self.export_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.export_rx = None;
            if self.session.accepts_result(id, rev) {
                self.busy = false;
                match result
                    .map_err(anyhow::Error::msg)
                    .and_then(|prepared| platform::publish(prepared, self.hwnd))
                {
                    Ok(()) => {
                        eprintln!("Active selection exported successfully");
                        self.dismiss();
                    }
                    Err(e) => {
                        self.status =
                            format!("Export failed: {e}. Enter to copy again or Esc to cancel.");
                        self.invalidate();
                    }
                }
            }
        }
        if let Some(job) = &mut self.file_job {
            use crate::file_export::FileProgress;
            match job.poll(&self.session) {
                FileProgress::Pending => {}
                FileProgress::Publishing => {
                    self.status = "Finishing save...".into();
                    self.invalidate();
                }
                FileProgress::Finished(result) => {
                    self.file_job = None;
                    if let Some(result) = result {
                        self.busy = false;
                        self.status = match result {
                            Ok(()) => {
                                self.save_notice_until =
                                    Some(Instant::now() + Duration::from_secs(2));
                                "Saved cutout".into()
                            }

                            Err(error) => format!("Save failed: {error}"),
                        };
                        self.invalidate();
                    }
                }
            }
        }
        if let Some(result) = self.capture_rx.as_ref().and_then(|r| r.try_recv().ok()) {
            self.capture_rx = None;
            match result {
                Ok(d) => {
                    self.status = self.model_error.clone().unwrap_or_else(|| {
                        if self.loading {
                            "Loading local modelsâ€¦"
                        } else {
                            "Discovering objects Â· You can click or draw now"
                        }
                        .into()
                    });
                    let mut p = POINT::default();
                    unsafe {
                        let _ = GetCursorPos(&mut p);
                    }
                    self.pointer = ((p.x - d.left).max(0) as u32, (p.y - d.top).max(0) as u32);
                    self.worker.start(
                        self.session.id(),
                        Arc::new(d.frame.clone()),
                        (self.pointer.0 as f32, self.pointer.1 as f32),
                    );
                    unsafe {
                        let _ = SetWindowPos(
                            self.hwnd,
                            Some(HWND_TOPMOST),
                            d.left,
                            d.top,
                            d.frame.width as i32,
                            d.frame.height as i32,
                            SWP_SHOWWINDOW,
                        );
                        let _ = SetForegroundWindow(self.hwnd);
                        let _ = SetFocus(Some(self.hwnd));
                    }
                    self.desktop = Some(d);
                    self.screen_pointer = self.pointer;
                    self.update_monitor();
                    self.rebuild();
                    self.invalidate();
                    eprintln!(
                        "capture_to_overlay_ms={}",
                        self.started.elapsed().as_millis()
                    );
                }
                Err(e) => {
                    self.session.cancel();
                    eprintln!("Capture failed: {e}");
                    unsafe {
                        let text: Vec<u16> =
                            format!("Capture failed: {e}\0").encode_utf16().collect();
                        MessageBoxW(
                            Some(self.hwnd),
                            PCWSTR(text.as_ptr()),
                            w!("ObjectLift"),
                            MB_OK | MB_ICONERROR,
                        );
                    }
                    if self.once {
                        self.quit = true;
                    }
                }
            }
        }
        while let Ok(event) = self.worker.events.try_recv() {
            match event {
                Event::Ready(provider) => {
                    self.loading = false;
                    self.provider = provider;
                }
                Event::Status(id, s) if self.session.accepts(id) => {
                    self.status = s;
                    self.invalidate();
                }
                Event::Candidate(id, mask) if self.session.accepts(id) => {
                    self.candidates.insert(mask);
                    // Keep a visible preview stable while discovery adds smaller parts.
                    if self.hover.is_none() {
                        self.update_hover();
                    }
                    if !self.busy && self.selected.is_none() {
                        self.status = format!(
                            "{} objects Â· Click to select or press F6",
                            self.candidates.len()
                        );
                    }
                    self.invalidate();
                }
                Event::Selection(id, rev, mask) => self.receive_selection(id, rev, mask),
                Event::Error(id, revision, s)
                    if id == 0
                        || (self.session.accepts(id)
                            && revision.is_none_or(|rev| self.session.accepts_result(id, rev))) =>
                {
                    self.busy = false;
                    self.copy_on_result = false;
                    if id == 0 {
                        self.loading = false;
                        self.model_error = Some(s.clone());
                    }
                    self.status = s.clone();
                    eprintln!("{s}");
                    self.invalidate();
                }
                Event::Finished(id)
                    if self.session.accepts(id) && !self.busy && self.selected.is_none() =>
                {
                    self.status = format!(
                        "{} objects Â· F6 to browse Â· Draw to find a missed object",
                        self.candidates.len()
                    );
                    self.invalidate();
                }
                _ => {}
            }
        }
        if self.smoke {
            self.smoke_tick();
        }
    }
    fn smoke_shot(&self, name: &str) -> Result<()> {
        let desktop = self.desktop.as_ref().context("Overlay disappeared")?;
        let capture = platform::capture_gdi()?;
        let image = image::RgbaImage::from_raw(
            capture.frame.width,
            capture.frame.height,
            capture.frame.rgba,
        )
        .context("Invalid capture")?;
        let crop = image::imageops::crop_imm(
            &image,
            (desktop.left - capture.left) as u32,
            (desktop.top - capture.top) as u32,
            desktop.frame.width,
            desktop.frame.height,
        )
        .to_image();
        std::fs::write(
            name,
            platform::png(
                &Frame::new(crop.width(), crop.height(), crop.into_raw())
                    .map_err(anyhow::Error::msg)?,
            )?,
        )?;
        Ok(())
    }
    fn smoke_click(&self) {
        unsafe {
            let p =
                LPARAM(self.screen_pointer.0 as isize | ((self.screen_pointer.1 as isize) << 16));
            let _ = PostMessageW(Some(self.hwnd), WM_LBUTTONDOWN, WPARAM(0), p);
            if self.smoke_phase == 0 {
                let _ = PostMessageW(Some(self.hwnd), WM_APP + 12, WPARAM(0), LPARAM(0));
            }
            let _ = PostMessageW(Some(self.hwnd), WM_LBUTTONUP, WPARAM(0), p);
        }
    }
    fn smoke_tick(&mut self) {
        if self.started.elapsed() > Duration::from_secs(90) {
            eprintln!("FAIL: precision smoke timeout: {}", self.status);
            self.smoke_done = false;
            self.dismiss();
            return;
        }
        if self.smoke_phase != 10 && self.smoke_at.elapsed() < Duration::from_millis(200) {
            return;
        }
        use crate::dock::Action;
        let mut next = false;
        match self.smoke_phase {
            0 if !self.candidates.is_empty() && self.desktop.is_some() => {
                let d = self.desktop.as_ref().unwrap();
                self.set_pointer((d.frame.width / 2, d.frame.height / 2));
                self.update_hover();
                if self
                    .hover
                    .as_ref()
                    .zip(self.presented_hover.as_ref())
                    .is_some_and(|(pending, visible)| {
                        Arc::ptr_eq(pending, visible) && visible.area() > 1
                    })
                {
                    self.smoke_hover = self
                        .presented_hover
                        .clone()
                        .map(|mask| (mask, self.session.revision()));
                    self.smoke_click();
                    next = true;
                }
            }
            1 if !self.busy && self.editor.is_some() => {
                // Accepted worker results can arrive in this same poll iteration.
                // Give queued parent/child paints a full turn before recording.
                if !self.smoke_settled {
                    self.smoke_settled = true;
                    self.smoke_at = Instant::now();
                    return;
                }

                if let Some((parent, old_revision)) = &self.smoke_hover {
                    // A result from the superseded revision must not replace the locked mask.
                    let mut alpha = vec![0; parent.alpha.len()];
                    alpha[(self.pointer.1 * parent.width + self.pointer.0) as usize] = 255;
                    let nested = Mask::new(parent.width, parent.height, alpha).unwrap();
                    self.receive_selection(self.session.id(), *old_revision, nested);
                }
                if self
                    .selected
                    .as_ref()
                    .zip(self.smoke_hover.as_ref())
                    .is_none_or(|(selected, (hovered, _))| selected.alpha != hovered.alpha)
                {
                    eprintln!(
                        "FAIL: click changed the hovered mask after a nested candidate arrived"
                    );
                    self.dismiss();
                    return;
                }
                println!(
                    "PASS: click preserves hover mask through late nested discovery and stale model result"
                );
                if let Err(e) = self.smoke_shot("artifacts/precision-selected.png") {
                    eprintln!("FAIL: {e}");
                    self.dismiss();
                    return;
                }
                self.controls.invoke(Action::Refine, &self.dock());
                next = true;
            }
            2 if self.brush => {
                self.brush_alpha = 96;
                self.brush_soft = true;
                self.brush_radius = 0.5;
                self.invalidate();
                self.smoke_click();
                next = true;
            }
            3 if self.editor.as_ref().is_some_and(|e| {
                e.mask().alpha[(self.pointer.1 * self.desktop.as_ref().unwrap().frame.width
                    + self.pointer.0) as usize]
                    == 96
            }) =>
            {
                let _ = self.smoke_shot("artifacts/precision-refine.png");
                self.controls.invoke(Action::Done, &self.dock());
                next = true;
            }
            4 if !self.brush => {
                self.undo();
                next = true;
            }
            5 => {
                unsafe {
                    let _ = PostMessageW(Some(self.hwnd), WM_COMMAND, WPARAM(805), LPARAM(0));
                }
                next = true;
            }
            6 => {
                let i = (self.pointer.1 * self.desktop.as_ref().unwrap().frame.width
                    + self.pointer.0) as usize;
                if self.editor.as_ref().unwrap().mask().alpha[i] != 96 {
                    eprintln!("FAIL: native undo/redo did not restore exact opacity");
                    self.dismiss();
                    return;
                }
                self.controls.invoke(Action::Inspect, &self.dock());
                next = true;
            }
            7 if self.inspection.is_some() => {
                let _ = self.smoke_shot("artifacts/precision-inspect-checker.png");
                self.controls.invoke(Action::Inspect, &self.dock());
                next = true;
            }
            8 => {
                let _ = self.smoke_shot("artifacts/precision-inspect-dark.png");
                self.controls.invoke(Action::Inspect, &self.dock());
                next = true;
            }
            9 => {
                let _ = self.smoke_shot("artifacts/precision-inspect-light.png");
                self.smoke_expected = self
                    .editor
                    .as_mut()
                    .and_then(|e| e.ready().ok().map(|r| r.image.as_ref().clone()));
                let _ = std::fs::remove_file("artifacts/precision-native-export.png");
                self.export(ExportDestination::File {
                    path: PathBuf::from("artifacts/precision-native-export.png"),
                    format: crate::file_export::Format::Png,
                    replace: false,
                });
                next = true;
            }
            10..=12 if self.file_job.is_none() => {
                use crate::file_export::Format;
                let decode = |path| image::open(path).map(|img| img.to_rgba8()).ok();
                let actual = match self.smoke_phase {
                    10 => decode("artifacts/precision-native-export.png"),
                    11 => decode("artifacts/professional-native-export.webp"),
                    _ => (|| {
                        use std::io::Read;
                        let file =
                            std::fs::File::open("artifacts/professional-native-export.zip").ok()?;
                        let mut archive = zip::ZipArchive::new(file).ok()?;
                        let mut bytes = Vec::new();
                        archive
                            .by_name("cutout.png")
                            .ok()?
                            .read_to_end(&mut bytes)
                            .ok()?;
                        Some(image::load_from_memory(&bytes).ok()?.to_rgba8())
                    })(),
                };
                let passed = actual
                    .as_ref()
                    .zip(self.smoke_expected.as_ref())
                    .is_some_and(|(actual, expected)| {
                        actual.width() == expected.width
                            && actual.height() == expected.height
                            && actual
                                .as_raw()
                                .chunks_exact(4)
                                .zip(expected.rgba.chunks_exact(4))
                                .all(|(actual, expected)| {
                                    actual[3] == expected[3]
                                        && ((self.smoke_phase == 11 && expected[3] == 0)
                                            || actual[..3] == expected[..3])
                                })
                    });
                let label =
                    ["PNG", "WebP", "source and mask ZIP"][(self.smoke_phase - 10) as usize];
                println!(
                    "{}: native {label} export pixel agreement",
                    if passed { "PASS" } else { "FAIL" }
                );
                if !passed || self.smoke_phase == 12 {
                    self.smoke_done = passed;
                    self.dismiss();
                } else {
                    let (path, format) = if self.smoke_phase == 10 {
                        ("artifacts/professional-native-export.webp", Format::WebP)
                    } else {
                        ("artifacts/professional-native-export.zip", Format::Bundle)
                    };
                    let _ = std::fs::remove_file(path);
                    self.export(ExportDestination::File {
                        path: PathBuf::from(path),
                        format,
                        replace: false,
                    });
                    next = true;
                }
            }
            _ => {}
        }
        if next {
            self.smoke_phase += 1;
            self.smoke_at = Instant::now();
        }
    }
    fn update_hover(&mut self) {
        if self
            .dock()
            .contains(self.screen_pointer.0 as i32, self.screen_pointer.1 as i32)
            && self.selected.is_none()
            && !self.dragging
        {
            return;
        }
        if self.selected.is_some() || self.dragging {
            return;
        }
        let candidate = self
            .candidates
            .at(self.pointer.0, self.pointer.1, self.cycle);
        let key = candidate.map(|m| m.alpha.as_ptr() as usize).unwrap_or(0);
        if key != self.hover_key {
            self.hover = candidate.cloned().map(Arc::new);
            self.hover_key = key;
            self.rebuild();
            self.invalidate();
        }
    }
    fn select(&mut self) {
        if self.inspection.is_some() && !self.brush {
            return;
        }
        let previous_points = self.points.clone();
        if !self.brush {
            self.update_monitor();
        }
        if self.export_rx.is_some() {
            return;
        }
        if self.desktop.is_none() {
            return;
        }
        if self.model_error.is_some() {
            self.status = self.model_error.clone().unwrap();
            self.invalidate();
            return;
        }
        if self.edge_tool && self.selected.is_some() {
            let stroke: Vec<_> = self
                .stroke
                .iter()
                .map(|&(x, y)| (x as f32, y as f32))
                .collect();
            let rev = self.revise();
            self.worker.prompt_with_hint(
                self.session.id(),
                rev,
                self.points.clone(),
                self.settings.quality_mode,
                crate::worker::Hint {
                    edge: Some((self.selected.clone().unwrap(), stroke, self.brush_radius)),
                    ..Default::default()
                },
            );
            self.busy = true;
            self.copy_on_result = false;
            self.stroke.clear();
            self.status = "Refining this edge Â· exact edits remain protected".into();
            self.invalidate();
            return;
        }
        let alt = unsafe { GetKeyState(0x12) < 0 };
        let shift = unsafe { GetKeyState(0x10) < 0 };
        let moved = self
            .stroke
            .first()
            .zip(self.stroke.last())
            .map(|(a, b)| a.0.abs_diff(b.0) + a.1.abs_diff(b.1) > 8)
            .unwrap_or(false)
            || self.stroke.len() > 12;
        if self.brush && !self.smart && self.selected.is_some() && !self.edge_tool {
            self.revise();
            self.export_rx = None;
            self.copy_on_result = false;
            self.busy = false;
            let points: Vec<_> = self
                .stroke
                .iter()
                .map(|&(x, y)| (x as f32, y as f32))
                .collect();
            let target = if alt { 0 } else { self.brush_alpha };
            if let Some(editor) = &mut self.editor {
                if let Err(e) = editor.paint(&points, self.brush_radius, target) {
                    self.status = e;
                } else {
                    self.selected = Some(editor.mask());
                    self.status = "Adjusted Â· Undo available".into();
                }
            }
            self.stroke.clear();
            self.rebuild();
            self.invalidate();
            return;
        }
        let fresh_selection = !self.brush && !alt && !shift;
        if fresh_selection
            && !moved
            && self.selected.as_ref().is_some_and(|m| {
                m.alpha
                    .get((self.pointer.1 * m.width + self.pointer.0) as usize)
                    .is_some_and(|&a| a > 0)
            })
        {
            return;
        }
        if fresh_selection {
            self.points.clear();
        }
        if !moved && !self.brush && !alt && !shift {
            let pressed = self.stroke.first().copied().unwrap_or(self.pointer);
            let candidate = if let Some(preview) = self.pressed_hover.take() {
                self.hover = None;
                self.presented_hover = None;
                Some(Arc::unwrap_or_clone(preview))
            } else if self.selected.is_some() {
                // An explicit click outside the active selection can choose a neighbor.
                self.candidates
                    .at(pressed.0, pressed.1, self.cycle)
                    .cloned()
            } else {
                None
            };
            if let Some(mask) = candidate {
                self.lock_candidate(mask);
                if self.settings.quick_copy {
                    self.copy();
                }
                return;
            }
            self.points = vec![(self.pointer.0 as f32, self.pointer.1 as f32, true)];
            self.copy_on_result = self.settings.quick_copy;
        } else {
            self.copy_on_result = false;
            if let Some(mask) = self.selected.as_ref().or(self.presented_hover.as_deref()) {
                self.history.push((mask.clone(), self.points.clone()));
                if self.history.len() > 12 {
                    self.history.remove(0);
                }
            }
            if !fresh_selection
                && self.points.is_empty()
                && let Some(mask) = self.selected.as_ref().or(self.presented_hover.as_deref())
                && let Some((x, y)) = mask.interior_point().or_else(|| mask.visible_point())
            {
                self.points.push((x as f32, y as f32, true));
            }
            if moved && !self.brush {
                self.points.extend(lasso_prompts(&self.stroke));
            } else if moved {
                let step = (self.stroke.len() / 12).max(1);
                self.points.extend(
                    self.stroke
                        .iter()
                        .step_by(step)
                        .take(12)
                        .map(|&(x, y)| (x as f32, y as f32, !alt && self.brush_alpha != 0)),
                );
            } else {
                self.points.push((
                    self.pointer.0 as f32,
                    self.pointer.1 as f32,
                    !alt && (!self.brush || self.brush_alpha != 0),
                ));
            }
        }
        let mut hint = crate::worker::Hint::default();
        if let Some(d) = &self.desktop {
            if moved && !self.brush {
                let x = self.stroke.iter().map(|p| p.0).min().unwrap_or(0);
                let y = self.stroke.iter().map(|p| p.1).min().unwrap_or(0);
                let r = self
                    .stroke
                    .iter()
                    .map(|p| p.0)
                    .max()
                    .unwrap_or(x + 1)
                    .min(d.frame.width - 1);
                let b = self
                    .stroke
                    .iter()
                    .map(|p| p.1)
                    .max()
                    .unwrap_or(y + 1)
                    .min(d.frame.height - 1);
                if r > x && b > y {
                    hint.bbox = Some([x as f32, y as f32, r as f32, b as f32]);
                    hint.region = crate::roi::Region::new(
                        x.saturating_sub(16),
                        y.saturating_sub(16),
                        (r + 17).min(d.frame.width) - x.saturating_sub(16),
                        (b + 17).min(d.frame.height) - y.saturating_sub(16),
                        d.frame.width,
                        d.frame.height,
                    )
                    .ok();
                }
            } else if self.points.len() > 1 {
                hint.region = self
                    .selected
                    .as_ref()
                    .and_then(|m| crate::roi::Region::around_mask(m, 0.25));
            }
        }
        let edit_region = if self.smart && self.brush && self.brush_alpha == 0 {
            self.desktop.as_ref().and_then(|d| {
                let pad = (self.brush_radius * 2.).max(32.) as u32;
                let x = self
                    .stroke
                    .iter()
                    .map(|p| p.0)
                    .min()
                    .unwrap_or(self.pointer.0)
                    .saturating_sub(pad);
                let y = self
                    .stroke
                    .iter()
                    .map(|p| p.1)
                    .min()
                    .unwrap_or(self.pointer.1)
                    .saturating_sub(pad);
                let r = (self
                    .stroke
                    .iter()
                    .map(|p| p.0)
                    .max()
                    .unwrap_or(self.pointer.0)
                    + pad
                    + 1)
                .min(d.frame.width);
                let b = (self
                    .stroke
                    .iter()
                    .map(|p| p.1)
                    .max()
                    .unwrap_or(self.pointer.1)
                    + pad
                    + 1)
                .min(d.frame.height);
                crate::roi::Region::new(x, y, r - x, b - y, d.frame.width, d.frame.height).ok()
            })
        } else {
            None
        };
        self.stroke.clear();
        if let Some(d) = &self.desktop {
            for (x, y, _) in &mut self.points {
                *x = x.clamp(0.0, d.frame.width.saturating_sub(1) as f32);
                *y = y.clamp(0.0, d.frame.height.saturating_sub(1) as f32);
            }
        }
        if self.points.len() > 64 {
            self.points = previous_points;
            self.status = "64 correction points reached. Undo or reset before adding more.".into();
            self.invalidate();
            return;
        }
        self.busy = true;
        self.status = "Refining selectionâ€¦ Esc to cancel".into();
        let rev = self.revise();
        if fresh_selection {
            self.replace_revision = Some(rev);
        }
        if self.smart && self.brush {
            self.merge = Some(MergeIntent {
                revision: rev,
                add: self.brush_alpha != 0,
                region: edit_region,
            });
        }
        let correction_mode = if self.settings.quality_mode == crate::settings::QualityMode::Fast
            && ((self.smart && self.brush) || (moved && !self.brush))
        {
            if !self.models.join("sam21/interactive_decoder.onnx").exists() {
                self.busy = false;
                self.points = previous_points;
                self.status = "Install correction models, or use exact Add / Remove".into();
                self.invalidate();
                return;
            }
            crate::settings::QualityMode::Balanced
        } else {
            self.settings.quality_mode
        };
        self.worker.prompt_with_hint(
            self.session.id(),
            rev,
            self.points.clone(),
            correction_mode,
            hint,
        );
        self.invalidate();
    }
    fn undo(&mut self) {
        if self.editor.as_mut().is_some_and(|e| e.undo()) {
            self.revise();
            self.busy = false;
            self.copy_on_result = false;
            self.export_rx = None;
            self.selected = self.editor.as_ref().map(|e| e.mask());
            self.points.clear();
            self.status = "Undo Â· Ctrl+Y to redo".into();
            self.rebuild();
            self.invalidate();
            return;
        }
        if self.export_rx.take().is_some() {
            self.revise();
            self.busy = false;
            self.copy_on_result = false;
            self.status = "Copy cancelled Â· Enter to retry".into();
            self.invalidate();
        }
        if let Some((mask, points)) = self.history.pop() {
            self.revise();
            self.selected = Some(mask);
            self.points = points;
            self.busy = false;
            self.copy_on_result = false;
            self.rebuild();
            self.status = "Undone Â· Enter to copy".into();
            self.invalidate();
        }
    }
    fn copy(&mut self) {
        self.export(ExportDestination::Clipboard);
    }
    fn save_png(&mut self) {
        self.save_file(crate::file_export::Format::Png);
    }
    fn save_file(&mut self, format: crate::file_export::Format) {
        if self.busy
            || self.file_job.is_some()
            || self
                .selected
                .as_ref()
                .or(self.presented_hover.as_deref())
                .is_none()
            || (format.needs_source() && self.editor.is_none())
        {
            return;
        }
        match platform::choose_export_path(self.hwnd, format) {
            Ok(Some((path, replace))) => self.export(ExportDestination::File {
                path,
                format,
                replace,
            }),
            Ok(None) => {}
            Err(error) => {
                self.status = format!("Save failed: {error}");
                self.invalidate();
            }
        }
    }
    fn export(&mut self, destination: ExportDestination) {
        if self.busy
            || !self.session.accepts(self.session.id())
            || (matches!(&destination, ExportDestination::File { .. }) && self.file_job.is_some())
        {
            return;
        }
        let Some(mask) = self.selected.as_ref().or(self.presented_hover.as_deref()) else {
            return;
        };
        let Some(d) = &self.desktop else {
            return;
        };
        let (image, origin) = if let Some(editor) = &mut self.editor {
            match editor.ready() {
                Ok(ready) => (ready.image.clone(), (ready.x, ready.y)),
                Err(e) => {
                    self.status = e;
                    self.invalidate();
                    return;
                }
            }
        } else {
            match extract(&d.frame, mask) {
                Ok(image) => (Arc::new(image), (0, 0)),
                Err(e) => {
                    self.status = e;
                    self.invalidate();
                    return;
                }
            }
        };
        let id = self.session.id();
        let rev = self.revise();
        self.busy = true;
        match destination {
            ExportDestination::Clipboard => {
                let (tx, rx) = mpsc::channel();
                self.export_rx = Some(rx);
                self.status = "Preparing clipboard image… Esc to cancel".into();
                std::thread::spawn(move || {
                    let result = platform::prepare(&image).map_err(|error| error.to_string());
                    let _ = tx.send((id, rev, result));
                });
            }
            ExportDestination::File {
                path,
                format,
                replace,
            } => {
                let source = if format.needs_source() {
                    self.editor.as_ref().map(|editor| editor.source())
                } else {
                    None
                };
                self.status = format!("Saving {}... Esc to cancel", format.label());
                self.file_job = Some(crate::file_export::FileJob::start(id, rev, move || {
                    crate::file_export::stage(
                        &path,
                        replace,
                        format,
                        &image,
                        source.as_deref(),
                        origin,
                    )
                }));
            }
        }
        self.invalidate();
    }
    fn next_object(&mut self) {
        let Some(mask) = self.candidates.cycle(self.cycle).cloned() else {
            return;
        };
        self.cycle = self.cycle.wrapping_add(1);
        self.points.clear();
        self.brush = false;
        self.inspection = None;
        self.view = crate::editor::View::default();
        self.dock_anchor = None;
        if let Some((x, y)) = mask.interior_point().or_else(|| mask.visible_point()) {
            self.set_pointer((x, y));
            self.points = vec![(x as f32, y as f32, true)];
        }
        self.update_monitor();
        self.lock_candidate(mask);
    }
    fn lock_candidate(&mut self, mask: Mask) {
        self.invalidate_selection();
        self.replace_revision = None;
        self.merge = None;
        self.editor = None;
        self.dock_anchor = None;
        self.hover = None;
        self.presented_hover = None;
        self.pressed_hover = None;
        self.hover_key = 0;
        self.stroke.clear();
        self.accept_selection(mask);
        self.status = "Selected - Refine, Inspect, or Enter to copy".into();
        self.rebuild();
        self.invalidate();
    }
    fn receive_selection(&mut self, id: u64, revision: u64, mask: Mask) {
        if !self.session.accepts_result(id, revision) {
            return;
        }
        self.busy = false;
        self.accept_selection(mask);
        self.rebuild();
        self.status = "Ready - Inspect, Refine, or Copy".into();
        self.invalidate();
        if std::mem::take(&mut self.copy_on_result) {
            self.copy();
        }
    }
    fn accept_selection(&mut self, mask: Mask) {
        if self.replace_revision == Some(self.session.revision()) {
            self.editor = None;
            self.replace_revision = None;
            self.dock_anchor = None;
        }

        if let Some(editor) = &mut self.editor {
            if let Some(intent) = self
                .merge
                .take()
                .filter(|m| m.revision == self.session.revision())
            {
                let _ = editor.accept_smart_in(mask, intent.add, intent.region);
            } else {
                let _ = editor.accept(mask);
            }
        } else if let Some(d) = &self.desktop {
            self.editor = crate::editor::Editor::new(
                Arc::new(d.frame.clone()),
                mask,
                self.settings.quality_mode == crate::settings::QualityMode::Quality,
            )
            .ok();
        }
        if let Some(editor) = &mut self.editor {
            editor.set_colors(self.settings.quality_mode == crate::settings::QualityMode::Quality);
            self.selected = Some(editor.mask());
        }
    }
    fn redo(&mut self) {
        if self.editor.as_mut().is_some_and(|e| e.redo()) {
            self.revise();
            self.busy = false;
            self.copy_on_result = false;
            self.export_rx = None;
            self.selected = self.editor.as_ref().map(|e| e.mask());
            self.points.clear();
            self.status = "Redo".into();
            self.rebuild();
            self.invalidate();
        }
    }
    fn inspect(&mut self) {
        if self.editor.is_some() {
            self.inspection = Some(
                self.inspection
                    .map_or(crate::editor::Background::Checker, |b| b.next()),
            );
            self.rebuild();
            self.invalidate();
        }
    }
    fn escape(&mut self) {
        if self
            .file_job
            .as_ref()
            .is_some_and(|job| job.is_publishing(&self.session))
        {
            return;
        }
        if self.dragging {
            self.dragging = false;
            self.pressed_hover = None;
            self.stroke.clear();
            unsafe {
                let _ = ReleaseCapture();
            }
            self.invalidate();
            return;
        }
        if self.busy || self.export_rx.is_some() {
            self.invalidate_selection();
            self.status = "Cancelled Â· accepted selection preserved".into();
            self.invalidate();
            return;
        }
        if self.brush || self.inspection.is_some() {
            self.brush = false;
            self.inspection = None;
            self.status = "Selected Â· Enter to copy".into();
            self.rebuild();
            self.invalidate();
        } else {
            self.dismiss();
        }
    }
    fn rebuild(&mut self) {
        self.canvas_revision = self.canvas_revision.wrapping_add(1);
        self.transition = if (self.selected.is_some() || self.hover.is_some())
            && !self.brush
            && self.inspection.is_none()
            && !self.smoke
            && self
                .desktop
                .as_ref()
                .is_some_and(|d| u64::from(d.frame.width) * u64::from(d.frame.height) <= 4_000_000)
            && !crate::native_dock::reduced_motion()
        {
            Some(Instant::now())
        } else {
            None
        };

        if let Some(editor) = &mut self.editor {
            if let Some(bg) = self.inspection
                && let Ok(canvas) = editor.inspect(bg)
            {
                self.canvas = canvas;
                return;
            }
            let _ = editor.ready();
        }
        let Some(d) = &self.desktop else {
            return;
        };
        let mask = self.selected.as_ref().or(self.hover.as_deref());
        let mut style = if self.brush {
            crate::highlight::Appearance::REFINE
        } else if self.selected.is_some() {
            crate::highlight::Appearance::SELECTED
        } else {
            crate::highlight::Appearance::HOVER
        };
        style.color = self.preview_color;
        style.strength = if self.mask_view {
            255
        } else if self.brush {
            64
        } else {
            self.preview_strength
                .saturating_add(if self.selected.is_some() { 26 } else { 0 })
        };
        style.contour = !self.mask_view;
        if crate::native_dock::high_contrast() {
            style.contour = true;
            style.strength = 150;
            style.color = [255, 255, 255];
        }
        self.canvas = crate::highlight::render_with(&d.frame, mask, style);
        if self.transition.is_some() {
            self.transition = Some(Instant::now());
        }
    }

    fn dock(&self) -> crate::dock::Dock {
        let (width, height) = self
            .desktop
            .as_ref()
            .map_or((1280, 720), |d| (d.frame.width, d.frame.height));
        let stage = if self.selected.is_none() {
            crate::dock::Stage::Discover
        } else if self.brush {
            crate::dock::Stage::Refine
        } else {
            crate::dock::Stage::Selected
        };
        let selection = self
            .selected
            .as_ref()
            .and_then(|m| crate::roi::Region::around_visible(m, 0.))
            .map(|r| {
                let p = self.view.screen_point((r.x as f32, r.y as f32));
                (
                    p.0 as i32,
                    p.1 as i32,
                    (p.0 + r.width as f32 * self.view.scale) as i32,
                    (p.1 + r.height as f32 * self.view.scale) as i32,
                )
            });
        let (ox, oy, work_width, work_height) = self.viewport.unwrap_or((0, 0, width, height));
        let selection = selection.map(|(x, y, r, b)| (x - ox, y - oy, r - ox, b - oy));
        let mut dock = crate::dock::Dock::context(
            work_width,
            work_height,
            self.monitor_dpi as f32 / 96.,
            stage,
            selection,
        );
        dock.place(dock.left + ox, dock.top + oy);
        if self.brush && self.brush_soft {
            dock.bottom += dock.slider_step();
        }
        {
            let (x, y) = self.dock_anchor.unwrap_or((dock.left, dock.top));
            let w = dock.right - dock.left;
            let h = dock.bottom - dock.top;
            dock.place(
                x.clamp(ox + 8, (ox + work_width as i32 - w - 8).max(ox + 8)),
                y.clamp(oy + 8, (oy + work_height as i32 - h - 8).max(oy + 8)),
            );
        }
        dock
    }
    fn revise(&mut self) -> u64 {
        let rev = self.session.revise();
        self.worker.invalidate(self.session.id(), rev);
        rev
    }
    fn invalidate_selection(&mut self) {
        self.revise();
        self.busy = false;
        self.export_rx = None;
        self.copy_on_result = false;
    }
    fn recompute_selection(&mut self) {
        if let Some(editor) = &mut self.editor {
            editor.set_colors(self.settings.quality_mode == crate::settings::QualityMode::Quality);
        }
        if let Some(mask) = &self.selected {
            if self.points.is_empty()
                && let Some((x, y)) = mask.interior_point().or_else(|| mask.visible_point())
            {
                self.points = vec![(x as f32, y as f32, true)];
            }
            if !self.points.is_empty() {
                let rev = self.revise();
                self.worker.prompt(
                    self.session.id(),
                    rev,
                    self.points.clone(),
                    self.settings.quality_mode,
                );
                self.busy = true;
                self.status = "Updating quality Â· manual edits remain protected".into();
            }
        }
        self.rebuild();
        self.invalidate();
    }
    fn set_refinement(&mut self, enabled: bool) {
        if enabled && !self.models.join("sam21/interactive_decoder.onnx").exists() {
            self.status =
                "Balanced unavailable: install optional models (README). Fast remains available."
                    .into();
            self.invalidate();
            return;
        }
        self.invalidate_selection();
        self.settings.quality_mode = if enabled {
            crate::settings::QualityMode::Balanced
        } else {
            crate::settings::QualityMode::Fast
        };
        self.status = if enabled {
            "Balanced: SAM 2.1 with detail refinement"
        } else {
            "Fast: instant cached selections"
        }
        .into();
        if let Err(e) = if self.smoke {
            Ok(())
        } else {
            self.settings.save(&Settings::path())
        } {
            self.status = format!("Settings could not be saved: {e}");
        }
        self.recompute_selection();
    }
    fn set_quality(&mut self) {
        if !self.models.join("sam21/interactive_decoder.onnx").exists()
            || !self.models.join("vitmatte/model.onnx").exists()
        {
            self.status =
                "Quality unavailable: install optional models (README). Fast remains available."
                    .into();
            self.invalidate();
            return;
        }
        self.invalidate_selection();
        self.settings.quality_mode = crate::settings::QualityMode::Quality;
        self.status = "Quality: SAM 2.1 + selective alpha matting (first use loads models)".into();
        if let Err(e) = if self.smoke {
            Ok(())
        } else {
            self.settings.save(&Settings::path())
        } {
            self.status = format!("Settings could not be saved: {e}");
        }
        self.recompute_selection();
    }
    fn more(&mut self) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            if let Ok(export_menu) = CreatePopupMenu() {
                let available = !self.busy
                    && self.file_job.is_none()
                    && self
                        .selected
                        .as_ref()
                        .or(self.presented_hover.as_deref())
                        .is_some();
                for (id, label, enabled) in [
                    (801, "Transparent PNG    Ctrl+S", available),
                    (823, "Lossless WebP", available),
                    (
                        824,
                        "Source and mask bundle",
                        available && self.editor.is_some(),
                    ),
                    #[cfg(feature = "ag-psd")]
                    (
                        825,
                        "Editable PSD (experimental)",
                        available && self.editor.is_some(),
                    ),
                ] {
                    let label: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    let flags = if enabled {
                        MF_STRING
                    } else {
                        MF_STRING | MF_GRAYED
                    };
                    let _ = AppendMenuW(export_menu, flags, id, PCWSTR(label.as_ptr()));
                }
                if AppendMenuW(menu, MF_POPUP, export_menu.0 as usize, w!("Export")).is_err() {
                    let _ = DestroyMenu(export_menu);
                }
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            let append_items = |target, items: &[(usize, &str)]| {
                for &(id, label) in items {
                    let wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    let _ = AppendMenuW(target, MF_STRING, id, PCWSTR(wide.as_ptr()));
                }
            };
            append_items(
                menu,
                &[
                    (802, "Smart Add"),
                    (803, "Smart Remove"),
                    (804, "Soft opacity brush"),
                    (815, "Undo    Ctrl+Z"),
                    (805, "Redo    Ctrl+Y"),
                ],
            );
            for (label, items) in [
                (
                    w!("Quality"),
                    &[(806, "Fast"), (807, "Balanced"), (808, "Quality")][..],
                ),
                (
                    w!("View"),
                    &[
                        (810, "Zoom in"),
                        (811, "Zoom out"),
                        (812, "Reset view"),
                        (814, "Detail lens    M"),
                    ][..],
                ),
                (
                    w!("Highlight"),
                    &[
                        (816, "Lavender"),
                        (817, "Cyan"),
                        (818, "Coral"),
                        (819, "Tint strength 25%"),
                        (820, "Tint strength 35%"),
                        (821, "Tint strength 50%"),
                        (822, "Toggle solid mask coverage"),
                    ][..],
                ),
            ] {
                if let Ok(submenu) = CreatePopupMenu() {
                    append_items(submenu, items);
                    if AppendMenuW(menu, MF_POPUP, submenu.0 as usize, label).is_err() {
                        let _ = DestroyMenu(submenu);
                    }
                }
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_items(menu, &[(809, "Toggle Quick Copy"), (813, "Close")]);
            let mut point = POINT::default();
            if let Some((_, (left, _, _, bottom))) = self
                .dock()
                .buttons()
                .into_iter()
                .find(|(action, _)| *action == crate::dock::Action::More)
            {
                point = POINT {
                    x: left,
                    y: bottom + 4,
                };
                let _ = ClientToScreen(self.hwnd, &mut point);
            } else {
                let _ = GetCursorPos(&mut point);
            }
            let command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY,
                point.x,
                point.y,
                Some(0),
                self.hwnd,
                None,
            )
            .0;
            let _ = DestroyMenu(menu);
            match command {
                801 => self.save_png(),
                823 => self.save_file(crate::file_export::Format::WebP),
                824 => self.save_file(crate::file_export::Format::Bundle),
                #[cfg(feature = "ag-psd")]
                825 => self.save_file(crate::file_export::Format::Psd),
                802 | 803 => {
                    self.brush = true;
                    self.smart = true;
                    self.edge_tool = false;
                    self.brush_alpha = if command == 802 { 255 } else { 0 };
                }
                804 => {
                    self.brush = true;
                    self.smart = false;
                    self.edge_tool = false;
                    self.brush_alpha = 128;
                    self.brush_soft = true;
                }
                805 => self.redo(),
                815 => self.undo(),
                806 => self.set_refinement(false),
                807 => self.set_refinement(true),
                808 => self.set_quality(),
                809 => {
                    self.settings.quick_copy = !self.settings.quick_copy;
                    let _ = self.settings.save(&Settings::path());
                }
                810 => self.zoom(self.view.scale * 2.),
                811 => self.zoom(self.view.scale / 2.),
                812 => self.zoom(1.),
                813 => self.dismiss(),
                814 => {
                    self.lens = !self.lens;
                    self.invalidate();
                }
                816..=818 => {
                    self.preview_color = match command {
                        817 => [60, 215, 236],
                        818 => [255, 130, 145],
                        _ => [137, 151, 255],
                    };
                    self.rebuild();
                }
                819..=821 => {
                    self.preview_strength = match command {
                        819 => 64,
                        821 => 128,
                        _ => 89,
                    };
                    self.rebuild();
                }
                822 => {
                    self.mask_view = !self.mask_view;
                    self.rebuild();
                }
                _ => {}
            }
        }
    }
    fn zoom(&mut self, scale: f32) {
        if let Some(d) = &self.desktop {
            self.view.zoom_at(
                scale,
                (self.screen_pointer.0 as f32, self.screen_pointer.1 as f32),
                (d.frame.width, d.frame.height),
            );
            self.invalidate();
        }
    }
    fn dock_action(&mut self, action: crate::dock::Action) {
        let was_brush = self.brush;
        use crate::dock::Action;
        match action {
            Action::Select => {
                self.brush = false;
                self.status = "Click to select Â· Draw a loose lasso to find an object".into();
            }
            Action::Brush => {
                self.brush = true;
                self.status = "Brush: paint to add, hold Alt to remove".into();
            }
            Action::Mode => match self.settings.quality_mode {
                crate::settings::QualityMode::Fast => self.set_refinement(true),
                crate::settings::QualityMode::Balanced => self.set_quality(),
                crate::settings::QualityMode::Quality => self.set_refinement(false),
            },
            Action::Copy => self.copy(),
            Action::Save => self.save_png(),
            Action::Close => self.dismiss(),
            Action::Refine => {
                let d = self.dock();
                self.dock_anchor = Some((d.left, d.top));
                self.brush = true;
                self.smart = false;
                self.edge_tool = false;
                self.brush_alpha = 255;
                self.brush_soft = false;
            }
            Action::Inspect => self.inspect(),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Add => {
                self.brush = true;
                self.smart = false;
                self.edge_tool = false;
                self.brush_alpha = 255;
                self.brush_soft = false;
            }
            Action::Remove => {
                self.brush = true;
                self.smart = false;
                self.edge_tool = false;
                self.brush_alpha = 0;
                self.brush_soft = false;
            }
            Action::Edge => {
                self.brush = true;
                self.smart = false;
                self.edge_tool = true;
            }
            Action::Done => {
                self.brush = false;
                self.edge_tool = false;
            }
            Action::Lasso => {
                self.brush = false;
                self.status = "Draw loosely around the object".into();
            }
            Action::More => self.more(),
        }
        if was_brush != self.brush {
            self.rebuild();
        }
        self.invalidate();
    }
    fn paint(&mut self) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(self.hwnd, &mut ps);
            if let Some(d) = &self.desktop {
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: d.frame.width as i32,
                        biHeight: -(d.frame.height as i32),
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let opacity = self.transition.map_or(1., |start| {
                    crate::motion::opacity(
                        start.elapsed().as_secs_f32() * 1000.,
                        if self.selected.is_some() { 160. } else { 120. },
                        false,
                    )
                });
                let paint = crate::presentation::Paint {
                    capture: self.session.id(),
                    revision: self.canvas_revision,
                    frame: &d.frame,
                    canvas: &self.canvas,
                    view: self.view,
                    opacity,
                };
                let drawn = if std::env::var("OBJECTLIFT_RENDERER").as_deref() == Ok("legacy") {
                    false
                } else if std::env::var("OBJECTLIFT_RENDERER").as_deref() == Ok("direct2d") {
                    self.presenter.draw(self.hwnd, paint)
                } else {
                    self.gdi_presenter.draw(dc, paint)
                };
                if !drawn {
                    self.transition = None;
                    StretchDIBits(
                        dc,
                        0,
                        0,
                        d.frame.width as i32,
                        d.frame.height as i32,
                        self.view.x as i32,
                        self.view.y as i32,
                        (d.frame.width as f32 / self.view.scale) as i32,
                        (d.frame.height as f32 / self.view.scale) as i32,
                        Some(self.canvas.as_ptr().cast()),
                        &info,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                }
                let scale = self.monitor_dpi as f32 / 96.;
                let dock = self.dock();
                let left = dock.left;
                let top = dock.top;
                let rect = RECT {
                    left,
                    top,
                    right: dock.right,
                    bottom: dock.bottom,
                };
                let brush = CreateSolidBrush(crate::native_dock::color(0x00221a17, COLOR_WINDOW));
                let border = CreatePen(PS_SOLID, 1, COLORREF(0x0053433e));
                let old_brush = SelectObject(dc, brush.into());
                let old_pen = SelectObject(dc, border.into());
                let _ = RoundRect(
                    dc,
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    (40.0 * scale) as i32,
                    (40.0 * scale) as i32,
                );
                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                let _ = DeleteObject(border.into());
                let _ = DeleteObject(brush.into());
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, crate::native_dock::color(0x00f4f6f6, COLOR_WINDOWTEXT));
                let font = crate::ui_paint::font((13.0 * scale) as i32);
                let old = SelectObject(dc, font.into());
                if let Some(r) = dock.slider() {
                    let mut label: Vec<u16> = format!("Size {} px", self.brush_radius)
                        .encode_utf16()
                        .collect();
                    let mut text_rect = RECT {
                        left: dock.left + 12,
                        top: r.1,
                        right: r.0 - 4,
                        bottom: r.3,
                    };
                    DrawTextW(
                        dc,
                        &mut label,
                        &mut text_rect,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                    );
                    if self.brush_soft {
                        let mut text: Vec<u16> =
                            format!("Opacity {}%", (self.brush_alpha as u32 * 100) / 255)
                                .encode_utf16()
                                .collect();
                        text_rect.top += dock.slider_step();
                        text_rect.bottom += dock.slider_step();
                        DrawTextW(
                            dc,
                            &mut text,
                            &mut text_rect,
                            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                        );
                    }
                }
                SetTextColor(dc, crate::native_dock::color(0x00ccb9ad, COLOR_WINDOWTEXT));
                let status_visible = self.selected.is_none()
                    || self.busy
                    || self.status.starts_with("Saved ")
                    || self.status.contains("failed")
                    || self.status.contains("unavailable")
                    || self.status.contains("cancelled");
                if status_visible {
                    let label = if self.selected.is_none() && !self.loading && !self.busy {
                        "Hover an object - click to select"
                    } else {
                        &self.status
                    };
                    let mut text: Vec<u16> = label.encode_utf16().collect();
                    let last = dock.buttons().last().unwrap().1;
                    let mut tr = RECT {
                        left: left + (12. * scale) as i32,
                        top: last.3 + (6. * scale) as i32,
                        right: rect.right - (12. * scale) as i32,
                        bottom: rect.bottom - (5. * scale) as i32,
                    };
                    if self.selected.is_some() {
                        tr.top = (rect.top - (36. * scale) as i32).max(0);
                        tr.bottom = rect.top - (6. * scale) as i32;
                        let bg = CreateSolidBrush(COLORREF(0x00221a17));
                        let _ = FillRect(dc, &tr, bg);
                        let _ = DeleteObject(bg.into());
                    }
                    DrawTextW(
                        dc,
                        &mut text,
                        &mut tr,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                    );
                }
                SelectObject(dc, old);
                if self.brush {
                    let pen = CreatePen(PS_SOLID, 1, COLORREF(0x00fcb4a5));
                    let old_pen = SelectObject(dc, pen.into());
                    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                    let r = (self.brush_radius * self.view.scale).max(1.) as i32;
                    let (x, y) = (self.screen_pointer.0 as i32, self.screen_pointer.1 as i32);
                    let _ = Ellipse(dc, x - r, y - r, x + r + 1, y + r + 1);
                    SelectObject(dc, old_pen);
                    SelectObject(dc, old_brush);
                    let _ = DeleteObject(pen.into());
                    if self.lens
                        && let Some(ready) = self.editor.as_ref().and_then(|e| e.ready_ref())
                    {
                        let mut pixels = vec![0u8; 32 * 32 * 4];
                        for ly in 0..32i32 {
                            for lx in 0..32i32 {
                                let wx = self.pointer.0 as i32 + lx - 16;
                                let wy = self.pointer.1 as i32 + ly - 16;
                                let i = ((ly * 32 + lx) * 4) as usize;
                                let bg = if (lx / 4 + ly / 4) % 2 == 0 {
                                    60u32
                                } else {
                                    84
                                };
                                pixels[i..i + 4]
                                    .copy_from_slice(&[bg as u8, bg as u8, bg as u8, 255]);
                                if wx >= ready.x as i32
                                    && wy >= ready.y as i32
                                    && wx < (ready.x + ready.image.width) as i32
                                    && wy < (ready.y + ready.image.height) as i32
                                {
                                    let src = (((wy as u32 - ready.y) * ready.image.width
                                        + wx as u32
                                        - ready.x)
                                        * 4) as usize;
                                    let alpha = ready.image.rgba[src + 3] as u32;
                                    for c in 0..3 {
                                        pixels[i + c] = ((ready.image.rgba[src + 2 - c] as u32
                                            * alpha
                                            + bg * (255 - alpha)
                                            + 127)
                                            / 255)
                                            as u8;
                                    }
                                }
                            }
                        }
                        let mut lens_info = info;
                        lens_info.bmiHeader.biWidth = 32;
                        lens_info.bmiHeader.biHeight = -32;
                        let left = (x + 24).min(d.frame.width as i32 - 132).max(0);
                        let top = (y + 24).min(d.frame.height as i32 - 132).max(0);
                        StretchDIBits(
                            dc,
                            left,
                            top,
                            128,
                            128,
                            0,
                            0,
                            32,
                            32,
                            Some(pixels.as_ptr().cast()),
                            &lens_info,
                            DIB_RGB_COLORS,
                            SRCCOPY,
                        );
                    }
                }
                if self.stroke.len() > 1 {
                    let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00b9e447));
                    let old = SelectObject(dc, pen.into());
                    let points: Vec<POINT> = self
                        .stroke
                        .iter()
                        .map(|&(x, y)| POINT {
                            x: self.view.screen_point((x as f32, y as f32)).0 as i32,
                            y: self.view.screen_point((x as f32, y as f32)).1 as i32,
                        })
                        .collect();
                    let _ = Polyline(dc, &points);
                    SelectObject(dc, old);
                    let _ = DeleteObject(pen.into());
                }
            }
            let _ = EndPaint(self.hwnd, &ps);
            self.presented_hover = if self.desktop.is_some() && self.selected.is_none() {
                self.hover.clone()
            } else {
                None
            };
        }
    }
}

pub fn lasso_prompts(stroke: &[(u32, u32)]) -> Vec<(f32, f32, bool)> {
    if stroke.len() < 3 {
        return Vec::new();
    }
    let min_x = stroke.iter().map(|p| p.0).min().unwrap() as f32;
    let max_x = stroke.iter().map(|p| p.0).max().unwrap() as f32;
    let min_y = stroke.iter().map(|p| p.1).min().unwrap() as f32;
    let max_y = stroke.iter().map(|p| p.1).max().unwrap() as f32;
    let mut result = Vec::new();
    for row in 0..3 {
        for col in 0..3 {
            let x = min_x + (col as f32 + 0.5) * (max_x - min_x) / 3.0;
            let y = min_y + (row as f32 + 0.5) * (max_y - min_y) / 3.0;
            let mut inside = false;
            let mut j = stroke.len() - 1;
            for i in 0..stroke.len() {
                let (xi, yi) = (stroke[i].0 as f32, stroke[i].1 as f32);
                let (xj, yj) = (stroke[j].0 as f32, stroke[j].1 as f32);
                if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
                j = i;
            }
            if inside {
                result.push((x, y, true));
            }
        }
    }
    // A loose lasso includes background. Use its interior seed as semantic guidance,
    // rather than incorrectly forcing every sampled interior pixel into the object.
    let center = ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
    result.sort_by(|a, b| {
        ((a.0 - center.0).powi(2) + (a.1 - center.1).powi(2))
            .total_cmp(&((b.0 - center.0).powi(2) + (b.1 - center.1).powi(2)))
    });
    result.truncate(1);
    let margin = 8.0;
    for (x, y) in [
        (min_x - margin, min_y - margin),
        (max_x + margin, min_y - margin),
        (min_x - margin, max_y + margin),
        (max_x + margin, max_y + margin),
    ] {
        result.push((x.max(0.0), y.max(0.0), false));
    }
    result
}
