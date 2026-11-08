use objectlift::motion;
#[test]
fn selection_fade_finishes_on_time_and_reduced_motion_is_immediate() {
    assert_eq!(motion::opacity(0., 160., false), 0.);
    assert!(motion::opacity(80., 160., false) > 0.5);
    assert_eq!(motion::opacity(160., 160., false), 1.);
    assert_eq!(motion::opacity(1000., 160., false), 1.);
    assert_eq!(motion::opacity(0., 160., true), 1.);
}
