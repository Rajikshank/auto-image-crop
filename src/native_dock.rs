//! Native BUTTON/trackbar controls retain keyboard and MSAA/UI Automation behavior.
use crate::dock::{Action, Dock};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::Com::*,
        UI::{Accessibility::*, Controls::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, PWSTR, w},
};
pub fn high_contrast() -> bool {
    unsafe {
        let mut info = windows::Win32::UI::Accessibility::HIGHCONTRASTW {
            cbSize: std::mem::size_of::<windows::Win32::UI::Accessibility::HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        let _ = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some((&mut info as *mut _) as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        info.dwFlags.0 & 1 != 0
    }
}
pub fn reduced_motion() -> bool {
    unsafe {
        let mut enabled = windows::core::BOOL(1);
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut enabled as *mut windows::core::BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        !enabled.as_bool() || high_contrast()
    }
}
pub fn color(normal: u32, system: SYS_COLOR_INDEX) -> COLORREF {
    COLORREF(if high_contrast() {
        unsafe { GetSysColor(system) }
    } else {
        normal
    })
}
#[derive(Default)]
pub struct NativeDock {
    buttons: Vec<HWND>,
    size: Option<HWND>,
    opacity: Option<HWND>,
    tips: Option<HWND>,
    accessibility: Option<IAccPropServices>,
    com_initialized: bool,
}
impl Drop for NativeDock {
    fn drop(&mut self) {
        self.accessibility = None;
        if self.com_initialized {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
impl NativeDock {
    pub fn sync(
        &mut self,
        parent: HWND,
        dock: &Dock,
        selected: bool,
        busy: bool,
        edit: (bool, bool, Option<Action>),
        brush: (f32, u8, bool),
    ) {
        let (radius, alpha, soft) = brush;
        let (undo, redo, active) = edit;
        unsafe {
            if self.buttons.is_empty() {
                self.com_initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
                self.accessibility =
                    CoCreateInstance(&CAccPropServices, None, CLSCTX_INPROC_SERVER).ok();
                let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                    dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                    dwICC: ICC_BAR_CLASSES,
                });
                self.tips = CreateWindowExW(
                    WS_EX_TOPMOST,
                    w!("tooltips_class32"),
                    w!(""),
                    WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP),
                    0,
                    0,
                    0,
                    0,
                    Some(parent),
                    None,
                    None,
                    None,
                )
                .ok();
                for i in 0..6 {
                    if let Ok(hwnd) = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("BUTTON"),
                        w!(""),
                        WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                        0,
                        0,
                        1,
                        1,
                        Some(parent),
                        Some(HMENU((900 + i) as *mut _)),
                        None,
                        None,
                    ) {
                        if let Some(tips) = self.tips {
                            let tool = TTTOOLINFOW {
                                cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                                uFlags: TTF_IDISHWND | TTF_SUBCLASS,
                                hwnd: parent,
                                uId: hwnd.0 as usize,
                                lpszText: PWSTR(-1isize as *mut u16),
                                ..Default::default()
                            };
                            SendMessageW(
                                tips,
                                TTM_ADDTOOLW,
                                None,
                                Some(LPARAM((&tool as *const TTTOOLINFOW) as isize)),
                            );
                        }
                        self.buttons.push(hwnd);
                    }
                }
                self.size = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("msctls_trackbar32"),
                    w!("Brush size in image pixels"),
                    WS_CHILD | WS_TABSTOP | WINDOW_STYLE(TBS_HORZ),
                    0,
                    0,
                    1,
                    1,
                    Some(parent),
                    Some(HMENU(980usize as *mut _)),
                    None,
                    None,
                )
                .ok();
                self.opacity = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("msctls_trackbar32"),
                    w!("Brush opacity"),
                    WS_CHILD | WS_TABSTOP | WINDOW_STYLE(TBS_HORZ),
                    0,
                    0,
                    1,
                    1,
                    Some(parent),
                    Some(HMENU(981usize as *mut _)),
                    None,
                    None,
                )
                .ok();
            }
            let buttons = dock.buttons();
            for (i, &hwnd) in self.buttons.iter().enumerate() {
                if let Some(&(action, r)) = buttons.get(i) {
                    let label = label(action);
                    let text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    let _ = SetWindowTextW(hwnd, PCWSTR(text.as_ptr()));
                    SetWindowLongPtrW(
                        hwnd,
                        GWLP_USERDATA,
                        (if matches!(action, Action::Copy | Action::Done) {
                            1
                        } else {
                            0
                        }) | (if active == Some(action) { 2 } else { 0 }),
                    );
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.0,
                        r.1,
                        r.2 - r.0,
                        r.3 - r.1,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                        hwnd,
                        !matches!(action, Action::Copy | Action::Save) || selected && !busy,
                    );
                    if action == Action::Undo {
                        let _ =
                            windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(hwnd, undo);
                    }
                    if action == Action::Redo {
                        let _ =
                            windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(hwnd, redo);
                    }
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                } else {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
            for (control, is_opacity) in [(self.size, false), (self.opacity, true)] {
                if let Some(hwnd) = control {
                    if let Some(r) = dock.slider().filter(|_| !is_opacity || soft) {
                        let dy = if is_opacity { dock.slider_step() } else { 0 };
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            r.0,
                            r.1 + dy,
                            r.2 - r.0,
                            r.3 - r.1,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                        SendMessageW(
                            hwnd,
                            TBM_SETRANGE,
                            Some(WPARAM(1)),
                            Some(LPARAM(if is_opacity {
                                255 << 16
                            } else {
                                1 | (64 << 16)
                            })),
                        );
                        SendMessageW(
                            hwnd,
                            TBM_SETPOS,
                            Some(WPARAM(1)),
                            Some(LPARAM(if is_opacity {
                                alpha as isize
                            } else {
                                radius.round().clamp(1., 64.) as isize
                            })),
                        );
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    } else {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
        }
    }
    pub fn invoke(&self, action: Action, dock: &Dock) -> bool {
        if let Some(i) = dock.buttons().iter().position(|p| p.0 == action)
            && let Some(&hwnd) = self.buttons.get(i)
        {
            return unsafe { PostMessageW(Some(hwnd), BM_CLICK, WPARAM(0), LPARAM(0)).is_ok() };
        }
        false
    }
    pub fn value_names(&self, radius: f32, alpha: u8) {
        unsafe {
            for (control, text) in [
                (self.size, format!("Brush radius {radius} image pixels")),
                (
                    self.opacity,
                    format!("Brush opacity {} percent", alpha as u32 * 100 / 255),
                ),
            ] {
                if let Some(hwnd) = control {
                    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                    let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
                    if let Some(service) = &self.accessibility {
                        let _ = service.SetHwndPropStr(
                            hwnd,
                            OBJID_CLIENT.0 as u32,
                            0,
                            PROPID_ACC_NAME,
                            PCWSTR(wide.as_ptr()),
                        );
                    }
                }
            }
        }
    }
    pub fn slider_values(&self, source: HWND) -> Vec<(bool, i32)> {
        [(self.size, false), (self.opacity, true)]
            .into_iter()
            .filter_map(|(handle, opacity)| {
                handle
                    .filter(|&h| source.0.is_null() || h == source)
                    .filter(|&h| unsafe { IsWindowVisible(h).as_bool() })
                    .map(|h| {
                        (opacity, unsafe {
                            SendMessageW(h, WM_USER, None, None).0 as i32
                        })
                    })
            })
            .collect()
    }
    pub fn set_slider_value(&self, opacity: bool, value: i32) {
        if let Some(hwnd) = if opacity { self.opacity } else { self.size } {
            unsafe {
                SendMessageW(
                    hwnd,
                    TBM_SETPOS,
                    Some(WPARAM(1)),
                    Some(LPARAM(value as isize)),
                );
            }
        }
    }
    pub fn tooltip(l: LPARAM) -> bool {
        unsafe {
            let header = &*(l.0 as *const NMHDR);
            if header.code != TTN_GETDISPINFOW {
                return false;
            }
            let info = &mut *(l.0 as *mut NMTTDISPINFOW);
            GetWindowTextW(HWND(info.hdr.idFrom as *mut _), &mut info.szText);
            info.lpszText = PWSTR(info.szText.as_mut_ptr());
            true
        }
    }
    pub fn draw_slider(l: LPARAM) -> Option<LRESULT> {
        unsafe {
            let header = &*(l.0 as *const NMHDR);
            if header.code != NM_CUSTOMDRAW {
                return None;
            }
            let draw = &*(l.0 as *const NMCUSTOMDRAW);
            let dc = draw.hdc;
            if draw.dwDrawStage == CDDS_PREPAINT {
                let mut bounds = RECT::default();
                let _ = GetClientRect(header.hwndFrom, &mut bounds);
                let brush = CreateSolidBrush(color(0x00221a17, COLOR_WINDOW));
                let _ = FillRect(dc, &bounds, brush);
                let _ = DeleteObject(brush.into());
                let mut channel = RECT::default();
                let mut thumb = RECT::default();
                SendMessageW(
                    header.hwndFrom,
                    TBM_GETCHANNELRECT,
                    None,
                    Some(LPARAM((&mut channel as *mut RECT) as isize)),
                );
                SendMessageW(
                    header.hwndFrom,
                    TBM_GETTHUMBRECT,
                    None,
                    Some(LPARAM((&mut thumb as *mut RECT) as isize)),
                );
                let middle = (bounds.top + bounds.bottom) / 2;
                channel.top = middle - 2;
                channel.bottom = middle + 2;
                for (rect, fill) in [
                    (channel, color(0x00524039, COLOR_WINDOWTEXT)),
                    (thumb, color(0x00fcb4a5, COLOR_HIGHLIGHT)),
                ] {
                    let brush = CreateSolidBrush(fill);
                    let pen = CreatePen(PS_SOLID, 1, fill);
                    let old_brush = SelectObject(dc, brush.into());
                    let old_pen = SelectObject(dc, pen.into());
                    let _ = RoundRect(dc, rect.left, rect.top, rect.right, rect.bottom, 8, 8);
                    SelectObject(dc, old_pen);
                    SelectObject(dc, old_brush);
                    let _ = DeleteObject(pen.into());
                    let _ = DeleteObject(brush.into());
                }
                if windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() == header.hwndFrom {
                    let _ = DrawFocusRect(dc, &bounds);
                }
                return Some(LRESULT(CDRF_SKIPDEFAULT as isize));
            }
            Some(LRESULT(CDRF_DODEFAULT as isize))
        }
    }
    pub fn draw(item: &DRAWITEMSTRUCT) {
        if crate::ui_paint::button(item) {
            return;
        }
        unsafe {
            let dc = item.hDC;
            let rect = item.rcItem;
            let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
            let primary = GetWindowLongPtrW(item.hwndItem, GWLP_USERDATA) == 1;
            let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
            let surface = CreateSolidBrush(color(0x00221a17, COLOR_WINDOW));
            let _ = FillRect(dc, &rect, surface);
            let _ = DeleteObject(surface.into());
            let brush = CreateSolidBrush(color(
                if primary {
                    if pressed { 0x00e5ab9a } else { 0x00ffc2b7 }
                } else if pressed {
                    0x00382924
                } else {
                    0x00221a17
                },
                if pressed {
                    COLOR_HIGHLIGHT
                } else {
                    COLOR_WINDOW
                },
            ));
            let pen = CreatePen(
                PS_SOLID,
                1,
                COLORREF(if item.itemState.0 & ODS_FOCUS.0 != 0 {
                    0x00fcb4a5
                } else {
                    0x00221a17
                }),
            );
            let old_brush = SelectObject(dc, brush.into());
            let old_pen = SelectObject(dc, pen.into());
            let _ = RoundRect(
                dc,
                rect.left + 1,
                rect.top + 1,
                rect.right - 1,
                rect.bottom - 1,
                24,
                24,
            );
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
            let _ = DeleteObject(brush.into());
            let _ = DeleteObject(pen.into());
            let font = crate::ui_paint::font(((rect.bottom - rect.top) as f32 * 0.32) as i32);
            let old = SelectObject(dc, font.into());
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(
                dc,
                color(
                    if disabled {
                        0x0092847a
                    } else if primary {
                        0x00271815
                    } else {
                        0x00e8d9d5
                    },
                    if disabled {
                        COLOR_GRAYTEXT
                    } else {
                        COLOR_WINDOWTEXT
                    },
                ),
            );
            let mut text = [0u16; 96];
            let count = GetWindowTextW(item.hwndItem, &mut text);
            let mut bounds = rect;
            DrawTextW(
                dc,
                &mut text[..count as usize],
                &mut bounds,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            if item.itemState.0 & ODS_FOCUS.0 != 0 {
                let mut focus = rect;
                focus.left += 4;
                focus.top += 4;
                focus.right -= 4;
                focus.bottom -= 4;
                let _ = DrawFocusRect(dc, &focus);
            }
            SelectObject(dc, old);
        }
    }
}
pub fn label(action: Action) -> &'static str {
    match action {
        Action::Select => "Select",
        Action::Lasso => "Lasso",
        Action::Close => "Close",
        Action::Refine => "Refine",
        Action::Inspect => "Inspect",
        Action::Undo => "Undo",
        Action::Redo => "Redo",
        Action::Copy => "Copy",
        Action::More => "More",
        Action::Add => "Add",
        Action::Remove => "Remove",
        Action::Edge => "Edge",
        Action::Done => "Done",
        Action::Brush => "Brush",
        Action::Mode => "Quality",
        Action::Save => "Save PNG",
    }
}
