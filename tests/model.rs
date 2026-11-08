//! Opt-in tests use explicitly downloaded local models and the upstream SAM 2 truck fixture.
use objectlift::{Frame, app::lasso_prompts, inference::Segmenter};
use std::path::Path;

fn fixture() -> Frame {
    let image = image::open("artifacts/truck.jpg")
        .expect("Download the upstream truck fixture with scripts/verify.ps1")
        .resize(900, 600, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Frame::new(image.width(), image.height(), image.into_raw()).unwrap()
}

#[test]
#[ignore = "Requires local model assets and image fixture; executes real CPU and DirectML inference"]
fn real_model_cpu_and_gpu_agree_and_respect_corrective_prompts() {
    let frame = fixture();
    let root = Path::new("models");
    let mut cpu = Segmenter::load(root, false).unwrap();
    let e = cpu.encode(&frame).unwrap();
    let (baseline, _) = cpu.segment(&e, &[(375., 250., true)]).unwrap();
    assert!(baseline.contains(375, 250));
    assert!(!baseline.contains(10, 10));
    let (corrected, _) = cpu
        .segment(&e, &[(375., 250., true), (286., 373., false)])
        .unwrap();
    assert!(corrected.contains(375, 250));
    assert!(!corrected.contains(286, 373));
    let stroke = vec![(40, 135), (860, 135), (860, 430), (40, 430)];
    let (lasso, _) = cpu.segment(&e, &lasso_prompts(&stroke)).unwrap();
    assert!(lasso.contains(375, 250));
    assert!(!lasso.contains(10, 10));
    let mut gpu = Segmenter::load(root, true).unwrap();
    assert_eq!(
        gpu.provider, "DirectML",
        "GPU parity requires actual GPU execution, not CPU fallback"
    );
    let e = gpu.encode(&frame).unwrap();
    let (accelerated, _) = gpu.segment(&e, &[(375., 250., true)]).unwrap();
    let mut intersection = 0usize;
    let mut union = 0usize;
    for (&a, &b) in baseline.alpha.iter().zip(&accelerated.alpha) {
        if a > 0 || b > 0 {
            union += 1;
        }
        if a > 0 && b > 0 {
            intersection += 1;
        }
    }
    let iou = intersection as f32 / union as f32;
    println!("CPU/DirectML mask IoU={iou:.6}");
    assert!(iou > 0.98);
}

#[test]
#[ignore = "Opt-in measured benchmark; run optimized and without parallel test execution"]
fn record_warm_model_and_extraction_timings() {
    use std::time::Instant;
    let frame = fixture();
    let loaded = Instant::now();
    let mut engine = Segmenter::load(Path::new("models"), true).unwrap();
    let load_ms = loaded.elapsed().as_secs_f64() * 1000.0;
    let encoded = Instant::now();
    let embedding = engine.encode(&frame).unwrap();
    let encode_ms = encoded.elapsed().as_secs_f64() * 1000.0;
    let (mask, _) = engine.segment(&embedding, &[(375., 250., true)]).unwrap();
    let mut decoder = Vec::new();
    let mut extraction = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let _ = engine.segment(&embedding, &[(375., 250., true)]).unwrap();
        decoder.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let cutout = objectlift::extract(&frame, &mask).unwrap();
        let _ = objectlift::platform::png(&cutout).unwrap();
        extraction.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    decoder.sort_by(f64::total_cmp);
    extraction.sort_by(f64::total_cmp);
    let report = serde_json::json!({"samples":20,"image_width":frame.width,"image_height":frame.height,"provider":engine.provider,"optimized":!cfg!(debug_assertions),"model_load_ms":load_ms,"first_encoder_ms":encode_ms,"warm_decoder_ms":{"p50":decoder[9],"p95":decoder[18]},"cached_mask_extract_and_png_ms":{"p50":extraction[9],"p95":extraction[18]},"limitations":"Fixture only. Excludes native clipboard publication, screen capture, UI presentation and full discovery. Not a product SLA."});
    std::fs::write(
        "artifacts/benchmark.json",
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}
