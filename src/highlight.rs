//! Preview-only coverage styling. No rendered pixels enter the authoritative cutout.
use crate::{Frame, Mask};
#[derive(Clone, Copy, Debug)]
pub struct Appearance {
    pub strength: u8,
    pub dim: u8,
    pub color: [u8; 3],
    pub contour: bool,
}
impl Appearance {
    pub const HOVER: Self = Self {
        strength: 89,
        dim: 46,
        color: [137, 151, 255],
        contour: true,
    };
    pub const SELECTED: Self = Self {
        strength: 115,
        dim: 71,
        ..Self::HOVER
    };
    pub const REFINE: Self = Self {
        strength: 64,
        dim: 46,
        ..Self::HOVER
    };
}

pub fn render(frame: &Frame, mask: Option<&Mask>) -> Vec<u8> {
    render_with(frame, mask, Appearance::HOVER)
}
pub fn render_exact(frame: &Frame, mask: &Mask) -> Vec<u8> {
    render_with(frame, Some(mask), Appearance::SELECTED)
}
pub fn render_with(frame: &Frame, mask: Option<&Mask>, appearance: Appearance) -> Vec<u8> {
    let Appearance {
        strength,
        dim,
        color,
        contour,
    } = appearance;
    let mask = mask.filter(|m| {
        (m.width, m.height) == (frame.width, frame.height) && m.alpha.len() == frame.rgba.len() / 4
    });
    let mut output = vec![0; frame.rgba.len()];
    let (w, h) = (frame.width as usize, frame.height as usize);
    let (mut left, mut top, mut right, mut bottom) = (w, h, 0, 0);
    let background = if mask.is_some() {
        255 - dim as u32
    } else {
        255
    };
    let colors = [color[2] as u32, color[1] as u32, color[0] as u32];
    // Walk rows to avoid a division/modulo for every included pixel. Common
    // fully excluded/opaque pixels bypass the fractional-alpha arithmetic.
    for (y, (src, dst)) in frame
        .rgba
        .chunks_exact(w * 4)
        .zip(output.chunks_exact_mut(w * 4))
        .enumerate()
    {
        for (x, (p, out)) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)).enumerate() {
            let a = mask.map_or(0, |m| m.alpha[y * w + x] as u32 * p[3] as u32 / 255);
            if a == 0 {
                out[0] = (p[2] as u32 * background / 255) as u8;
                out[1] = (p[1] as u32 * background / 255) as u8;
                out[2] = (p[0] as u32 * background / 255) as u8;
            } else {
                let tint = a * strength as u32 / 255;
                let shade = 255 - dim as u32 * (255 - a) / 255;
                for c in 0..3 {
                    let base = p[2 - c] as u32 * shade / 255;
                    out[c] = ((base * (255 - tint) + colors[c] * tint + 127) / 255) as u8;
                }
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
            out[3] = 255;
        }
    }
    let Some(mask) = mask else {
        return output;
    };
    if right == 0 || !contour {
        return output;
    }
    // Contours are prepared only around the mask bounds, never over the whole
    // virtual desktop. Bounded chamfer passes include holes and thin components.
    left = left.saturating_sub(7);
    top = top.saturating_sub(7);
    right = (right + 7).min(w);
    bottom = (bottom + 7).min(h);
    let (rw, rh) = (right - left, bottom - top);
    let mut distance = vec![24u8; rw * rh];
    let visible =
        |x: usize, y: usize| mask.alpha[y * w + x] > 32 && frame.rgba[(y * w + x) * 4 + 3] > 0;
    for y in top..bottom {
        for x in left..right {
            let inside = visible(x, y);
            if (x > 0 && visible(x - 1, y) != inside)
                || (x + 1 < w && visible(x + 1, y) != inside)
                || (y > 0 && visible(x, y - 1) != inside)
                || (y + 1 < h && visible(x, y + 1) != inside)
                || (inside && (x == 0 || y == 0 || x + 1 == w || y + 1 == h))
            {
                distance[(y - top) * rw + x - left] = 0;
            }
        }
    }
    for y in 0..rh {
        for x in 0..rw {
            let i = y * rw + x;
            if x > 0 {
                distance[i] = distance[i].min(distance[i - 1].saturating_add(3));
            }
            if y > 0 {
                distance[i] = distance[i].min(distance[i - rw].saturating_add(3));
                if x > 0 {
                    distance[i] = distance[i].min(distance[i - rw - 1].saturating_add(4));
                }
                if x + 1 < rw {
                    distance[i] = distance[i].min(distance[i - rw + 1].saturating_add(4));
                }
            }
        }
    }
    for y in (0..rh).rev() {
        for x in (0..rw).rev() {
            let i = y * rw + x;
            if x + 1 < rw {
                distance[i] = distance[i].min(distance[i + 1].saturating_add(3));
            }
            if y + 1 < rh {
                distance[i] = distance[i].min(distance[i + rw].saturating_add(3));
                if x > 0 {
                    distance[i] = distance[i].min(distance[i + rw - 1].saturating_add(4));
                }
                if x + 1 < rw {
                    distance[i] = distance[i].min(distance[i + rw + 1].saturating_add(4));
                }
            }
            let d = distance[i];
            let opacity = if d < 3 {
                220
            } else if d < 18 {
                ((18 - d) as u32).pow(2) / 7
            } else {
                0
            };
            if opacity > 0 {
                let p = ((y + top) * w + x + left) * 4;
                for (c, color) in [255u32, 221, 176].into_iter().enumerate() {
                    output[p + c] =
                        ((output[p + c] as u32 * (255 - opacity) + color * opacity) / 255) as u8;
                }
            }
        }
    }
    output
}
