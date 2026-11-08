use objectlift::{roi::Region, tiles};
#[test]
fn large_refinement_regions_are_fully_covered_by_bounded_overlapping_tiles() {
    let region = Region::new(100, 50, 2100, 1600, 2400, 1800).unwrap();
    let tiles = tiles::regions(region, 1024, 64);
    assert!(tiles.len() > 1);
    for tile in &tiles {
        assert!(tile.width <= 1024 && tile.height <= 1024);
        assert!(tile.x >= 100 && tile.y >= 50);
    }
    for y in 50..1650 {
        for x in 100..2200 {
            assert!(
                tiles
                    .iter()
                    .any(|r| x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height)
            );
        }
    }
}
