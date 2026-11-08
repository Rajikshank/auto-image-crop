use anyhow::{Context, Result, bail};
use windows::{
    Win32::{
        Foundation::*, Graphics::Gdi::*, System::LibraryLoader::*, UI::WindowsAndMessaging::*,
    },
    core::w,
};

unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let brush = CreateSolidBrush(COLORREF(0x0035bf42));
                let _ = FillRect(
                    dc,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: 320,
                        bottom: 240,
                    },
                    brush,
                );
                let _ = DeleteObject(brush.into());
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }
}
pub fn run(path: &str, browser: bool) -> Result<()> {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: w!("ObjectLiftCaptureProbe"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            w!("ObjectLiftCaptureProbe"),
            w!("ObjectLift capture verification"),
            WS_POPUP | WS_VISIBLE,
            40,
            40,
            320,
            240,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let _ = UpdateWindow(hwnd);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let window_dc = GetDC(Some(hwnd));
        let screen_dc = GetDC(None);
        eprintln!(
            "window_pixel={:?} screen_pixel={:?} remote_session={}",
            GetPixel(window_dc, 60, 60),
            GetPixel(screen_dc, 100, 100),
            GetSystemMetrics(SM_REMOTESESSION)
        );
        ReleaseDC(Some(hwnd), window_dc);
        ReleaseDC(None, screen_dc);
        let result = crate::platform::capture();
        let _ = ShowWindow(hwnd, SW_HIDE);
        let clipboard = if browser {
            crate::platform::browser_clipboard_probe(hwnd)
        } else {
            crate::platform::clipboard_probe(hwnd)
        };
        DestroyWindow(hwnd)?;
        clipboard?;
        let desktop = result?;
        std::fs::write(path, crate::platform::png(&desktop.frame)?)?;
        let x = (100 - desktop.left) as u32;
        let y = (100 - desktop.top) as u32;
        let pixel = desktop
            .frame
            .rgba
            .get(
                ((y * desktop.frame.width + x) * 4) as usize
                    ..((y * desktop.frame.width + x) * 4 + 4) as usize,
            )
            .context("Probe outside capture")?;
        if pixel != [0x42, 0xbf, 0x35, 255] {
            bail!("Capture verification failed: expected painted probe pixel, got {pixel:?}");
        }
        println!(
            "PASS: actual desktop capture contains the native painted window (DXGI with GDI fallback)"
        );
        Ok(())
    }
}
