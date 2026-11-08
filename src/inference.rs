//! Local promptable segmentation. Models are explicitly installed; inference never uses the network.
use crate::{Frame, Mask};
use anyhow::{Context, Result, bail};
use ort::{ep, session::Session, value::Tensor};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Instant};

pub struct Segmenter {
    encoder: Session,
    decoder: Session,
    pub provider: String,
}
pub struct Embedding {
    data: Vec<f32>,
    pub width: u32,
    pub height: u32,
    pub resized: (u32, u32),
}

impl Segmenter {
    pub fn load(root: &Path, gpu: bool) -> Result<Self> {
        for (name, expected) in [
            (
                "mobile_sam_image_encoder.onnx",
                "580f5fb648ea1062c0aabc26217aed56921985f03f0cbbd852bba81d760cc749",
            ),
            (
                "sam_mask_decoder_multi.onnx",
                "8976b90a87ba50a6a72217a5ff994f7d25ce16f2229fcc1ed259e1294c622ffe",
            ),
        ] {
            let bytes = std::fs::read(root.join(name))
                .with_context(|| format!("Missing {name}. Run scripts/setup-models.ps1"))?;
            let actual: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if actual != expected {
                bail!(
                    "Model integrity check failed for {name}. Run scripts/setup-models.ps1 to repair."
                );
            }
        }
        let runtime = root.join("runtime/runtimes/win-x64/native/onnxruntime.dll");
        if !runtime.exists() {
            bail!("Runtime missing. Run scripts/setup-models.ps1 first.");
        }
        ort::init_from(runtime)?
            .with_name("ObjectLift")
            .with_telemetry(false)
            .commit();
        let make = |name: &str, gpu: bool| -> Result<Session> {
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
            Ok(builder.commit_from_file(root.join(name))?)
        };
        let (encoder, decoder, provider) = if gpu {
            match (
                make("mobile_sam_image_encoder.onnx", true),
                make("sam_mask_decoder_multi.onnx", true),
            ) {
                (Ok(e), Ok(d)) => (e, d, "DirectML"),
                (e, d) => {
                    eprintln!("GPU unavailable ({:?}; {:?}); using CPU", e.err(), d.err());
                    (
                        make("mobile_sam_image_encoder.onnx", false)?,
                        make("sam_mask_decoder_multi.onnx", false)?,
                        "CPU",
                    )
                }
            }
        } else {
            (
                make("mobile_sam_image_encoder.onnx", false)?,
                make("sam_mask_decoder_multi.onnx", false)?,
                "CPU",
            )
        };
        Ok(Self {
            encoder,
            decoder,
            provider: provider.into(),
        })
    }
    pub fn encode(&mut self, frame: &Frame) -> Result<Embedding> {
        let start = Instant::now();
        let scale = 1024.0 / frame.width.max(frame.height) as f32;
        let w = (frame.width as f32 * scale + 0.5) as u32;
        let h = (frame.height as f32 * scale + 0.5) as u32;
        let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
            .context("Invalid frame")?;
        let resized = image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle);
        // This export includes HWC normalization and right/bottom padding in its graph.
        let input: Vec<f32> = resized
            .pixels()
            .flat_map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect();
        let outputs = self.encoder.run(
            ort::inputs!["input_image"=>Tensor::from_array(([h as usize,w as usize,3],input))?],
        )?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>()?;
        eprintln!("embedding_ms={}", start.elapsed().as_millis());
        Ok(Embedding {
            data: data.to_vec(),
            width: frame.width,
            height: frame.height,
            resized: (w, h),
        })
    }
    pub fn segment(
        &mut self,
        embedding: &Embedding,
        points: &[(f32, f32, bool)],
    ) -> Result<(Mask, f32)> {
        let prediction = self.predict(embedding, points)?;
        Ok((prediction.mask, prediction.score))
    }
    pub fn predict(
        &mut self,
        embedding: &Embedding,
        points: &[(f32, f32, bool)],
    ) -> Result<crate::advanced::Prediction> {
        if points.is_empty() {
            bail!("Select a point first");
        }
        if points.len() > 64
            || points.iter().any(|&(x, y, _)| {
                !x.is_finite()
                    || !y.is_finite()
                    || x < 0.0
                    || y < 0.0
                    || x >= embedding.width as f32
                    || y >= embedding.height as f32
            })
        {
            bail!("Selection points must be finite and inside the captured image (maximum 64)");
        }
        let mut coords = Vec::new();
        let mut labels = Vec::new();
        for &(x, y, positive) in points {
            coords.extend([
                x * embedding.resized.0 as f32 / embedding.width as f32,
                y * embedding.resized.1 as f32 / embedding.height as f32,
            ]);
            labels.push(if positive { 1.0f32 } else { 0.0 });
        }
        coords.extend([0.0, 0.0]);
        labels.push(-1.0);
        let n = labels.len();
        let out=self.decoder.run(ort::inputs![
            "image_embeddings"=>Tensor::from_array(([1,256,64,64],embedding.data.clone()))?,
            "point_coords"=>Tensor::from_array(([1,n,2],coords))?,
            "point_labels"=>Tensor::from_array(([1,n],labels))?,
            "mask_input"=>Tensor::from_array(([1,1,256,256],vec![0f32;256*256]))?,
            "has_mask_input"=>Tensor::from_array(([1],vec![0f32]))?,
            "orig_im_size"=>Tensor::from_array(([2],vec![embedding.height as f32,embedding.width as f32]))?
        ])?;
        let (shape, scores) = out["iou_predictions"].try_extract_tensor::<f32>()?;
        let (_, masks) = out["masks"].try_extract_tensor::<f32>()?;
        let count = *shape.last().context("Missing scores")? as usize;
        let size = embedding.width as usize * embedding.height as usize;
        if masks.len() != size * count {
            bail!("Unexpected decoder mask dimensions");
        }
        // Prefer masks that actually satisfy the user's corrections over a larger mask
        // with a higher predicted IoU. Include the dedicated multi-prompt token.
        let first = if points.len() == 1 && count > 1 { 1 } else { 0 };
        let violations = |candidate: usize| {
            points
                .iter()
                .filter(|&&(x, y, positive)| {
                    let index = y as usize * embedding.width as usize + x as usize;
                    (masks[candidate * size + index] > 0.0) != positive
                })
                .count()
        };
        let best = (first..count)
            .min_by(|&a, &b| {
                violations(a)
                    .cmp(&violations(b))
                    .then_with(|| scores[b].total_cmp(&scores[a]))
            })
            .context("No model masks")?;
        if violations(best) > 0 {
            bail!(
                "The model could not satisfy these corrections. Try a point further inside the desired region."
            );
        }
        let logits = masks[best * size..(best + 1) * size].to_vec();
        let alpha = crate::quality::antialias(&logits, embedding.width, embedding.height);
        Ok(crate::advanced::Prediction {
            mask: Mask::new(embedding.width, embedding.height, alpha)
                .map_err(anyhow::Error::msg)?,
            logits,
            score: scores[best],
        })
    }
}
