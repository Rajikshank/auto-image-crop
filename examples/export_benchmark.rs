use anyhow::Result;
use objectlift::{
    Frame,
    file_export::{Format, stage},
    platform,
};
use std::time::Instant;

fn main() -> Result<()> {
    let photo = image::open("artifacts/truck.jpg")?;
    let dir = tempfile::tempdir_in("artifacts")?;
    let mut records = Vec::new();
    for side in [1024, 2048] {
        let original = photo
            .resize_exact(side, side, image::imageops::FilterType::Triangle)
            .to_rgba8();
        let source = Frame::new(side, side, original.into_raw()).map_err(anyhow::Error::msg)?;
        let mut ready = source.clone();
        for (i, pixel) in ready.rgba.chunks_exact_mut(4).enumerate() {
            let x = (i % side as usize) as u32;
            pixel[3] = (x.min(side - 1 - x) * 4).min(255) as u8;
            if pixel[3] == 0 {
                pixel[..3].fill(0);
            }
        }
        for (label, format) in [
            ("previous PNG pipeline", None),
            ("staged PNG", Some(Format::Png)),
            ("lossless WebP", Some(Format::WebP)),
            ("source and mask ZIP", Some(Format::Bundle)),
        ] {
            let path = dir.path().join(format!(
                "output.{}",
                format.map_or("png", Format::extension)
            ));
            let mut timings = Vec::new();
            for _ in 0..12 {
                let start = Instant::now();
                if let Some(format) = format {
                    stage(&path, true, format, &ready, Some(&source), (0, 0))?.commit()?;
                } else {
                    platform::prepare(&ready)?.save_png(&path)?;
                }
                timings.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            timings.sort_by(f64::total_cmp);
            records.push(serde_json::json!({
                "size": side, "operation": label, "bytes": path.metadata()?.len(),
                "samples": timings.len(), "p50_ms": (timings[5]+timings[6])/2.0, "p95_ms": timings[11],
            }));
        }
        #[cfg(feature = "ag-psd")]
        if side == 1024 {
            stage(
                std::path::Path::new("artifacts/experimental-editable.psd"),
                true,
                Format::Psd,
                &ready,
                Some(&source),
                (0, 0),
            )?
            .commit()?;
            image::save_buffer(
                "artifacts/experimental-expected.png",
                &ready.rgba,
                side,
                side,
                image::ColorType::Rgba8,
            )?;
        }
    }
    println!("{}", serde_json::to_string_pretty(&records)?);
    Ok(())
}
