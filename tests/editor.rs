use objectlift::{
    Frame, Mask,
    editor::{Background, Editor},
};
use std::sync::Arc;
#[test]
fn inspection_and_export_share_the_same_cropped_rgba_including_holes_and_partial_alpha() {
    let frame = Arc::new(Frame::new(4, 3, vec![80; 4 * 3 * 4]).unwrap());
    let mask = Mask::new(4, 3, vec![0, 0, 0, 0, 0, 255, 128, 0, 0, 0, 0, 0]).unwrap();
    let mut editor = Editor::new(frame, mask, false).unwrap();
    let ready = editor.ready().unwrap();
    let image = ready.image.clone();
    let png = objectlift::platform::png(&image).unwrap();
    assert_eq!(
        image::load_from_memory(&png).unwrap().to_rgba8().into_raw(),
        image.rgba
    );
    assert_eq!((ready.x, ready.y, image.width, image.height), (1, 1, 2, 1));
    let revision = ready.revision;
    let dark = editor.inspect(Background::Dark).unwrap();
    let light = editor.inspect(Background::Light).unwrap();
    assert_ne!(dark, light);
    assert_eq!(editor.ready().unwrap().revision, revision);
    assert_eq!(editor.ready().unwrap().image.rgba, image.rgba);
}
#[test]
fn smart_remove_preserves_correct_parts_outside_the_requested_correction_region() {
    let frame = Arc::new(Frame::new(4, 1, vec![255; 16]).unwrap());
    let mut editor = Editor::new(frame, Mask::new(4, 1, vec![255; 4]).unwrap(), false).unwrap();
    let region = objectlift::roi::Region::new(1, 0, 1, 1, 4, 1).unwrap();
    editor
        .accept_smart_in(Mask::new(4, 1, vec![0; 4]).unwrap(), false, Some(region))
        .unwrap();
    assert_eq!(editor.mask().alpha, [255, 0, 255, 255]);
}
#[test]
fn a_visible_translucent_selection_remains_editable_below_fifty_percent_opacity() {
    let mask = Mask::new(4, 2, vec![96; 8]).unwrap();
    assert!(mask.visible_point().is_some());
    let mut editor = Editor::new(
        Arc::new(Frame::new(4, 2, vec![255; 32]).unwrap()),
        mask,
        false,
    )
    .unwrap();
    assert!(
        editor
            .ready()
            .unwrap()
            .image
            .rgba
            .chunks_exact(4)
            .all(|p| p[3] == 96)
    );
}
#[test]
fn invalid_strokes_and_mismatched_predictions_leave_the_accepted_selection_unchanged() {
    let frame = Arc::new(Frame::new(4, 2, vec![255; 32]).unwrap());
    let mut editor = Editor::new(frame, Mask::new(4, 2, vec![255; 8]).unwrap(), false).unwrap();
    assert!(editor.paint(&[(5., 1.)], 1., 0).is_err());
    assert!(
        editor
            .accept(Mask {
                width: 4,
                height: 2,
                alpha: vec![0]
            })
            .is_err()
    );
    assert_eq!(editor.mask().alpha, vec![255; 8]);
}
#[test]
fn smart_add_cannot_erase_existing_parts_and_smart_remove_cannot_add_background() {
    let frame = Arc::new(Frame::new(3, 1, vec![255; 12]).unwrap());
    let mut editor = Editor::new(frame, Mask::new(3, 1, vec![255, 0, 0]).unwrap(), false).unwrap();
    editor
        .accept_smart(Mask::new(3, 1, vec![0, 255, 0]).unwrap(), true)
        .unwrap();
    assert_eq!(editor.mask().alpha, [255, 255, 0]);
    editor
        .accept_smart(Mask::new(3, 1, vec![255, 0, 255]).unwrap(), false)
        .unwrap();
    assert_eq!(editor.mask().alpha, [255, 0, 0]);
}
#[test]
fn zoom_keeps_the_cursor_on_the_same_image_pixel_and_round_trips_brush_coordinates() {
    let mut view = objectlift::editor::View::default();
    view.zoom_at(4., (320., 180.), (1280, 720));
    assert_eq!(view.image_point((320., 180.)), (320., 180.));
    let image = view.image_point((324., 184.));
    assert_eq!(image, (321., 181.));
    assert_eq!(view.screen_point(image), (324., 184.));
    view.zoom_at(8., (1279., 719.), (1280, 720));
    let p = view.image_point((1279., 719.));
    assert!(p.0 < 1280. && p.1 < 720.);
}
#[test]
fn exact_brush_edits_survive_ai_and_can_be_undone_and_redone_as_one_stroke() {
    let frame = Arc::new(Frame::new(8, 4, vec![255; 8 * 4 * 4]).unwrap());
    let mut editor = Editor::new(frame, Mask::new(8, 4, vec![0; 32]).unwrap(), false).unwrap();
    editor.paint(&[(1., 1.), (5., 1.)], 0.45, 96).unwrap();
    assert_eq!(&editor.mask().alpha[9..14], &[96; 5]);
    editor
        .accept(Mask::new(8, 4, vec![255; 32]).unwrap())
        .unwrap();
    assert_eq!(editor.mask().alpha[11], 96);
    editor.paint(&[(3., 1.)], 0.45, 0).unwrap();
    assert_eq!(editor.mask().alpha[11], 0);
    assert!(editor.undo());
    assert_eq!(editor.mask().alpha[11], 96);
    assert!(editor.redo());
    assert_eq!(editor.mask().alpha[11], 0);
    assert!(editor.undo());
    assert!(editor.undo()); // Undo automatic prediction; manual stroke remains.
    assert_eq!(editor.mask().alpha[0], 0);
    assert_eq!(editor.mask().alpha[11], 96);
    assert!(editor.undo());
    assert!(editor.mask().alpha.iter().all(|&a| a == 0));
    assert!(editor.redo());
    assert_eq!(editor.mask().alpha[11], 96);
    editor.paint(&[(0., 0.)], 0.45, 255).unwrap();
    assert!(!editor.redo());
}
