//! Deterministic annotated fixtures. Never use annotation geometry to generate proposals.
use anyhow::Result;
use objectlift::{Frame, Mask, inference::Segmenter, refinement};
use std::{path::Path, time::Instant};
fn metrics(mask: &Mask, truth: &image::GrayImage) -> (f64, f64) {
    let mut intersection = 0.;
    let mut union: f64 = 0.;
    let mut error = 0.;
    for (&a, b) in mask.alpha.iter().zip(truth.pixels()) {
        let b = b.0[0];
        if a > 127 && b > 127 {
            intersection += 1.;
        }
        if a > 127 || b > 127 {
            union += 1.;
        }
        error += (a as f64 - b as f64).abs() / 255.;
    }
    (
        intersection / union.max(1.),
        error / mask.alpha.len() as f64,
    )
}
fn main() -> Result<()> {
    let root = Path::new("artifacts/evaluation");
    let fixtures: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json"))?)?;
    let mut engine = Segmenter::load(Path::new("models"), true)?;
    let mut records = Vec::new();
    let mut precision = objectlift::advanced::Sam21::load(Path::new("models"), true)?;
    let mut matte = objectlift::advanced::Matte::load(Path::new("models"), true)?;
    for entry in fixtures.as_array().unwrap() {
        let image = image::open(root.join(entry["image"].as_str().unwrap()))?.to_rgba8();
        let truth = image::open(root.join(entry["alpha"].as_str().unwrap()))?.to_luma8();
        let frame = Frame::new(image.width(), image.height(), image.into_raw())
            .map_err(anyhow::Error::msg)?;
        let points = [(
            entry["point"][0].as_f64().unwrap() as f32,
            entry["point"][1].as_f64().unwrap() as f32,
            true,
        )];
        let start = Instant::now();
        let e = engine.encode(&frame)?;
        let (coarse, _) = engine.segment(&e, &points)?;
        let baseline_ms = start.elapsed().as_millis();
        let baseline = metrics(&coarse, &truth);
        let start = Instant::now();
        let result = refinement::refine(&mut engine, &frame, &coarse, &points, || false);
        let record = match result {
            Ok((mask, _)) => {
                let m = metrics(&mask, &truth);
                serde_json::json!({"id":entry["id"],"split":entry["split"],"category":entry["category"],"baseline_iou":baseline.0,"refined_iou":m.0,"baseline_alpha_mae":baseline.1,"refined_alpha_mae":m.1,"baseline_ms":baseline_ms,"additional_refinement_ms":start.elapsed().as_millis()})
            }
            Err(e) => serde_json::json!({"id":entry["id"],"error":e.to_string()}),
        };
        let mut record = record;
        let t = Instant::now();
        match precision.predict(&frame, &points) {
            Ok(pred) => {
                let m = metrics(&pred.mask, &truth);
                record["sam21_iou"] = m.0.into();
                record["sam21_ms"] = (t.elapsed().as_millis() as u64).into();
                match matte.finish(&frame, &pred.mask) {
                    Ok(mask) => {
                        let m = metrics(&mask, &truth);
                        record["matte_iou"] = m.0.into();
                        record["matte_alpha_mae"] = m.1.into();
                    }
                    Err(e) => record["matte_error"] = e.to_string().into(),
                }
            }
            Err(e) => record["sam21_error"] = e.to_string().into(),
        }
        println!("{record}");
        records.push(record);
    }
    std::fs::write(
        "artifacts/evaluation/results.json",
        serde_json::to_vec_pretty(&records)?,
    )?;
    Ok(())
}
