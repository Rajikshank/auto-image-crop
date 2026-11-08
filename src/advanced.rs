//! Optional precision/matting adapters. Tensor contracts are validated against pinned assets.
use crate::{Frame, Mask};
use anyhow::{Context, Result, bail};
use ort::{ep, session::Session, value::Tensor};
use std::path::Path;

pub struct Prediction {
    pub mask: Mask,
    pub logits: Vec<f32>,
    pub score: f32,
}
pub fn session(root: &Path, path: &str, gpu: bool) -> Result<Session> {
    verify_asset(root, path)?;
    ort::init_from(root.join("runtime/runtimes/win-x64/native/onnxruntime.dll"))?
        .with_telemetry(false)
        .commit();
    let build = |gpu: bool| -> Result<Session> {
        let mut builder = Session::builder()?
            .with_intra_threads(4)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .with_parallel_execution(false)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .with_memory_pattern(false)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if gpu {
            builder = builder
                .with_execution_providers([ep::DirectML::default()
                    .with_device_id(crate::device::preferred().map_or(0, |a| a.index))
                    .build()
                    .error_on_failure()])
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        Ok(builder.commit_from_file(root.join(path))?)
    };
    build(gpu).context("Model execution provider unavailable; select CPU-only in the tray to retry")
}

fn verify_asset(root: &Path, path: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    #[derive(serde::Deserialize)]
    struct Asset {
        file: String,
        sha256: String,
    }
    let assets: Vec<Asset> =
        serde_json::from_str(include_str!("../scripts/upgrade-models.lock.json"))?;
    let mut found = false;
    for asset in assets.into_iter().filter(|a| {
        a.file == path || a.file == format!("{path}_data") || a.file == format!("{path}.data")
    }) {
        found = true;
        let bytes = std::fs::read(root.join(&asset.file)).with_context(|| {
            format!(
                "Missing {}. Install the optional quality models; see README.",
                asset.file
            )
        })?;
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if digest != asset.sha256 {
            bail!("Model integrity check failed for {}", asset.file);
        }
    }
    if !found {
        bail!("Unpinned model asset: {path}");
    }
    Ok(())
}

pub struct Sam21 {
    encoder: Session,
    decoder: Session,
}

pub struct Matte {
    model: Session,
}
impl Matte {
    pub fn load(root: &Path, gpu: bool) -> Result<Self> {
        Ok(Self {
            model: session(root, "vitmatte/model.onnx", gpu)?,
        })
    }
    pub fn finish(&mut self, frame: &Frame, mask: &Mask) -> Result<Mask> {
        self.finish_detail(frame, mask, None, 6., &|| false)
    }
    pub fn finish_detail(
        &mut self,
        frame: &Frame,
        mask: &Mask,
        edge: Option<&[(f32, f32)]>,
        radius: f32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Mask> {
        if frame.width != mask.width || frame.height != mask.height {
            bail!("Matting dimensions differ");
        }
        // Preserve native pixels. Bound inference cost with a selection ROI; do not resize fine edges.
        let dense_edge = if let Some(points) = edge {
            if points.is_empty()
                || !radius.is_finite()
                || !(0.25..=256.).contains(&radius)
                || points.iter().any(|p| {
                    !p.0.is_finite()
                        || !p.1.is_finite()
                        || p.0 < 0.
                        || p.1 < 0.
                        || p.0 >= frame.width as f32
                        || p.1 >= frame.height as f32
                })
            {
                bail!("Invalid edge stroke");
            }
            let mut dense = vec![points[0]];
            for pair in points.windows(2) {
                let distance =
                    ((pair[1].0 - pair[0].0).powi(2) + (pair[1].1 - pair[0].1).powi(2)).sqrt();
                let steps = (distance / (radius * 0.5).max(0.5)).ceil() as usize;
                if dense.len() + steps > 65536 {
                    bail!("Refine the edge with a shorter stroke");
                }
                for step in 1..=steps {
                    let t = step as f32 / steps as f32;
                    dense.push((
                        pair[0].0 + (pair[1].0 - pair[0].0) * t,
                        pair[0].1 + (pair[1].1 - pair[0].1) * t,
                    ));
                }
            }
            Some(dense)
        } else {
            None
        };
        let edge = dense_edge.as_deref();
        if cancelled() {
            bail!("Refinement cancelled");
        }
        let region = if let Some(points) = edge {
            let first = points.first().context("Draw over an edge first")?;
            let pad = (radius + 40.).ceil() as u32;
            let x = (first.0.max(0.) as u32).saturating_sub(pad);
            let y = (first.1.max(0.) as u32).saturating_sub(pad);
            let mut r = crate::roi::Region {
                x,
                y,
                width: (pad * 2 + 1).min(frame.width - x),
                height: (pad * 2 + 1).min(frame.height - y),
            };
            let p: Vec<_> = points.iter().map(|&(x, y)| (x, y, true)).collect();
            r = r
                .including(&p, frame.width, frame.height)
                .expand(frame.width, frame.height);
            r
        } else {
            crate::roi::Region::around_mask(mask, 0.08).context("Empty matting selection")?
        };
        if region.width > 1024 || region.height > 1024 {
            if region.width as u64 * region.height as u64 > 16_000_000 {
                bail!("Refine a smaller region (16 megapixel matting budget)");
            }
            let mut sum = vec![0f32; (region.width * region.height) as usize];
            let mut weight = vec![0f32; sum.len()];
            for tile in crate::tiles::regions(region, 1024, 64) {
                if cancelled() {
                    bail!("Refinement cancelled");
                }
                let image = tile.crop(frame).map_err(anyhow::Error::msg)?;
                let alpha = (tile.y..tile.y + tile.height)
                    .flat_map(|y| {
                        let i = (y * mask.width + tile.x) as usize;
                        mask.alpha[i..i + tile.width as usize].iter().copied()
                    })
                    .collect();
                let local =
                    Mask::new(tile.width, tile.height, alpha).map_err(anyhow::Error::msg)?;
                if local.area() == 0 && edge.is_none() {
                    continue;
                }
                let local_points: Vec<_> = edge
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|&(x, y)| tile.local_point((x, y, true)).map(|p| (p.0, p.1)))
                    .collect();
                let output = self.finish_detail(
                    &image,
                    &local,
                    if edge.is_some() && !local_points.is_empty() {
                        Some(&local_points)
                    } else {
                        None
                    },
                    radius,
                    cancelled,
                )?;
                for y in 0..tile.height {
                    for x in 0..tile.width {
                        let i = ((tile.y - region.y + y) * region.width + tile.x - region.x + x)
                            as usize;
                        let w = ((x + 1)
                            .min(tile.width - x)
                            .min(y + 1)
                            .min(tile.height - y)
                            .min(32)) as f32;
                        sum[i] += output.alpha[(y * tile.width + x) as usize] as f32 * w;
                        weight[i] += w;
                    }
                }
            }
            let mut output = mask.clone();
            for y in 0..region.height {
                for x in 0..region.width {
                    let i = (y * region.width + x) as usize;
                    if weight[i] > 0. {
                        output.alpha[((region.y + y) * mask.width + region.x + x) as usize] =
                            (sum[i] / weight[i]).round().clamp(0., 255.) as u8;
                    }
                }
            }
            return Ok(output);
        }
        let crop = region.crop(frame).map_err(anyhow::Error::msg)?;
        let mut local = Vec::with_capacity((region.width * region.height) as usize);
        for y in region.y..region.y + region.height {
            let start = (y * mask.width + region.x) as usize;
            local.extend_from_slice(&mask.alpha[start..start + region.width as usize]);
        }
        let local = Mask::new(region.width, region.height, local).map_err(anyhow::Error::msg)?;
        let mut trimap = if edge.is_some() {
            local
                .alpha
                .iter()
                .map(|&a| match a {
                    0 => 0,
                    255 => 255,
                    _ => 128,
                })
                .collect()
        } else {
            crate::quality::trimap(&local, 6)
        };
        let mut affected = vec![false; local.alpha.len()];
        if let Some(points) = edge {
            for &(x, y) in points {
                let x = x - region.x as f32;
                let y = y - region.y as f32;
                let r = radius;
                for py in ((y - r).floor() as i32).max(0)
                    ..=((y + r).ceil() as i32).min(region.height as i32 - 1)
                {
                    for px in ((x - r).floor() as i32).max(0)
                        ..=((x + r).ceil() as i32).min(region.width as i32 - 1)
                    {
                        if (px as f32 - x).powi(2) + (py as f32 - y).powi(2) <= r * r {
                            let i = (py as u32 * region.width + px as u32) as usize;
                            trimap[i] = 128;
                            affected[i] = true;
                        }
                    }
                }
            }
        }
        let width = region.width.div_ceil(32) * 32;
        let height = region.height.div_ceil(32) * 32;
        let area = (width * height) as usize;
        let mut input = vec![0f32; 4 * area];
        for y in 0..region.height {
            for x in 0..region.width {
                let i = (y * region.width + x) as usize;
                let j = (y * width + x) as usize;
                for c in 0..3 {
                    input[c * area + j] = (crop.rgba[i * 4 + c] as f32 / 255. - 0.5) / 0.5;
                }
                input[3 * area + j] = trimap[i] as f32 / 255.;
            }
        }
        let output=self.model.run(ort::inputs!["pixel_values"=>Tensor::from_array(([1,4,height as usize,width as usize],input))?])?;
        let (_, alpha) = output["alphas"].try_extract_tensor::<f32>()?;
        if alpha.len() != area || alpha.iter().any(|v| !v.is_finite()) {
            bail!("Invalid matting output");
        }
        let mut finished = local.clone();
        for y in 0..region.height {
            for x in 0..region.width {
                let i = (y * region.width + x) as usize;
                if edge.is_some() && !affected[i] {
                    continue;
                }
                finished.alpha[i] = match trimap[i] {
                    0 => 0,
                    255 => 255,
                    _ => (alpha[(y * width + x) as usize].clamp(0., 1.) * 255.).round() as u8,
                };
            }
        }
        let mut output = mask.clone();
        for y in 0..region.height {
            let dst = ((region.y + y) * frame.width + region.x) as usize;
            let src = (y * region.width) as usize;
            output.alpha[dst..dst + region.width as usize]
                .copy_from_slice(&finished.alpha[src..src + region.width as usize]);
        }
        Ok(output)
    }
}

pub struct Detector {
    model: Session,
}
impl Detector {
    pub fn load(root: &Path, gpu: bool) -> Result<Self> {
        Ok(Self {
            model: session(root, "rfdetr/model.onnx", gpu)?,
        })
    }
    pub fn discover(&mut self, frame: &Frame) -> Result<Vec<Mask>> {
        let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
            .context("Invalid discovery frame")?;
        let image =
            image::imageops::resize(&image, 384, 384, image::imageops::FilterType::Triangle);
        let mut input = vec![0f32; 3 * 384 * 384];
        let mean = [0.485, 0.456, 0.406];
        let std = [0.229, 0.224, 0.225];
        for (i, p) in image.pixels().enumerate() {
            for c in 0..3 {
                input[c * 384 * 384 + i] = (p[c] as f32 / 255. - mean[c]) / std[c];
            }
        }
        let out = self
            .model
            .run(ort::inputs!["pixel_values"=>Tensor::from_array(([1,3,384,384],input))?])?;
        let (shape, logits) = out["logits"].try_extract_tensor::<f32>()?;
        let (mask_shape, masks) = out["pred_masks"].try_extract_tensor::<f32>()?;
        if shape.len() != 3 || mask_shape.len() != 4 || shape[1] != mask_shape[1] {
            bail!("Detector output contract mismatch");
        }
        let count = shape[1] as usize;
        let classes = shape[2] as usize;
        let h = mask_shape[2] as u32;
        let w = mask_shape[3] as u32;
        let area = (w * h) as usize;
        let mut ranking: Vec<_> = (0..count)
            .map(|i| {
                (
                    i,
                    logits[i * classes..(i + 1) * classes]
                        .iter()
                        .copied()
                        .fold(f32::NEG_INFINITY, f32::max),
                )
            })
            .filter(|p| p.1 > 0.8473)
            .collect(); // sigmoid >= .70
        ranking.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut candidates = crate::Candidates::default();
        for (i, _) in ranking
            .into_iter()
            .take((64_000_000usize / (frame.width as usize * frame.height as usize)).clamp(1, 24))
        {
            let logits = resize_logits(
                &masks[i * area..(i + 1) * area],
                w,
                h,
                frame.width,
                frame.height,
            );
            let mask = Mask::new(
                frame.width,
                frame.height,
                logits
                    .iter()
                    .map(|&v| if v > 0. { 255 } else { 0 })
                    .collect(),
            )
            .map_err(anyhow::Error::msg)?;
            let fraction = mask.area() as f32 / (frame.width * frame.height) as f32;
            if (0.0001..0.90).contains(&fraction) {
                candidates.insert(mask);
            }
        }
        Ok(candidates.into_masks())
    }
}
impl Sam21 {
    pub fn refine(
        &mut self,
        frame: &Frame,
        points: &[(f32, f32, bool)],
        cancelled: impl Fn() -> bool,
    ) -> Result<Prediction> {
        let mut full = self.predict(frame, points)?;
        let Some(mut region) = crate::roi::Region::around_mask(&full.mask, 0.25) else {
            return Ok(full);
        };
        region = region.including(points, frame.width, frame.height);
        // Full-screen objects already use most encoder pixels. Avoid an extra encoder pass.
        if region.width as u64 * region.height as u64 * 100
            > frame.width as u64 * frame.height as u64 * 60
        {
            return Ok(full);
        }
        for attempt in 0..3 {
            if cancelled() {
                bail!("Selection cancelled");
            }
            let crop = region.crop(frame).map_err(anyhow::Error::msg)?;
            let local: Vec<_> = points
                .iter()
                .filter_map(|&p| region.local_point(p))
                .collect();
            if local.len() != points.len() {
                bail!("Crop excludes correction points");
            }
            let prediction = self.predict(&crop, &local)?;
            if region.touches_boundary(&prediction.mask) && attempt < 2 {
                region = region.expand(frame.width, frame.height);
                continue;
            }
            let restored = region
                .restore(&prediction.mask, frame.width, frame.height)
                .map_err(anyhow::Error::msg)?;
            let mut intersection = 0u64;
            let mut union = 0u64;
            for (&a, &b) in restored.alpha.iter().zip(&full.mask.alpha) {
                if a > 127 && b > 127 {
                    intersection += 1;
                }
                if a > 127 || b > 127 {
                    union += 1;
                }
            }
            // Model confidence alone cannot justify large semantic geometry changes.
            if prediction.score > full.score + 0.02
                && intersection as f32 / union.max(1) as f32 > 0.75
            {
                let mut logits = vec![-32.; (frame.width * frame.height) as usize];
                for y in 0..region.height {
                    let src = (y * region.width) as usize;
                    let dst = ((y + region.y) * frame.width + region.x) as usize;
                    logits[dst..dst + region.width as usize]
                        .copy_from_slice(&prediction.logits[src..src + region.width as usize]);
                }
                full = Prediction {
                    mask: restored,
                    logits,
                    score: prediction.score,
                };
            }
            break;
        }
        full.mask.alpha = crate::quality::antialias(&full.logits, frame.width, frame.height);
        Ok(full)
    }
    pub fn load(root: &Path, gpu: bool) -> Result<Self> {
        Ok(Self {
            encoder: session(root, "sam21/vision_encoder.onnx", gpu)?,
            decoder: session(root, "sam21/point_decoder.onnx", gpu)?,
        })
    }
    pub fn predict(&mut self, frame: &Frame, points: &[(f32, f32, bool)]) -> Result<Prediction> {
        if points.is_empty() {
            bail!("A foreground point is required");
        }
        let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
            .context("Invalid frame")?;
        let resized =
            image::imageops::resize(&image, 1024, 1024, image::imageops::FilterType::Triangle);
        let mut input = vec![0f32; 3 * 1024 * 1024];
        let mean = [0.485, 0.456, 0.406];
        let std = [0.229, 0.224, 0.225];
        for (i, p) in resized.pixels().enumerate() {
            for c in 0..3 {
                input[c * 1024 * 1024 + i] = (p[c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
        let output = self
            .encoder
            .run(ort::inputs!["pixel_values"=>Tensor::from_array(([1,3,1024,1024],input))?])?;
        let features: Vec<Vec<f32>> = (0..3)
            .map(|i| {
                output[i]
                    .try_extract_tensor::<f32>()
                    .map(|(_, v)| v.to_vec())
            })
            .collect::<std::result::Result<_, _>>()?;
        drop(output);
        let coords: Vec<f32> = points
            .iter()
            .flat_map(|&(x, y, _)| {
                [
                    x * 1024.0 / frame.width as f32,
                    y * 1024.0 / frame.height as f32,
                ]
            })
            .collect();
        let labels: Vec<i64> = points.iter().map(|p| if p.2 { 1 } else { 0 }).collect();
        let out = self.decoder.run(ort::inputs![
            "input_points"=>Tensor::from_array(([1,1,points.len(),2],coords))?,
            "input_labels"=>Tensor::from_array(([1,1,points.len()],labels))?,
            "image_embeddings.0"=>Tensor::from_array(([1,32,256,256],features[0].clone()))?,
            "image_embeddings.1"=>Tensor::from_array(([1,64,128,128],features[1].clone()))?,
            "image_embeddings.2"=>Tensor::from_array(([1,256,64,64],features[2].clone()))?
        ])?;
        let (_, scores) = out["iou_scores"].try_extract_tensor::<f32>()?;
        let (shape, data) = out["pred_masks"].try_extract_tensor::<f32>()?;
        let h = shape[shape.len() - 2] as u32;
        let w = shape[shape.len() - 1] as u32;
        let area = (w * h) as usize;
        if scores.is_empty() || data.len() != scores.len() * area {
            bail!("SAM 2.1 output shape mismatch: {shape:?}");
        }
        let resized_logits: Vec<Vec<f32>> = (0..scores.len())
            .map(|i| {
                resize_logits(
                    &data[i * area..(i + 1) * area],
                    w,
                    h,
                    frame.width,
                    frame.height,
                )
            })
            .collect();
        let violations = |i: usize| {
            points
                .iter()
                .filter(|&&(x, y, pos)| {
                    let x = (x.max(0.) as u32).min(frame.width - 1);
                    let y = (y.max(0.) as u32).min(frame.height - 1);
                    (resized_logits[i][(y * frame.width + x) as usize] > 0.) != pos
                })
                .count()
        };
        let best = (0..scores.len())
            .min_by(|&a, &b| {
                violations(a)
                    .cmp(&violations(b))
                    .then_with(|| scores[b].total_cmp(&scores[a]))
            })
            .unwrap();
        if violations(best) > 0 {
            bail!("Precision model could not satisfy all corrections");
        }
        let logits = resized_logits.into_iter().nth(best).unwrap();
        let alpha = logits
            .iter()
            .map(|&v| if v > 0.0 { 255 } else { 0 })
            .collect();
        Ok(Prediction {
            mask: Mask::new(frame.width, frame.height, alpha).map_err(anyhow::Error::msg)?,
            logits,
            score: scores[best],
        })
    }
}

pub fn resize_logits(input: &[f32], w: u32, h: u32, width: u32, height: u32) -> Vec<f32> {
    let mut output = vec![0.; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let sx = ((x as f32 + 0.5) * w as f32 / width as f32 - 0.5)
                .clamp(0., w.saturating_sub(1) as f32);
            let sy = ((y as f32 + 0.5) * h as f32 / height as f32 - 0.5)
                .clamp(0., h.saturating_sub(1) as f32);
            let x0 = sx.floor() as u32;
            let y0 = sy.floor() as u32;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;
            output[(y * width + x) as usize] = (input[(y0 * w + x0) as usize] * (1. - fx)
                + input[(y0 * w + x1) as usize] * fx)
                * (1. - fy)
                + (input[(y1 * w + x0) as usize] * (1. - fx) + input[(y1 * w + x1) as usize] * fx)
                    * fy;
        }
    }
    output
}
