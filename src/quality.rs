//! Alpha finishing is independent of binary selection geometry and preview styling.
use crate::Mask;
pub fn recover_colors(frame: &crate::Frame, mask: &Mask) -> crate::Frame {
    let mut output = frame.clone();
    if frame.width != mask.width || frame.height != mask.height {
        return output;
    }
    let w = frame.width as i32;
    let h = frame.height as i32;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let alpha = mask.alpha[i];
            if alpha == 0 || alpha == 255 {
                continue;
            }
            let mut fg = None;
            let mut bg = None;
            // Search increasing square rings, bounded independently of screen dimensions.
            for radius in 1i32..=12 {
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        if dx.abs() != radius && dy.abs() != radius {
                            continue;
                        }
                        let px = x + dx;
                        let py = y + dy;
                        if px < 0 || py < 0 || px >= w || py >= h {
                            continue;
                        }
                        let j = (py * w + px) as usize;
                        if mask.alpha[j] == 255 && fg.is_none() {
                            fg = Some(j);
                        }
                        if mask.alpha[j] == 0 && bg.is_none() {
                            bg = Some(j);
                        }
                    }
                }
                if fg.is_some() && bg.is_some() {
                    break;
                }
            }
            if let (Some(fg), Some(bg)) = (fg, bg) {
                let a = alpha as f32 / 255.;
                let regularization = 0.02;
                for c in 0..3 {
                    let observed = frame.rgba[i * 4 + c] as f32;
                    let background = frame.rgba[bg * 4 + c] as f32;
                    let foreground = frame.rgba[fg * 4 + c] as f32;
                    let recovered = (a * (observed - (1. - a) * background)
                        + regularization * foreground)
                        / (a * a + regularization);
                    output.rgba[i * 4 + c] = recovered.round().clamp(0., 255.) as u8;
                }
            }
        }
    }
    output
}
pub fn antialias(logits: &[f32], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    if logits.len() != w * h {
        return Vec::new();
    }
    let sample = |x: f32, y: f32| {
        let x = x.clamp(0.0, width.saturating_sub(1) as f32);
        let y = y.clamp(0.0, height.saturating_sub(1) as f32);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(w - 1);
        let y1 = (y0 + 1).min(h - 1);
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        (logits[y0 * w + x0] * (1.0 - fx) + logits[y0 * w + x1] * fx) * (1.0 - fy)
            + (logits[y1 * w + x0] * (1.0 - fx) + logits[y1 * w + x1] * fx) * fy
    };
    (0..w * h)
        .map(|i| {
            let px = i % w;
            let py = i / w;
            let positive = logits[i] > 0.;
            let uniform = (py.saturating_sub(1)..=(py + 1).min(h - 1)).all(|y| {
                (px.saturating_sub(1)..=(px + 1).min(w - 1))
                    .all(|x| (logits[y * w + x] > 0.) == positive)
            });
            if uniform {
                return if positive { 255 } else { 0 };
            }
            let x = (i % w) as f32;
            let y = (i / w) as f32;
            let mut covered = 0;
            for dy in [-0.375, -0.125, 0.125, 0.375] {
                for dx in [-0.375, -0.125, 0.125, 0.375] {
                    if sample(x + dx, y + dy) > 0.0 {
                        covered += 1;
                    }
                }
            }
            let alpha = (covered * 255 / 16) as u8;
            if logits[i] > 0.0 {
                alpha.max(128)
            } else {
                alpha.min(127)
            }
        })
        .collect()
}

pub fn trimap(mask: &Mask, band: u32) -> Vec<u8> {
    let w = mask.width as usize;
    let h = mask.height as usize;
    let limit = band.saturating_add(2).min(u16::MAX as u32) as u16;
    let mut foreground = vec![limit; w * h];
    let mut background = vec![limit; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if mask.alpha[i] > 127 {
                foreground[i] = 0;
                if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                    background[i] = 1;
                }
            } else {
                background[i] = 0;
            }
        }
    }
    for distance in [&mut foreground, &mut background] {
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if x > 0 {
                    distance[i] = distance[i].min(distance[i - 1].saturating_add(1));
                }
                if y > 0 {
                    distance[i] = distance[i].min(distance[i - w].saturating_add(1));
                }
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let i = y * w + x;
                if x + 1 < w {
                    distance[i] = distance[i].min(distance[i + 1].saturating_add(1));
                }
                if y + 1 < h {
                    distance[i] = distance[i].min(distance[i + w].saturating_add(1));
                }
            }
        }
    }
    (0..w * h)
        .map(|i| {
            if mask.alpha[i] > 127 {
                if background[i] > band as u16 {
                    255
                } else {
                    128
                }
            } else if foreground[i] <= band as u16 {
                128
            } else {
                0
            }
        })
        .collect()
}
