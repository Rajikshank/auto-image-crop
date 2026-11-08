use objectlift::{Frame, Mask, highlight};

#[test]
fn preview_has_a_soft_outer_halo_and_visible_interior_without_changing_export_pixels() {
    let frame = Frame::new(40, 40, [80, 80, 80, 255].repeat(1600)).unwrap();
    let mut alpha = vec![0; 1600];
    for y in 10..30 {
        for x in 10..30 {
            alpha[y * 40 + x] = 255;
        }
    }
    let mask = Mask::new(40, 40, alpha).unwrap();
    let preview = highlight::render(&frame, Some(&mask));
    let pixel = |x: usize, y: usize| &preview[(y * 40 + x) * 4..(y * 40 + x) * 4 + 3];
    assert_eq!(pixel(0, 0), [65, 65, 65]);
    assert_ne!(
        pixel(8, 20),
        [80, 80, 80],
        "soft glow should extend beyond the shape"
    );
    assert!(
        pixel(20, 20)[0].abs_diff(80) > 35,
        "interior coverage must be obvious"
    );
    assert_eq!(frame.rgba, [80, 80, 80, 255].repeat(1600));
    let exported = objectlift::extract(&frame, &mask).unwrap();
    assert!(
        exported
            .rgba
            .chunks_exact(4)
            .all(|p| p == [80, 80, 80, 255])
    );
}
#[test]
fn alpha_visualization_separates_background_and_shows_partial_coverage_without_mutating_mask() {
    let frame = Frame::new(40, 40, [80, 80, 80, 255].repeat(1600)).unwrap();
    let none = Mask::new(40, 40, vec![0; 1600]).unwrap();
    let half = Mask::new(40, 40, vec![96; 1600]).unwrap();
    let solid = Mask::new(40, 40, vec![255; 1600]).unwrap();
    let a = highlight::render_exact(&frame, &none);
    let b = highlight::render_exact(&frame, &half);
    let c = highlight::render_exact(&frame, &solid);
    let i = (20 * 40 + 20) * 4;
    assert!(a[i] < b[i] && b[i] < c[i]);
    assert_eq!(half.alpha, vec![96; 1600]);
}

#[test]
fn coverage_preview_is_obvious_preserves_holes_and_never_changes_export() {
    let frame = Frame::new(64, 64, [80, 80, 80, 255].repeat(4096)).unwrap();
    let mut alpha = vec![0; 4096];
    for y in 8..56 {
        for x in 8..56 {
            alpha[y * 64 + x] = 255;
        }
    }
    for y in 24..40 {
        for x in 24..40 {
            alpha[y * 64 + x] = 0;
        }
    }
    let mask = Mask::new(64, 64, alpha).unwrap();
    let before = objectlift::extract(&frame, &mask).unwrap().rgba;
    let preview = highlight::render(&frame, Some(&mask));
    let inside = (16 * 64 + 16) * 4;
    assert!(
        preview[inside].abs_diff(80) > 35,
        "whole object must have an unmistakable tint"
    );
    let hole = (32 * 64 + 32) * 4;
    assert_eq!(
        &preview[hole..hole + 3],
        &preview[0..3],
        "excluded holes use background treatment"
    );
    assert_eq!(objectlift::extract(&frame, &mask).unwrap().rgba, before);
}

#[test]
fn preview_respects_transparent_source_and_solid_mask_mode() {
    let mut rgba = [80, 80, 80, 255].repeat(4096);
    for p in rgba.chunks_exact_mut(4).take(64 * 32) {
        p[3] = 0;
    }
    let frame = Frame::new(64, 64, rgba).unwrap();
    let mask = Mask::new(64, 64, vec![255; 4096]).unwrap();
    let style = highlight::Appearance {
        strength: 255,
        dim: 0,
        color: [137, 151, 255],
        contour: false,
    };
    let preview = highlight::render_with(&frame, Some(&mask), style);
    assert_eq!(
        &preview[(16 * 64 + 32) * 4..(16 * 64 + 32) * 4 + 3],
        &[80, 80, 80]
    );
    assert_eq!(
        &preview[(48 * 64 + 32) * 4..(48 * 64 + 32) * 4 + 3],
        &[255, 151, 137]
    );
}
