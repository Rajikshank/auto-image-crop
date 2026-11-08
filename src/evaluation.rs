//! Declared metric conventions: binary boundary tolerance 2 px; union boundary alpha band 6 px.
use crate::Mask;
#[derive(serde::Serialize)]
pub struct Metrics {
    pub iou: f64,
    pub boundary_f1: f64,
    pub boundary_alpha_mae: f64,
    pub sad: f64,
}
pub fn measure(prediction: &Mask, truth: &Mask) -> Result<Metrics, String> {
    if (prediction.width, prediction.height) != (truth.width, truth.height) {
        return Err("Evaluation dimensions differ".into());
    }
    let w = truth.width as usize;
    let h = truth.height as usize;
    let edges = |mask: &Mask| {
        let mut result = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let fg = mask.alpha[i] > 127;
                result[i] = fg
                    && ((x == 0 || y == 0 || x + 1 == w || y + 1 == h)
                        || (x > 0 && mask.alpha[i - 1] <= 127)
                        || (x + 1 < w && mask.alpha[i + 1] <= 127)
                        || (y > 0 && mask.alpha[i - w] <= 127)
                        || (y + 1 < h && mask.alpha[i + w] <= 127));
            }
        }
        result
    };
    let a = edges(prediction);
    let b = edges(truth);
    let dilate = |map: &[bool], radius: i32| {
        let mut result = vec![false; w * h];
        for (i, &edge) in map.iter().enumerate() {
            if !edge {
                continue;
            }
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let px = x + dx;
                    let py = y + dy;
                    if dx * dx + dy * dy <= radius * radius
                        && px >= 0
                        && py >= 0
                        && px < w as i32
                        && py < h as i32
                    {
                        result[py as usize * w + px as usize] = true;
                    }
                }
            }
        }
        result
    };
    let a2 = dilate(&a, 2);
    let b2 = dilate(&b, 2);
    let a6 = dilate(&a, 6);
    let b6 = dilate(&b, 6);
    let (mut intersection, mut union, mut na, mut nb, mut matched_a, mut matched_b) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut total_error = 0.;
    let mut band_error = 0.;
    let mut band_pixels = 0usize;
    for i in 0..w * h {
        let pa = prediction.alpha[i];
        let ta = truth.alpha[i];
        if pa > 127 && ta > 127 {
            intersection += 1;
        }
        if pa > 127 || ta > 127 {
            union += 1;
        }
        if a[i] {
            na += 1;
            if b2[i] {
                matched_a += 1;
            }
        }
        if b[i] {
            nb += 1;
            if a2[i] {
                matched_b += 1;
            }
        }
        let error = (pa as f64 - ta as f64).abs() / 255.;
        total_error += error;
        if a6[i] || b6[i] {
            band_error += error;
            band_pixels += 1;
        }
    }
    let precision = matched_a as f64 / na.max(1) as f64;
    let recall = matched_b as f64 / nb.max(1) as f64;
    Ok(Metrics {
        iou: if union == 0 {
            1.
        } else {
            intersection as f64 / union as f64
        },
        boundary_f1: if na + nb == 0 {
            1.
        } else {
            2. * precision * recall / (precision + recall).max(f64::EPSILON)
        },
        boundary_alpha_mae: band_error / band_pixels.max(1) as f64,
        sad: total_error / 1000.,
    })
}
