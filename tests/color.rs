use objectlift::{Frame, Mask};
#[test]
fn soft_edge_color_recovery_removes_known_background_without_changing_opaque_pixels() {
    let frame = Frame::new(
        5,
        1,
        vec![
            20, 40, 80, 255, 20, 40, 80, 255, 120, 120, 140, 255, 220, 200, 200, 255, 220, 200,
            200, 255,
        ],
    )
    .unwrap();
    let mask = Mask::new(5, 1, vec![255, 255, 128, 0, 0]).unwrap();
    let recovered = objectlift::quality::recover_colors(&frame, &mask);
    assert_eq!(&recovered.rgba[..8], &frame.rgba[..8]);
    for (got, expected) in recovered.rgba[8..11].iter().zip([20u8, 40, 80]) {
        assert!(got.abs_diff(expected) < 4);
    }
}
