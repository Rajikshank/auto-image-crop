//! Reused Direct2D/DirectWrite resources for native owner-drawn controls.
use std::cell::RefCell;
use windows::{
    Win32::{
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::*,
        },
        UI::{Controls::*, WindowsAndMessaging::*},
    },
    core::w,
};
struct Painter {
    target: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    write: IDWriteFactory,
    format: IDWriteTextFormat,
    size: f32,
}
thread_local! {static PAINTER:RefCell<Option<Painter>>=const {RefCell::new(None)};}
fn color(rgb: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 255) as f32 / 255.,
        g: ((rgb >> 8) & 255) as f32 / 255.,
        b: (rgb & 255) as f32 / 255.,
        a: 1.,
    }
}
impl Painter {
    unsafe fn new() -> windows::core::Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = factory.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.,
                dpiY: 96.,
                ..Default::default()
            })?;
            let brush = target.CreateSolidColorBrush(&color(0xffffff), None)?;
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let format = write.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                14.,
                w!("en-us"),
            )?;
            Ok(Self {
                target,
                brush,
                write,
                format,
                size: 14.,
            })
        }
    }
    unsafe fn button(&mut self, item: &DRAWITEMSTRUCT) -> windows::core::Result<()> {
        unsafe {
            self.target.BindDC(item.hDC, &item.rcItem)?;
            let width = (item.rcItem.right - item.rcItem.left) as f32;
            let height = (item.rcItem.bottom - item.rcItem.top) as f32;
            let size = (height * 0.32).max(12.);
            if (size - self.size).abs() > 0.1 {
                self.format = self.write.CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    DWRITE_FONT_WEIGHT_MEDIUM,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )?;
                self.size = size;
            }
            self.format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            self.format
                .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            let flags = GetWindowLongPtrW(item.hwndItem, GWLP_USERDATA);
            let primary = flags & 1 != 0;
            let active = flags & 2 != 0;
            let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
            let hot = item.itemState.0 & ODS_HOTLIGHT.0 != 0;
            let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
            let focus = item.itemState.0 & ODS_FOCUS.0 != 0;
            let mut text = [0u16; 96];
            let count = GetWindowTextW(item.hwndItem, &mut text) as usize;
            let name = String::from_utf16_lossy(&text[..count]);
            let label = match name.as_str() {
                "More" => "\u{00b7}\u{00b7}\u{00b7}",
                "Close" => "\u{00d7}",
                "Undo" => "\u{21b6}",
                "Redo" => "\u{21b7}",
                other => other,
            };
            let text: Vec<u16> = label.encode_utf16().collect();
            self.target.BeginDraw();
            self.target.Clear(Some(&color(0x171a22)));
            let rounded = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: 2.,
                    top: 2.,
                    right: width - 2.,
                    bottom: height - 2.,
                },
                radiusX: 12.,
                radiusY: 12.,
            };
            let fill = if primary {
                if pressed { 0x9ba9e4 } else { 0xb7c2ff }
            } else if active || pressed {
                0x30364e
            } else if hot {
                0x242938
            } else {
                0x171a22
            };
            self.brush.SetColor(&color(fill));
            self.target.FillRoundedRectangle(&rounded, &self.brush);
            if focus {
                self.brush.SetColor(&color(0xc4ccff));
                self.target
                    .DrawRoundedRectangle(&rounded, &self.brush, 1.5, None);
            }
            self.brush.SetColor(&color(if disabled {
                0x929bad
            } else if primary {
                0x151827
            } else {
                0xd5d9e8
            }));
            self.target.DrawText(
                &text,
                &self.format,
                &D2D_RECT_F {
                    left: 0.,
                    top: 0.,
                    right: width,
                    bottom: height,
                },
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.target.EndDraw(None, None)
        }
    }
}
pub fn button(item: &DRAWITEMSTRUCT) -> bool {
    if crate::native_dock::high_contrast() {
        return false;
    }
    PAINTER.with(|slot| {
        let Ok(mut painter) = slot.try_borrow_mut() else {
            return false;
        };
        if painter.is_none() {
            *painter = unsafe { Painter::new().ok() };
        }
        let result = painter
            .as_mut()
            .is_some_and(|p| unsafe { p.button(item).is_ok() });
        if !result {
            *painter = None;
        }
        result
    })
}

use windows::Win32::Graphics::Gdi::*;
struct Fonts(Vec<(i32, HFONT)>);
impl Drop for Fonts {
    fn drop(&mut self) {
        for (_, font) in self.0.drain(..) {
            unsafe {
                let _ = DeleteObject(font.into());
            }
        }
    }
}
thread_local! {static FONTS:RefCell<Fonts>=const {RefCell::new(Fonts(Vec::new()))};}
pub fn font(height: i32) -> HFONT {
    FONTS.with(|cache| {
        let mut fonts = cache.borrow_mut();
        if let Some((_, font)) = fonts.0.iter().find(|p| p.0 == height) {
            return *font;
        }
        unsafe {
            let font = CreateFontW(
                height,
                0,
                0,
                0,
                500,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            );
            if fonts.0.len() == 8 {
                let (_, old) = fonts.0.remove(0);
                let _ = DeleteObject(old.into());
            }
            fonts.0.push((height, font));
            font
        }
    })
}
