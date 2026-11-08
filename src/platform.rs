//! Windows capture and clipboard boundaries. All unsafe Win32 access stays here or in the native UI.
use crate::Frame;
use anyhow::{Context, Result, bail};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
        System::{DataExchange::*, Memory::*},
    },
    core::Interface,
};

pub struct Desktop {
    pub frame: Frame,
    pub left: i32,
    pub top: i32,
}

pub fn capture() -> Result<Desktop> {
    match capture_dxgi() {
        Ok(d)
            if d.frame
                .rgba
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0) =>
        {
            Ok(d)
        }
        result => {
            eprintln!(
                "DXGI returned unavailable/black pixels ({}); trying GDI capture",
                result
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "black frame".into())
            );
            capture_gdi()
        }
    }
}

pub fn capture_gdi() -> Result<Desktop> {
    use windows::Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::*};
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w <= 0 || h <= 0 || i64::from(w) * i64::from(h) > 64_000_000 {
            bail!("Invalid virtual desktop dimensions");
        }
        let screen = GetDC(None);
        if screen.is_invalid() {
            bail!("Cannot access desktop DC");
        }
        let dc = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        if dc.is_invalid() || bitmap.is_invalid() {
            if !dc.is_invalid() {
                let _ = DeleteDC(dc);
            }
            if !bitmap.is_invalid() {
                let _ = DeleteObject(bitmap.into());
            }
            ReleaseDC(None, screen);
            bail!("Cannot allocate screen capture");
        }
        let old = SelectObject(dc, bitmap.into());
        let blit = BitBlt(
            dc,
            0,
            0,
            w,
            h,
            Some(screen),
            left,
            top,
            SRCCOPY | CAPTUREBLT,
        );
        SelectObject(dc, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let rows = GetDIBits(
            dc,
            bitmap,
            0,
            h as u32,
            Some(rgba.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        blit?;
        if rows != h {
            bail!("Incomplete GDI screen capture");
        }
        for p in rgba.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        Ok(Desktop {
            frame: Frame::new(w as u32, h as u32, rgba).map_err(anyhow::Error::msg)?,
            left,
            top,
        })
    }
}

pub fn capture_dxgi() -> Result<Desktop> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut pieces = Vec::new();
        let mut adapter_index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
            adapter_index += 1;
            let mut output_index = 0;
            while let Ok(output) = adapter.EnumOutputs(output_index) {
                output_index += 1;
                let desc = output.GetDesc()?;
                if !desc.AttachedToDesktop.as_bool() {
                    continue;
                }
                let mut device = None;
                let mut context = None;
                let base: IDXGIAdapter = adapter.cast()?;
                D3D11CreateDevice(
                    &base,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )?;
                let device = device.context("No D3D11 device")?;
                let context = context.context("No D3D11 context")?;
                let output1: IDXGIOutput1 = output.cast()?;
                let duplication = output1
                    .DuplicateOutput(&device)
                    .context("Desktop capture unavailable (locked, protected, or device lost)")?;
                let mut resource = None;
                duplication.AcquireNextFrame(
                    1000,
                    &mut DXGI_OUTDUPL_FRAME_INFO::default(),
                    &mut resource,
                )?;
                // Release the acquired frame even when staging/readback fails.
                let readback = (|| -> Result<Frame> {
                    let texture: ID3D11Texture2D =
                        resource.context("No captured texture")?.cast()?;
                    let mut td = D3D11_TEXTURE2D_DESC::default();
                    texture.GetDesc(&mut td);
                    td.Usage = D3D11_USAGE_STAGING;
                    td.BindFlags = 0;
                    td.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                    td.MiscFlags = 0;
                    let mut staging = None;
                    device.CreateTexture2D(&td, None, Some(&mut staging))?;
                    let staging = staging.context("No staging texture")?;
                    context.CopyResource(&staging, &texture);
                    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                    context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
                    let (w, h) = (
                        (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32,
                        (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32,
                    );
                    let mut rgba = vec![0u8; (w * h * 4) as usize];
                    for y in 0..h {
                        for x in 0..w {
                            let (sx, sy) = match desc.Rotation {
                                DXGI_MODE_ROTATION_ROTATE90 => (y, td.Height - 1 - x),
                                DXGI_MODE_ROTATION_ROTATE180 => {
                                    (td.Width - 1 - x, td.Height - 1 - y)
                                }
                                DXGI_MODE_ROTATION_ROTATE270 => (td.Width - 1 - y, x),
                                _ => (x, y),
                            };
                            if sx >= td.Width || sy >= td.Height {
                                continue;
                            }
                            let p = (mapped.pData as *const u8)
                                .add(sy as usize * mapped.RowPitch as usize + sx as usize * 4);
                            let dst = ((y * w + x) * 4) as usize;
                            rgba[dst..dst + 4].copy_from_slice(&[*p.add(2), *p.add(1), *p, 255]);
                        }
                    }
                    context.Unmap(&staging, 0);
                    Frame::new(w, h, rgba).map_err(anyhow::Error::msg)
                })();
                let released = duplication.ReleaseFrame();
                let frame = readback?;
                released?;
                pieces.push((desc.DesktopCoordinates, frame));
            }
        }
        if pieces.is_empty() {
            bail!("No capturable desktop monitors found");
        }
        let left = pieces.iter().map(|(r, _)| r.left).min().unwrap();
        let top = pieces.iter().map(|(r, _)| r.top).min().unwrap();
        let right = pieces.iter().map(|(r, _)| r.right).max().unwrap();
        let bottom = pieces.iter().map(|(r, _)| r.bottom).max().unwrap();
        let (w, h) = ((right - left) as u32, (bottom - top) as u32);
        if u64::from(w) * u64::from(h) > 64_000_000 {
            bail!("Desktop exceeds the 64 megapixel capture limit");
        }
        let mut rgba = vec![0; (w * h * 4) as usize];
        for (r, f) in pieces {
            for y in 0..f.height {
                let dst = (((r.top - top) as u32 + y) * w + (r.left - left) as u32) as usize * 4;
                let src = (y * f.width * 4) as usize;
                rgba[dst..dst + (f.width * 4) as usize]
                    .copy_from_slice(&f.rgba[src..src + (f.width * 4) as usize]);
            }
        }
        Ok(Desktop {
            frame: Frame::new(w, h, rgba).map_err(anyhow::Error::msg)?,
            left,
            top,
        })
    }
}

pub fn png(frame: &Frame) -> Result<Vec<u8>> {
    let mut output = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
        .context("Invalid RGBA image")?
        .write_to(&mut output, image::ImageFormat::Png)?;
    Ok(output.into_inner())
}

pub fn dibv5(frame: &Frame) -> Vec<u8> {
    let mut bytes = vec![0u8; 124 + frame.rgba.len()];
    let mut write = |at: usize, v: u32| bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    write(0, 124);
    write(4, frame.width);
    write(8, frame.height);
    write(12, 1 | (32 << 16));
    write(16, 3);
    write(20, frame.rgba.len() as u32);
    write(40, 0x00ff0000);
    write(44, 0x0000ff00);
    write(48, 0x000000ff);
    write(52, 0xff000000);
    write(56, 0x73524742);
    write(108, 4);
    for y in 0..frame.height {
        for x in 0..frame.width {
            let s = ((y * frame.width + x) * 4) as usize;
            let d = 124 + (((frame.height - 1 - y) * frame.width + x) * 4) as usize;
            bytes[d..d + 4].copy_from_slice(&[
                ((u16::from(frame.rgba[s + 2]) * u16::from(frame.rgba[s + 3]) + 127) / 255) as u8,
                ((u16::from(frame.rgba[s + 1]) * u16::from(frame.rgba[s + 3]) + 127) / 255) as u8,
                ((u16::from(frame.rgba[s]) * u16::from(frame.rgba[s + 3]) + 127) / 255) as u8,
                frame.rgba[s + 3],
            ]);
        }
    }
    bytes
}

pub struct PreparedClipboard {
    png: Vec<u8>,
    dib: Vec<u8>,
}
impl PreparedClipboard {
    pub fn save_png(&self, path: &std::path::Path) -> Result<()> {
        std::fs::write(path, &self.png).context("Saving transparent PNG")
    }
}

pub fn choose_export_path(
    owner: HWND,
    format: crate::file_export::Format,
) -> Result<Option<(std::path::PathBuf, bool)>> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::UI::Controls::Dialogs::*;
    let mut name = vec![0u16; 32768];
    let default: Vec<u16> = format!("ObjectLift.{}", format.extension())
        .encode_utf16()
        .collect();
    let filter: Vec<u16> = format!(
        "{} (*.{})\0*.{}\0\0",
        format.label(),
        format.extension(),
        format.extension()
    )
    .encode_utf16()
    .collect();
    let extension: Vec<u16> = format.extension().encode_utf16().chain(Some(0)).collect();
    name[..default.len()].copy_from_slice(&default);
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: windows::core::PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(name.as_mut_ptr()),
        nMaxFile: name.len() as u32,
        lpstrDefExt: windows::core::PCWSTR(extension.as_ptr()),
        lpstrTitle: windows::core::w!("Save transparent cutout"),
        Flags: OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    unsafe {
        if !GetSaveFileNameW(&mut dialog).as_bool() {
            let code = CommDlgExtendedError();
            if code.0 != 0 {
                bail!("Save dialog failed: {}", code.0);
            }
            return Ok(None);
        }
    }
    let length = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&name[..length]));
    anyhow::ensure!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case(format.extension())),
        "Use the .{} extension for this format",
        format.extension()
    );
    let replace = path.try_exists().context("Checking export destination")?;
    if replace {
        use windows::Win32::UI::WindowsAndMessaging::*;
        let prompt: Vec<u16> = format!("Replace the existing file?\n{}", path.display())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        if unsafe {
            MessageBoxW(
                Some(owner),
                windows::core::PCWSTR(prompt.as_ptr()),
                windows::core::w!("ObjectLift"),
                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
            )
        } != IDYES
        {
            return Ok(None);
        }
    }
    Ok(Some((path, replace)))
}
pub fn prepare(frame: &Frame) -> Result<PreparedClipboard> {
    Ok(PreparedClipboard {
        png: png(frame)?,
        dib: dibv5(frame),
    })
}
pub fn copy(frame: &Frame, owner: HWND) -> Result<()> {
    publish(prepare(frame)?, owner)
}
pub fn publish(prepared: PreparedClipboard, owner: HWND) -> Result<()> {
    let PreparedClipboard { png, dib } = prepared;
    unsafe {
        // Allocate both formats before touching the user's clipboard.
        let make = |bytes: &[u8]| -> Result<HGLOBAL> {
            let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
            let ptr = GlobalLock(mem);
            if ptr.is_null() {
                let _ = GlobalFree(Some(mem));
                bail!("Cannot lock clipboard allocation");
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
            let _ = GlobalUnlock(mem);
            Ok(mem)
        };
        let p = make(&png)?;
        let d = match make(&dib) {
            Ok(d) => d,
            Err(e) => {
                let _ = GlobalFree(Some(p));
                return Err(e);
            }
        };
        // Publish an explicit V5 buffer under CF_DIB too. Letting Windows synthesize
        // CF_DIB from CF_DIBV5 strips the alpha-mask header used by older readers.
        let legacy = match make(&dib) {
            Ok(h) => h,
            Err(e) => {
                let _ = GlobalFree(Some(p));
                let _ = GlobalFree(Some(d));
                return Err(e);
            }
        };
        if let Err(e) = OpenClipboard(Some(owner)) {
            let _ = GlobalFree(Some(p));
            let _ = GlobalFree(Some(d));
            let _ = GlobalFree(Some(legacy));
            return Err(e.into());
        }
        let format = RegisterClipboardFormatW(windows::core::w!("PNG"));
        if format == 0 {
            let _ = GlobalFree(Some(p));
            let _ = GlobalFree(Some(d));
            let _ = GlobalFree(Some(legacy));
            let _ = CloseClipboard();
            bail!("Cannot register PNG clipboard format");
        }
        if let Err(e) = EmptyClipboard() {
            let _ = GlobalFree(Some(p));
            let _ = GlobalFree(Some(d));
            let _ = GlobalFree(Some(legacy));
            let _ = CloseClipboard();
            return Err(e.into());
        }
        let publish = (|| -> Result<()> {
            if let Err(e) = SetClipboardData(format, Some(HANDLE(p.0))) {
                let _ = GlobalFree(Some(p));
                let _ = GlobalFree(Some(d));
                let _ = GlobalFree(Some(legacy));
                return Err(e.into());
            }
            if let Err(e) = SetClipboardData(17, Some(HANDLE(d.0))) {
                let _ = GlobalFree(Some(d));
                let _ = GlobalFree(Some(legacy));
                return Err(e.into());
            }
            if let Err(e) = SetClipboardData(8, Some(HANDLE(legacy.0))) {
                let _ = GlobalFree(Some(legacy));
                return Err(e.into());
            }
            Ok(())
        })();
        let closed = CloseClipboard();
        publish?;
        closed?;
        Ok(())
    }
}

/// Diagnostic only: preserve HGLOBAL-backed formats and restore them after a native round-trip.
/// Refuses to modify the clipboard if any format cannot be safely snapshotted.
enum SavedData {
    Memory(Vec<u8>),
    Bitmap(SavedBitmap),
}
struct SavedBitmap(Option<windows::Win32::Graphics::Gdi::HBITMAP>);
impl Drop for SavedBitmap {
    fn drop(&mut self) {
        if let Some(bitmap) = self.0.take() {
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(bitmap.into());
            }
        }
    }
}
pub fn clipboard_probe(owner: HWND) -> Result<()> {
    clipboard_probe_impl(owner, false)
}
pub fn browser_clipboard_probe(owner: HWND) -> Result<()> {
    clipboard_probe_impl(owner, true)
}
fn clipboard_probe_impl(owner: HWND, browser: bool) -> Result<()> {
    unsafe {
        OpenClipboard(Some(owner)).context("Opening clipboard to preserve its current contents")?;
        let snapshot = (|| -> Result<Vec<(u32, SavedData)>> {
            let mut all = Vec::new();
            let mut format = EnumClipboardFormats(0);
            while format != 0 {
                let handle = GetClipboardData(format).with_context(|| {
                    format!("Reading clipboard format {format} for safe restoration")
                })?;
                if format == 2 {
                    // Windows synthesizes CF_BITMAP from DIB/DIBV5. Preserve the
                    // source pixel buffer instead of duplicating a synthesized GDI handle.
                    if IsClipboardFormatAvailable(8).is_ok()
                        || IsClipboardFormatAvailable(17).is_ok()
                    {
                        format = EnumClipboardFormats(format);
                        continue;
                    }
                    use windows::Win32::UI::WindowsAndMessaging::{
                        CopyImage, IMAGE_BITMAP, LR_CREATEDIBSECTION,
                    };
                    let copied = CopyImage(handle, IMAGE_BITMAP, 0, 0, LR_CREATEDIBSECTION)
                        .context("Cloning existing clipboard bitmap")?;
                    all.push((
                        format,
                        SavedData::Bitmap(SavedBitmap(Some(
                            windows::Win32::Graphics::Gdi::HBITMAP(copied.0),
                        ))),
                    ));
                    format = EnumClipboardFormats(format);
                    continue;
                }
                let global = HGLOBAL(handle.0);
                let size = GlobalSize(global);
                if size == 0 || size > 256 * 1024 * 1024 {
                    bail!(
                        "Clipboard format {format} cannot be safely preserved; probe skipped without mutation"
                    );
                }
                let ptr = GlobalLock(global);
                if ptr.is_null() {
                    bail!("Cannot preserve clipboard format {format}");
                }
                let bytes = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
                let _ = GlobalUnlock(global);
                all.push((format, SavedData::Memory(bytes)));
                format = EnumClipboardFormats(format);
            }
            Ok(all)
        })();
        CloseClipboard().context("Closing clipboard after snapshot")?;
        let snapshot = snapshot?;
        let result = (|| -> Result<()> {
            let frame = Frame::new(2, 1, vec![10, 20, 30, 128, 40, 50, 60, 255])
                .map_err(anyhow::Error::msg)?;
            copy(&frame, owner).context("Publishing test PNG and DIBV5")?;
            if browser {
                println!(
                    "READY: generated 2x1 PNG is on the clipboard; waiting for local browser paste verification"
                );
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                let path = std::path::Path::new("artifacts/browser-paste.json");
                while !path.exists() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                let result: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(path).context("Timed out waiting for browser paste")?,
                )?;
                if result["passed"] != true {
                    bail!("Browser paste failed: {result}");
                }
            }
            OpenClipboard(Some(owner)).context("Opening clipboard to read back test PNG")?;
            let check = (|| -> Result<()> {
                let fmt = RegisterClipboardFormatW(windows::core::w!("PNG"));
                let handle = GetClipboardData(fmt)?;
                let global = HGLOBAL(handle.0);
                let size = GlobalSize(global);
                let ptr = GlobalLock(global);
                if ptr.is_null() {
                    bail!("Cannot read native PNG clipboard");
                }
                let bytes = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
                let _ = GlobalUnlock(global);
                let decoded = image::load_from_memory(&bytes)?.to_rgba8();
                if decoded.into_raw() != frame.rgba {
                    bail!("Native clipboard alpha/channel round-trip mismatch");
                }
                // Independently inspect the legacy format that many receiving apps request.
                let h = GetClipboardData(8).context("Reading receiving-app CF_DIB fallback")?;
                let g = HGLOBAL(h.0);
                let size = GlobalSize(g);
                let ptr = GlobalLock(g);
                if ptr.is_null() {
                    bail!("Cannot inspect receiving-app DIB");
                }
                let data = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
                let _ = GlobalUnlock(g);
                if data.len() < 40 {
                    bail!("Invalid receiving-app DIB");
                }
                let header = u32::from_le_bytes(data[0..4].try_into().unwrap());
                let depth = u16::from_le_bytes(data[14..16].try_into().unwrap());
                println!("Receiving-app CF_DIB: header={header} bytes, depth={depth}");
                if header != 124 || depth != 32 {
                    bail!(
                        "Receiving-app CF_DIB lost the explicit alpha header during Windows format conversion"
                    );
                }
                if data != dibv5(&frame) {
                    bail!(
                        "Receiving-app CF_DIB payload changed alpha or premultiplied color values"
                    );
                }
                Ok(())
            })();
            CloseClipboard().context("Closing clipboard after test readback")?;
            check
        })();
        // Always attempt restoration, including when the check fails.
        OpenClipboard(Some(owner)).context("Opening clipboard to restore original formats")?;
        let restored = (|| -> Result<()> {
            EmptyClipboard()?;
            for (format, data) in snapshot {
                let bytes = match data {
                    SavedData::Memory(bytes) => bytes,
                    SavedData::Bitmap(mut saved) => {
                        let bitmap = saved.0.context("Missing saved clipboard bitmap")?;
                        SetClipboardData(format, Some(HANDLE(bitmap.0)))
                            .context("Restoring original clipboard bitmap")?;
                        saved.0 = None;
                        continue;
                    }
                };
                let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
                let ptr = GlobalLock(mem);
                if ptr.is_null() {
                    let _ = GlobalFree(Some(mem));
                    bail!("Cannot restore clipboard allocation");
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast(), bytes.len());
                let _ = GlobalUnlock(mem);
                if let Err(e) = SetClipboardData(format, Some(HANDLE(mem.0))) {
                    let _ = GlobalFree(Some(mem));
                    return Err(e.into());
                }
            }
            Ok(())
        })();
        let closed = CloseClipboard();
        restored?;
        closed?;
        if browser {
            println!("Original clipboard contents restored after browser probe");
        }
        result?;
        println!("PASS: native clipboard PNG round-trip preserves RGBA; original formats restored");
        Ok(())
    }
}
