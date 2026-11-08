use objectlift::{Mask, quality::trimap};

#[test]
fn adaptive_trimap_preserves_thin_structures_as_unknown_and_opaque_cores_as_foreground() {
    let mut alpha = vec![0; 21 * 21];
    for y in 5..16 {
        for x in 5..16 {
            alpha[y * 21 + x] = 255;
        }
    }
    for x in 0..5 {
        alpha[10 * 21 + x] = 255;
    }
    let mask = Mask::new(21, 21, alpha).unwrap();
    let map = trimap(&mask, 2);
    assert_eq!(map[10 * 21 + 10], 255);
    assert_eq!(map[10 * 21 + 2], 128);
    assert_eq!(map[0], 0);
    assert_eq!(map[5 * 21 + 5], 128);
}

#[test]
fn opaque_edge_antialiasing_keeps_thin_foreground_and_does_not_blur_far_background() {
    let logits = vec![-2., -2., -2., -2., 2., -2., -2., -2., -2.];
    let alpha = objectlift::quality::antialias(&logits, 3, 3);
    assert!(alpha[4] >= 128, "one-pixel structures remain foreground");
    assert!(alpha.iter().any(|&a| a > 0 && a < 255));
    assert_eq!(
        objectlift::quality::antialias(&[-4.; 25], 5, 5),
        vec![0; 25]
    );
}
