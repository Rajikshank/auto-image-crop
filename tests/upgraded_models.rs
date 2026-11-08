use objectlift::{
    Frame, Mask,
    advanced::{Detector, Matte, Sam21},
    extract, platform,
};
use std::path::Path;
#[test]
#[ignore = "Requires explicitly installed pinned upgrade assets; real CPU/DirectML inference"]
fn precision_matting_and_detection_execute_on_real_pixels() {
    let image = image::open("artifacts/truck.jpg")
        .unwrap()
        .resize(900, 600, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let frame = Frame::new(image.width(), image.height(), image.into_raw()).unwrap();
    let root = Path::new("models");
    let mut cpu = Sam21::load(root, false).unwrap();
    let a = cpu.predict(&frame, &[(375., 250., true)]).unwrap();
    drop(cpu);
    let mut gpu = Sam21::load(root, true).unwrap();
    let b = gpu.refine(&frame, &[(375., 250., true)], || false).unwrap();
    assert!(b.mask.contains(375, 250));
    assert!(!b.mask.contains(10, 10));
    let corrected = gpu
        .predict(&frame, &[(375., 250., true), (286., 373., false)])
        .unwrap();
    assert!(!corrected.mask.contains(286, 373));
    let intersection = a
        .mask
        .alpha
        .iter()
        .zip(&b.mask.alpha)
        .filter(|(a, b)| **a > 127 && **b > 127)
        .count();
    let union = a
        .mask
        .alpha
        .iter()
        .zip(&b.mask.alpha)
        .filter(|(a, b)| **a > 127 || **b > 127)
        .count();
    assert!(intersection as f32 / union as f32 > 0.98);
    drop(gpu);
    let mut matte = Matte::load(root, true).unwrap();
    let finished = matte.finish(&frame, &b.mask).unwrap();
    assert!(finished.contains(375, 250));
    assert!(!finished.contains(10, 10));
    assert!(finished.alpha.iter().any(|&a| a > 0 && a < 255));
    let recovered = objectlift::quality::recover_colors(&frame, &finished);
    let cutout = extract(&recovered, &finished).unwrap();
    std::fs::write(
        "artifacts/quality-truck.png",
        platform::png(&cutout).unwrap(),
    )
    .unwrap();
    drop(matte);
    let mut detector = Detector::load(root, false).unwrap();
    let masks = detector.discover(&frame).unwrap();
    assert!(
        !masks.is_empty(),
        "RF-DETR should discover the truck without a point"
    );
    assert!(masks.iter().any(|m| m.contains(375, 250)));
    println!(
        "{}; detector_candidates={}; precision CPU/GPU IoU={}",
        objectlift::device::preferred().unwrap().name,
        masks.len(),
        intersection as f32 / union as f32
    );
}

#[test]
#[ignore = "Requires local iterative decoder; executes real correction and cache invalidation"]
fn iterative_corrections_reuse_features_and_preserve_exact_user_constraints() {
    use objectlift::{editor::Editor, iterative::IterativeSam};
    use std::sync::Arc;
    let image = image::open("artifacts/truck.jpg")
        .unwrap()
        .resize(900, 600, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let mut frame = Frame::new(image.width(), image.height(), image.into_raw()).unwrap();
    let mut model = IterativeSam::load(Path::new("models"), true).unwrap();
    let first = model.predict(&frame, &[(375., 250., true)], None).unwrap();
    let mut cpu = IterativeSam::load(Path::new("models"), false).unwrap();
    let cpu_first = cpu.predict(&frame, &[(375., 250., true)], None).unwrap();
    let agree = |a: &Mask, b: &Mask| {
        let intersection = a
            .alpha
            .iter()
            .zip(&b.alpha)
            .filter(|(a, b)| **a > 127 && **b > 127)
            .count();
        let union = a
            .alpha
            .iter()
            .zip(&b.alpha)
            .filter(|(a, b)| **a > 127 || **b > 127)
            .count();
        intersection as f32 / union.max(1) as f32
    };
    assert!(agree(&first.mask, &cpu_first.mask) > 0.99);
    let mut editor = Editor::new(Arc::new(frame.clone()), first.mask, false).unwrap();
    editor.paint(&[(375., 250.)], 0.5, 96).unwrap();
    let corrected = model
        .predict(&frame, &[(375., 250., true), (286., 373., false)], None)
        .unwrap();
    assert!(!corrected.mask.contains(286, 373));
    assert_eq!(model.encoder_runs, 1);
    assert_eq!(model.prior_runs, 1);
    let cpu_corrected = cpu
        .predict(&frame, &[(375., 250., true), (286., 373., false)], None)
        .unwrap();
    assert!(agree(&corrected.mask, &cpu_corrected.mask) > 0.99);
    editor.accept(corrected.mask).unwrap();
    assert_eq!(editor.mask().alpha[(250 * 900 + 375) as usize], 96);
    frame.rgba[0] ^= 1;
    let boxed = model
        .predict(&frame, &[(375., 250., true)], Some([40., 135., 860., 430.]))
        .unwrap();
    let cpu_boxed = cpu
        .predict(&frame, &[(375., 250., true)], Some([40., 135., 860., 430.]))
        .unwrap();
    assert!(agree(&boxed.mask, &cpu_boxed.mask) > 0.99);
    assert_eq!(
        model.encoder_runs, 2,
        "New frame must invalidate the feature/prior cache"
    );
    model.set_context(42, objectlift::roi::Region::full(&frame));
    let _ = model.predict(&frame, &[(375., 250., true)], None).unwrap();
    assert_eq!(
        model.encoder_runs, 3,
        "A new session must invalidate even identical pixel content"
    );
}

#[test]
#[ignore = "Requires ViTMatte assets; verifies real large-region tiles and explicit edge locality"]
fn large_matting_and_local_edge_refinement_preserve_known_pixels() {
    let (w, h) = (1900u32, 300u32);
    let mut rgba = [220, 220, 220, 255].repeat((w * h) as usize);
    let mut alpha = vec![0; (w * h) as usize];
    for y in 50..250 {
        for x in 20..1880 {
            let i = (y * w + x) as usize;
            alpha[i] = 255;
            rgba[i * 4..i * 4 + 3].copy_from_slice(&[25, 50, 80]);
        }
    }
    let frame = Frame::new(w, h, rgba).unwrap();
    alpha[(100 * w + 850) as usize] = 96;
    let mask = Mask::new(w, h, alpha).unwrap();
    let mut matte = Matte::load(Path::new("models"), true).unwrap();
    let finished = matte.finish(&frame, &mask).unwrap();
    assert_eq!(finished.alpha[(150 * w + 1024) as usize], 255);
    assert_eq!(finished.alpha[0], 0);
    let edge = matte
        .finish_detail(
            &frame,
            &mask,
            Some(&[(800., 50.), (900., 50.)]),
            12.,
            &|| false,
        )
        .unwrap();
    assert_eq!(
        edge.alpha[(150 * w + 100) as usize],
        mask.alpha[(150 * w + 100) as usize]
    );
    assert_eq!(edge.alpha[(150 * w + 1800) as usize], 255);
    assert_eq!(
        edge.alpha[(100 * w + 850) as usize],
        96,
        "Edge refinement must preserve fractional alpha outside the painted stroke"
    );
    assert!(
        matte
            .finish_detail(&frame, &mask, None, 6., &|| true)
            .is_err()
    );
}
