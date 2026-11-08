use objectlift::{Mask, evaluation::measure};
#[test]
fn boundary_metrics_distinguish_geometry_errors_and_exact_alpha_agreement() {
    let mut alpha = vec![0; 400];
    for y in 4..16 {
        for x in 4..16 {
            alpha[y * 20 + x] = 255;
        }
    }
    let truth = Mask::new(20, 20, alpha).unwrap();
    let identical = measure(&truth, &truth).unwrap();
    assert_eq!(identical.iou, 1.);
    assert_eq!(identical.boundary_f1, 1.);
    assert_eq!(identical.boundary_alpha_mae, 0.);
    let mut shifted = vec![0; 400];
    for y in 4..16 {
        for x in 5..17 {
            shifted[y * 20 + x] = 255;
        }
    }
    let moved = measure(&Mask::new(20, 20, shifted).unwrap(), &truth).unwrap();
    assert!(moved.iou < 1.);
    assert_eq!(moved.boundary_f1, 1.);
    assert!(moved.sad > 0.);
}
