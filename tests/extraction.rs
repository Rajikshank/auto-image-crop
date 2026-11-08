use objectlift::{Frame, Mask, extract};

#[test]
fn extraction_tightly_crops_original_pixels_and_preserves_holes_and_soft_alpha() {
    let frame = Frame::new(3, 2, [100, 40, 20, 255].repeat(6)).unwrap();
    let mask = Mask::new(3, 2, vec![0, 255, 128, 0, 0, 255]).unwrap();
    let cutout = extract(&frame, &mask).unwrap();
    assert_eq!((cutout.width, cutout.height), (2, 2));
    assert_eq!(
        cutout.rgba,
        [
            100, 40, 20, 255, 100, 40, 20, 128, 0, 0, 0, 0, 100, 40, 20, 255
        ]
    );
}

#[test]
fn invalid_dimensions_and_mismatched_masks_are_rejected_without_panicking() {
    assert!(Frame::new(0, 1, vec![]).is_err());
    assert!(Frame::new(u32::MAX, u32::MAX, vec![]).is_err());
    assert!(Mask::new(2, 2, vec![255]).is_err());
    let frame = Frame::new(2, 2, vec![255; 16]).unwrap();
    let mask = Mask::new(1, 1, vec![255]).unwrap();
    assert!(extract(&frame, &mask).is_err());
}

#[test]
fn tight_crop_ignores_invisible_source_pixels_and_combines_source_alpha() {
    let frame = Frame::new(3, 1, vec![20, 30, 40, 0, 80, 90, 100, 128, 20, 30, 40, 0]).unwrap();
    let mask = Mask::new(3, 1, vec![255, 128, 255]).unwrap();
    let output = extract(&frame, &mask).unwrap();
    assert_eq!((output.width, output.height), (1, 1));
    assert_eq!(output.rgba, vec![80, 90, 100, 64]);
}
