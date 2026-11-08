//! Fixed regression suite. Manual repair strokes are explicitly fixture-informed, not AI predictions.
use anyhow::Result;
use objectlift::{
    Frame, Mask, advanced::Matte, editor::Editor, evaluation::measure, inference::Segmenter,
    iterative::IterativeSam,
};
use std::{path::Path, sync::Arc, time::Instant};
fn main() -> Result<()> {
    let root = Path::new("artifacts/evaluation");
    let entries: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json"))?)?;
    let mut baseline = Segmenter::load(Path::new("models"), true)?;
    let mut sam = IterativeSam::load(Path::new("models"), true)?;
    let mut matte = Matte::load(Path::new("models"), true)?;
    let mut records = Vec::new();
    for entry in entries
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["split"] == "heldout")
    {
        let image = image::open(root.join(entry["image"].as_str().unwrap()))?.to_rgba8();
        let alpha = image::open(root.join(entry["alpha"].as_str().unwrap()))?.to_luma8();
        let truth = Mask::new(alpha.width(), alpha.height(), alpha.into_raw())
            .map_err(anyhow::Error::msg)?;
        let frame = Arc::new(
            Frame::new(image.width(), image.height(), image.into_raw())
                .map_err(anyhow::Error::msg)?,
        );
        let point = (
            entry["point"][0].as_f64().unwrap() as f32,
            entry["point"][1].as_f64().unwrap() as f32,
            true,
        );
        let embedding = baseline.encode(&frame)?;
        let (coarse, _) = baseline.segment(&embedding, &[point])?;
        let t = Instant::now();
        let result = sam.predict(&frame, &[point], None)?;
        let first_ms = t.elapsed().as_millis();
        let t = Instant::now();
        let refined = matte.finish(&frame, &result.mask)?;
        let matte_ms = t.elapsed().as_millis();
        let automatic = measure(&refined, &truth).map_err(anyhow::Error::msg)?;
        let mut editor = Editor::new(frame.clone(), refined, true).map_err(anyhow::Error::msg)?;
        let mut strokes = 0;
        if entry["category"] == "thin" {
            let x = point.0 - 35.;
            let y = point.1 - 40.;
            for ray in 0..12 {
                editor
                    .paint(
                        &[(x + 35., y + 40.), (x + 180., y + ray as f32 * 15.)],
                        1.2,
                        255,
                    )
                    .map_err(anyhow::Error::msg)?;
                strokes += 1;
            }
        }
        let manual = measure(&editor.mask(), &truth).map_err(anyhow::Error::msg)?;
        let t = Instant::now();
        let correction = sam.predict(&frame, &[point, (10., 10., false)], None);
        let correction_ms = t.elapsed().as_millis();
        let record = serde_json::json!({"id":entry["id"],"category":entry["category"],"baseline":measure(&coarse,&truth).map_err(anyhow::Error::msg)?,"automatic_quality":automatic,"with_manual_repair":manual,"manual_strokes":strokes,"first_precision_ms":first_ms,"matting_ms":matte_ms,"cached_correction_ms":correction_ms,"correction_succeeded":correction.is_ok()});
        println!("{record}");
        records.push(record);
    }
    let report = serde_json::json!({"records":records,"adapter":objectlift::device::preferred().map(|a|&a.name),"limitations":"12 reused synthetic held-out regression fixtures, not an unseen natural-image benchmark. Thin-shape manual strokes use known generator geometry and are explicitly assisted/oracle-informed. Boundary F1 tolerance 2px; alpha MAE on union boundary band 6px; SAD uses alpha 0..1 divided by 1000. Per-stage times exclude loading/capture/UI/export."});
    std::fs::write(
        "artifacts/precision-evaluation.json",
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
