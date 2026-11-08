use objectlift::dock::{Action, Dock};
#[test]
fn toolbar_hits_are_separate_from_image_and_fit_small_scaled_screens() {
    for (width, scale) in [(1280, 1.0), (800, 1.5), (480, 2.0)] {
        let dock = Dock::new(width, scale);
        assert!(dock.right <= width as i32);
        assert!(dock.hit(0, 400).is_none());
        for (action, rect) in dock.buttons() {
            assert_eq!(
                dock.hit((rect.0 + rect.2) / 2, (rect.1 + rect.3) / 2),
                Some(action)
            );
        }
        assert!(dock.buttons().iter().any(|b| b.0 == Action::Copy));
    }
}
#[test]
fn contextual_dock_fits_a_narrow_viewport_at_two_hundred_percent_scaling() {
    use objectlift::dock::Stage;
    for stage in [Stage::Discover, Stage::Selected, Stage::Refine] {
        let dock = Dock::context(480, 320, 2., stage, None);
        assert!(
            dock.bottom <= 320 && dock.right <= 480,
            "{stage:?} exceeds the viewport"
        );
        assert!(
            dock.buttons()
                .iter()
                .all(|(_, r)| r.2 - r.0 >= 84 && r.3 - r.1 >= 88)
        );
    }
}
#[test]
fn dock_reveals_only_actions_needed_for_discover_select_and_refine() {
    use objectlift::dock::Stage;
    let discover = Dock::context(1280, 720, 1., Stage::Discover, None);
    assert!(discover.buttons().len() <= 3);
    let selected = Dock::context(1280, 720, 1., Stage::Selected, Some((900, 600, 1200, 710)));
    assert!(selected.buttons().iter().any(|p| p.0 == Action::Inspect));
    assert!(selected.bottom <= 720);
    assert!(selected.right <= 1280);
    let refine = Dock::context(480, 320, 1.5, Stage::Refine, None);
    assert!(refine.buttons().len() <= 6);
    assert!(refine.slider().is_some());
    assert!(!refine.buttons().iter().any(|p| p.0 == Action::Mode));
}

#[test]
fn selected_dock_prioritizes_copy_and_has_an_explicit_close_without_permanent_undo() {
    use objectlift::dock::Stage;
    let dock = Dock::context(1280, 720, 1., Stage::Selected, Some((100, 100, 500, 400)));
    let buttons = dock.buttons();
    assert!(buttons.iter().any(|p| p.0 == Action::Close));
    assert!(!buttons.iter().any(|p| p.0 == Action::Undo));
    assert!(
        dock.bottom - dock.top <= 68,
        "selected controls should be one compact surface"
    );
}

#[test]
fn compact_refinement_keeps_room_for_an_optional_opacity_slider() {
    use objectlift::dock::Stage;
    let dock = Dock::context(480, 320, 2., Stage::Refine, None);
    assert!(
        dock.bottom + 56 <= 320,
        "secondary slider must not run off a small scaled work area"
    );
    assert!(dock.buttons().iter().any(|p| p.0 == Action::Done));
}

#[test]
fn fractional_dpi_preserves_real_forty_four_dip_hit_targets() {
    use objectlift::dock::Stage;
    for scale in [1., 1.25, 1.5, 2., 2.5] {
        for stage in [Stage::Discover, Stage::Selected, Stage::Refine] {
            let dock = Dock::context(320, 640, scale, stage, None);
            assert!(
                dock.buttons()
                    .iter()
                    .all(|(_, r)| (r.2 - r.0) as f32 >= 44. * scale
                        && (r.3 - r.1) as f32 >= 44. * scale),
                "{stage:?} at {scale}x has an undersized control"
            );
        }
    }
}
