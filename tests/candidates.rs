use objectlift::{Candidates, Mask};
#[test]
fn hover_uses_shape_and_cycles_smaller_overlapping_objects_first() {
    let mut candidates = Candidates::default();
    candidates.insert(Mask::new(3, 2, vec![255; 6]).unwrap());
    candidates.insert(Mask::new(3, 2, vec![0, 255, 0, 0, 0, 0]).unwrap());
    assert_eq!(candidates.at(1, 0, 0).unwrap().area(), 1);
    assert_eq!(candidates.at(1, 0, 1).unwrap().area(), 6);
    assert_eq!(candidates.at(0, 1, 0).unwrap().area(), 6);
    assert!(candidates.at(10, 10, 0).is_none());
}
#[test]
fn objects_can_be_cycled_without_positioning_a_mouse_pointer() {
    let mut candidates = objectlift::Candidates::default();
    candidates.insert(Mask::new(3, 1, vec![255, 0, 0]).unwrap());
    candidates.insert(Mask::new(3, 1, vec![0, 0, 255]).unwrap());
    assert!(candidates.cycle(0).unwrap().contains(0, 0));
    assert!(candidates.cycle(1).unwrap().contains(2, 0));
    assert!(candidates.cycle(2).unwrap().contains(0, 0));
}

#[test]
fn correction_seed_is_inside_the_shape_even_when_its_center_is_a_hole() {
    let mask = Mask::new(3, 3, vec![255, 255, 255, 255, 0, 255, 255, 255, 255]).unwrap();
    let (x, y) = mask.interior_point().unwrap();
    assert!(mask.contains(x, y));
    assert!(Mask::new(1, 1, vec![0]).unwrap().interior_point().is_none());
}
