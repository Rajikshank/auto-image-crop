use crate::roi::Region;
pub fn regions(region: Region, limit: u32, overlap: u32) -> Vec<Region> {
    let limit = limit.max(32);
    let step = limit.saturating_sub(overlap.min(limit / 2)).max(1);
    let axis = |length: u32| {
        let mut starts = vec![0];
        while *starts.last().unwrap() + limit < length {
            let next = (*starts.last().unwrap() + step).min(length.saturating_sub(limit));
            if next == *starts.last().unwrap() {
                break;
            }
            starts.push(next);
        }
        starts
    };
    let mut tiles = Vec::new();
    for y in axis(region.height) {
        for x in axis(region.width) {
            tiles.push(Region {
                x: region.x + x,
                y: region.y + y,
                width: limit.min(region.width - x),
                height: limit.min(region.height - y),
            });
        }
    }
    tiles
}
