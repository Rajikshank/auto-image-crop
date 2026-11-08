use anyhow::Result;
use objectlift::{
    Frame,
    advanced::{Detector, Matte, Sam21},
    inference::Segmenter,
};
use std::{path::Path, time::Instant};
fn main() -> Result<()> {
    let root = Path::new("models");
    let image = image::open("artifacts/truck.jpg")?
        .resize(1200, 800, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let frame =
        Frame::new(image.width(), image.height(), image.into_raw()).map_err(anyhow::Error::msg)?;
    let mut mobile = Segmenter::load(root, true)?;
    let _cache = mobile.encode(&frame)?;
    let mut detector = Detector::load(root, false)?;
    let mut sam = Sam21::load(root, true)?;
    let mut matte = Matte::load(root, true)?;
    let mut samples = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let masks = detector.discover(&frame)?;
        let discovery = t.elapsed().as_millis();
        let t = Instant::now();
        let p = sam.refine(&frame, &[(500., 333., true)], || false)?;
        let precision = t.elapsed().as_millis();
        let t = Instant::now();
        let mask = matte.finish(&frame, &p.mask)?;
        let matting = t.elapsed().as_millis();
        let t = Instant::now();
        let colors = objectlift::quality::recover_colors(&frame, &mask);
        let cutout = objectlift::extract(&colors, &mask).map_err(anyhow::Error::msg)?;
        let _ = objectlift::platform::png(&cutout)?;
        samples.push(serde_json::json!({"discovery_cpu_ms":discovery,"candidates":masks.len(),"precision_gpu_ms":precision,"matting_gpu_ms":matting,"recover_extract_png_ms":t.elapsed().as_millis()}));
    }
    let report = serde_json::json!({"fixture":"truck 1200x800","adapter":objectlift::device::preferred().map(|a|&a.name),"samples":samples,"limitations":"Five fixture samples. Excludes capture, model load, OS clipboard publication, save dialog, UI, and user correction time; discovery uses CPU explicitly."});
    std::fs::write(
        "artifacts/quality-performance.json",
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
