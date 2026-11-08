use objectlift::{Frame, Mask, roi::Region};

#[test]
fn original_resolution_crop_and_mask_restoration_preserve_offset_and_holes() {
    let frame = Frame::new(6, 4, (0..24).flat_map(|n| [n, 0, 0, 255]).collect()).unwrap();
    let region = Region::new(2, 1, 3, 2, 6, 4).unwrap();
    let crop = region.crop(&frame).unwrap();
    assert_eq!((crop.width, crop.height), (3, 2));
    assert_eq!(crop.rgba[0], 8);
    assert_eq!(region.local_point((3., 2., true)), Some((1., 1., true)));
    let local = Mask::new(3, 2, vec![255, 0, 255, 0, 255, 0]).unwrap();
    let restored = region.restore(&local, 6, 4).unwrap();
    assert!(restored.contains(2, 1));
    assert!(!restored.contains(3, 1));
    assert!(restored.contains(3, 2));
    assert!(!restored.contains(0, 0));
}

#[test]
fn selection_region_keeps_every_correction_and_expands_without_clipping_screen_edges() {
    let region = Region::new(50, 40, 20, 20, 200, 100).unwrap().including(
        &[(5., 5., false), (198., 98., true)],
        200,
        100,
    );
    assert!(region.local_point((5., 5., false)).is_some());
    assert!(region.local_point((198., 98., true)).is_some());
    let expanded = region.expand(200, 100);
    assert_eq!(expanded, Region::new(0, 0, 200, 100, 200, 100).unwrap());
    assert!(Region::new(u32::MAX, 0, 20, 20, 200, 100).is_err());
}
