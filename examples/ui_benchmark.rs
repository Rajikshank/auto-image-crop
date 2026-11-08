use objectlift::{Frame, Mask, highlight};
use std::{hint::black_box, time::Instant};
fn main() {
    let mut records = Vec::new();
    for (w, h) in [(1920, 1080), (2560, 1440), (3840, 2160)] {
        let frame = Frame::new(w, h, [91, 105, 124, 255].repeat((w * h) as usize)).unwrap();
        let mut alpha = vec![0; (w * h) as usize];
        for y in h / 4..h * 3 / 4 {
            for x in w / 4..w * 3 / 4 {
                alpha[(y * w + x) as usize] = 255;
            }
        }
        let mask = Mask::new(w, h, alpha).unwrap();
        for selected in [false, true] {
            let mut times = Vec::new();
            for _ in 0..12 {
                let t = Instant::now();
                black_box(if selected {
                    highlight::render_exact(&frame, &mask)
                } else {
                    highlight::render(&frame, Some(&mask))
                });
                times.push(t.elapsed().as_secs_f64() * 1000.);
            }
            times.sort_by(f64::total_cmp);
            records.push(serde_json::json!({"width":w,"height":h,"selected":selected,"samples":12,"p50_ms":times[6],"p95_ms":times[11]}));
        }
    }
    println!(
        "{}",
        serde_json::json!({"records":records,"scope":"CPU scene preparation only, includes allocation; excludes native presentation, inference and capture. Synthetic central-quarter mask; release build."})
    );
}
