//! Cached native presentation. CPU/GDI fallback and authoritative exports stay independent.
use crate::{Frame, editor::View};
use anyhow::Result;
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Direct2D::{Common::*, *},
        Dxgi::Common::*,
    },
};

#[derive(Clone, Copy)]
pub struct Paint<'a> {
    pub capture: u64,
    pub revision: u64,
    pub frame: &'a Frame,
    pub canvas: &'a [u8],
    pub view: View,
    pub opacity: f32,
}
struct Scene {
    target: ID2D1HwndRenderTarget,
    original: ID2D1Bitmap,
    current: Option<ID2D1Bitmap>,
    capture: u64,
    revision: u64,
}
#[derive(Default)]
pub struct Presenter {
    scene: Option<Scene>,
    failed_capture: Option<u64>,
    pub uploads: u64,
}
impl Presenter {
    pub fn reset(&mut self) {
        self.scene = None;
        self.failed_capture = None;
    }
    pub fn draw(&mut self, hwnd: HWND, paint: Paint<'_>) -> bool {
        let capture = paint.capture;
        if paint.frame.rgba.len() > 64 * 1024 * 1024 {
            return false;
        }
        if paint.canvas.len() != paint.frame.rgba.len() {
            return false;
        }
        if std::env::var("OBJECTLIFT_RENDERER").as_deref() == Ok("gdi")
            || self.failed_capture == Some(capture)
        {
            return false;
        }
        let result = self.draw_inner(hwnd, paint);
        if let Err(error) = result {
            eprintln!("Direct2D unavailable; using GDI for this capture: {error}");
            self.scene = None;
            self.failed_capture = Some(capture);
            false
        } else {
            true
        }
    }
    fn draw_inner(&mut self, hwnd: HWND, paint: Paint<'_>) -> Result<()> {
        let Paint {
            capture,
            revision,
            frame,
            canvas,
            view,
            opacity,
        } = paint;
        unsafe {
            let size = D2D_SIZE_U {
                width: frame.width,
                height: frame.height,
            };
            let bitmap_properties = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.,
                dpiY: 96.,
            };
            if self.scene.as_ref().is_none_or(|s| s.capture != capture) {
                self.scene = None;
                let factory: ID2D1Factory =
                    D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
                let target = factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        pixelFormat: bitmap_properties.pixelFormat,
                        dpiX: 96.,
                        dpiY: 96.,
                        ..Default::default()
                    },
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd,
                        pixelSize: size,
                        presentOptions: D2D1_PRESENT_OPTIONS_RETAIN_CONTENTS,
                    },
                )?;
                let mut bgra = frame.rgba.clone();
                for p in bgra.chunks_exact_mut(4) {
                    p.swap(0, 2);
                    p[3] = 255;
                }
                let original = target.CreateBitmap(
                    size,
                    Some(bgra.as_ptr().cast()),
                    frame.width * 4,
                    &bitmap_properties,
                )?;
                self.scene = Some(Scene {
                    target,
                    original,
                    current: None,
                    capture,
                    revision: u64::MAX,
                });
                eprintln!("Presentation: cached Direct2D");
            }
            let scene = self.scene.as_mut().unwrap();
            if scene.revision != revision {
                // Replace rather than retaining every visited candidate; GPU cache is two frames.
                scene.current = None;
                scene.current = Some(scene.target.CreateBitmap(
                    size,
                    Some(canvas.as_ptr().cast()),
                    frame.width * 4,
                    &bitmap_properties,
                )?);
                scene.revision = revision;
                self.uploads += 1;
            }
            let source = D2D_RECT_F {
                left: view.x,
                top: view.y,
                right: view.x + frame.width as f32 / view.scale,
                bottom: view.y + frame.height as f32 / view.scale,
            };
            let destination = D2D_RECT_F {
                left: 0.,
                top: 0.,
                right: frame.width as f32,
                bottom: frame.height as f32,
            };
            scene.target.BeginDraw();
            scene.target.DrawBitmap(
                &scene.original,
                Some(&destination),
                1.,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                Some(&source),
            );
            if let Some(bitmap) = &scene.current {
                scene.target.DrawBitmap(
                    bitmap,
                    Some(&destination),
                    opacity,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    Some(&source),
                );
            }
            scene.target.EndDraw(None, None)?;
            Ok(())
        }
    }
}

use windows::Win32::Graphics::Gdi::*;
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut std::ffi::c_void,
    len: usize,
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
impl Surface {
    unsafe fn new(dc: HDC, frame: &Frame, pixels: &[u8]) -> Result<Self> {
        unsafe {
            let memory = CreateCompatibleDC(Some(dc));
            anyhow::ensure!(!memory.is_invalid(), "No memory DC");
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: frame.width as i32,
                    biHeight: -(frame.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(value) => value,
                Err(error) => {
                    let _ = DeleteDC(memory);
                    return Err(error.into());
                }
            };
            let old = SelectObject(memory, bitmap.into());
            let mut result = Self {
                dc: memory,
                bitmap,
                old,
                bits,
                len: pixels.len(),
            };
            result.update(pixels)?;
            Ok(result)
        }
    }
    fn update(&mut self, pixels: &[u8]) -> Result<()> {
        anyhow::ensure!(
            !self.bits.is_null() && self.len == pixels.len(),
            "Invalid bitmap data"
        );
        // Complete queued blits before changing DIB memory used by GDI.
        unsafe {
            GdiFlush().ok()?;
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits.cast(), pixels.len());
        }
        Ok(())
    }
}
struct GdiScene {
    original: Surface,
    current: Surface,
    capture: u64,
    revision: u64,
}
#[derive(Default)]
pub struct CachedGdi {
    scene: Option<GdiScene>,
    failed_capture: Option<u64>,
    pub uploads: u64,
}
impl CachedGdi {
    pub fn reset(&mut self) {
        self.scene = None;
        self.failed_capture = None;
    }
    pub fn draw(&mut self, dc: HDC, paint: Paint<'_>) -> bool {
        if paint.frame.rgba.len() > 64 * 1024 * 1024 {
            return false;
        }
        if self.failed_capture == Some(paint.capture) {
            return false;
        }
        match self.draw_inner(dc, paint) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("Cached GDI unavailable; static fallback for this capture: {error}");
                self.scene = None;
                self.failed_capture = Some(paint.capture);
                false
            }
        }
    }
    fn draw_inner(&mut self, dc: HDC, paint: Paint<'_>) -> Result<()> {
        unsafe {
            let Paint {
                capture,
                revision,
                frame,
                canvas,
                view,
                opacity,
            } = paint;
            anyhow::ensure!(canvas.len() == frame.rgba.len(), "Invalid canvas");
            if self.scene.as_ref().is_none_or(|s| s.capture != capture) {
                self.scene = None;
                let mut bgra = frame.rgba.clone();
                for p in bgra.chunks_exact_mut(4) {
                    p.swap(0, 2);
                    p[3] = 255;
                }
                let original = Surface::new(dc, frame, &bgra)?;
                let current = Surface::new(dc, frame, canvas)?;
                self.scene = Some(GdiScene {
                    original,
                    current,
                    capture,
                    revision,
                });
                self.uploads += 1;
            }
            let scene = self.scene.as_mut().unwrap();
            if scene.revision != revision {
                scene.current.update(canvas)?;
                scene.revision = revision;
                self.uploads += 1;
            }
            let (w, h) = (frame.width as i32, frame.height as i32);
            let (x, y, sw, sh) = (
                view.x as i32,
                view.y as i32,
                (frame.width as f32 / view.scale) as i32,
                (frame.height as f32 / view.scale) as i32,
            );
            if opacity >= 1. {
                StretchBlt(
                    dc,
                    0,
                    0,
                    w,
                    h,
                    Some(scene.current.dc),
                    x,
                    y,
                    sw,
                    sh,
                    SRCCOPY,
                )
                .ok()?;
            } else {
                StretchBlt(
                    dc,
                    0,
                    0,
                    w,
                    h,
                    Some(scene.original.dc),
                    x,
                    y,
                    sw,
                    sh,
                    SRCCOPY,
                )
                .ok()?;
                AlphaBlend(
                    dc,
                    0,
                    0,
                    w,
                    h,
                    scene.current.dc,
                    x,
                    y,
                    sw,
                    sh,
                    BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: (opacity.clamp(0., 1.) * 255.).round() as u8,
                        AlphaFormat: 0,
                    },
                )
                .ok()?;
            }
            Ok(())
        }
    }
}
