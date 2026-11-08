//! A bounded one-ROI SAM feature cache. Low-resolution priors never cross image/ROI identities.
use crate::{
    Frame, Mask,
    advanced::{Prediction, resize_logits, session},
};
use anyhow::{Result, bail};
use ort::{session::Session, value::Tensor};
use sha2::{Digest, Sha256};
use std::path::Path;
struct Cache {
    key: Vec<u8>,
    features: Vec<Vec<f32>>,
    prior: Vec<f32>,
    points: Vec<(f32, f32, bool)>,
    bbox: Option<[f32; 4]>,
}
pub struct IterativeSam {
    encoder: Session,
    decoder: Session,
    cache: Option<Cache>,
    pub encoder_runs: u64,
    pub prior_runs: u64,
    scope: Option<(u64, crate::roi::Region)>,
}
impl IterativeSam {
    pub fn load(root: &Path, gpu: bool) -> Result<Self> {
        Ok(Self {
            encoder: session(root, "sam21/vision_encoder.onnx", gpu)?,
            decoder: session(root, "sam21/interactive_decoder.onnx", gpu)?,
            cache: None,
            encoder_runs: 0,
            prior_runs: 0,
            scope: None,
        })
    }
    pub fn set_context(&mut self, session_id: u64, region: crate::roi::Region) {
        let scope = Some((session_id, region));
        if self.scope != scope {
            self.cache = None;
            self.scope = scope;
        }
    }
    pub fn predict(
        &mut self,
        frame: &Frame,
        points: &[(f32, f32, bool)],
        bbox: Option<[f32; 4]>,
    ) -> Result<Prediction> {
        if points.is_empty()
            || points.len() > 64
            || points.iter().any(|&(x, y, _)| {
                !x.is_finite()
                    || !y.is_finite()
                    || x < 0.
                    || y < 0.
                    || x >= frame.width as f32
                    || y >= frame.height as f32
            })
        {
            bail!("Corrective points must be inside the image (maximum 64)");
        }
        let mut digest = Sha256::new();
        digest.update(frame.width.to_le_bytes());
        digest.update(frame.height.to_le_bytes());
        digest.update(&frame.rgba);
        let key = digest.finalize().to_vec();
        if self.cache.as_ref().is_none_or(|c| c.key != key) {
            let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
                .ok_or_else(|| anyhow::anyhow!("Invalid image"))?;
            let image =
                image::imageops::resize(&image, 1024, 1024, image::imageops::FilterType::Triangle);
            let mut input = vec![0f32; 3 * 1024 * 1024];
            for (i, p) in image.pixels().enumerate() {
                for c in 0..3 {
                    input[c * 1024 * 1024 + i] =
                        (p[c] as f32 / 255. - [0.485, 0.456, 0.406][c]) / [0.229, 0.224, 0.225][c];
                }
            }
            let out = self
                .encoder
                .run(ort::inputs!["pixel_values"=>Tensor::from_array(([1,3,1024,1024],input))?])?;
            let features = (0..3)
                .map(|i| out[i].try_extract_tensor::<f32>().map(|(_, v)| v.to_vec()))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            self.cache = Some(Cache {
                key,
                features,
                prior: vec![0.; 256 * 256],
                points: Vec::new(),
                bbox: None,
            });
            self.encoder_runs += 1;
        }
        let cache = self.cache.as_mut().unwrap();
        let use_prior = cache.bbox == bbox
            && !cache.points.is_empty()
            && points.len() > cache.points.len()
            && points.starts_with(&cache.points);
        let mut coords: Vec<f32> = points
            .iter()
            .flat_map(|p| {
                [
                    p.0 * 1024. / frame.width as f32,
                    p.1 * 1024. / frame.height as f32,
                ]
            })
            .collect();
        let mut labels: Vec<i64> = points.iter().map(|p| if p.2 { 1 } else { 0 }).collect();
        if let Some([x, y, r, b]) = bbox {
            if ![x, y, r, b].iter().all(|v| v.is_finite())
                || x < 0.
                || y < 0.
                || r > frame.width as f32
                || b > frame.height as f32
                || x >= r
                || y >= b
            {
                bail!("Invalid lasso box");
            }
            coords.extend([
                x * 1024. / frame.width as f32,
                y * 1024. / frame.height as f32,
                r * 1024. / frame.width as f32,
                b * 1024. / frame.height as f32,
            ]);
            labels.extend([2, 3]);
        }
        for attempt in 0..2 {
            let has_prior = use_prior && attempt == 0;
            if has_prior {
                self.prior_runs += 1;
            }
            let n = labels.len();
            let out=self.decoder.run(ort::inputs![
                "input_points"=>Tensor::from_array(([1,1,n,2],coords.clone()))?,"input_labels"=>Tensor::from_array(([1,1,n],labels.clone()))?,
                "mask_input"=>Tensor::from_array(([1,1,256,256],cache.prior.clone()))?,"has_mask_input"=>Tensor::from_array(([1],vec![if has_prior{1f32}else{0.}]))?,
                "image_embeddings.0"=>Tensor::from_array(([1,32,256,256],cache.features[0].clone()))?,"image_embeddings.1"=>Tensor::from_array(([1,64,128,128],cache.features[1].clone()))?,"image_embeddings.2"=>Tensor::from_array(([1,256,64,64],cache.features[2].clone()))?
            ])?;
            let (_, scores) = out["iou_scores"].try_extract_tensor::<f32>()?;
            let (_, masks) = out["pred_masks"].try_extract_tensor::<f32>()?;
            if masks.len() != scores.len() * 256 * 256
                || scores.is_empty()
                || masks.iter().any(|v| !v.is_finite())
            {
                bail!("Invalid iterative mask output");
            }
            let logits: Vec<_> = masks
                .chunks_exact(256 * 256)
                .map(|m| resize_logits(m, 256, 256, frame.width, frame.height))
                .collect();
            let violations = |i: usize| {
                points
                    .iter()
                    .filter(|&&(x, y, pos)| {
                        (logits[i][y as usize * frame.width as usize + x as usize] > 0.) != pos
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
                if has_prior {
                    continue;
                }
                bail!("AI could not satisfy these points; exact Add/Remove remains available");
            }
            cache.prior = masks[best * 256 * 256..(best + 1) * 256 * 256].to_vec();
            cache.points = points.to_vec();
            cache.bbox = bbox;
            let logits = logits.into_iter().nth(best).unwrap();
            let alpha = crate::quality::antialias(&logits, frame.width, frame.height);
            return Ok(Prediction {
                mask: Mask::new(frame.width, frame.height, alpha).map_err(anyhow::Error::msg)?,
                logits,
                score: scores[best],
            });
        }
        bail!("AI correction failed")
    }
}
