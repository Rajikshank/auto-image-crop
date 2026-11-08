//! Re-encode original pixels around a proposal; never crop correction points away.
use crate::{Frame, Mask, inference::Segmenter, roi::Region};
use anyhow::{Result, bail};

pub fn refine(
    engine: &mut Segmenter,
    frame: &Frame,
    proposal: &Mask,
    points: &[(f32, f32, bool)],
    cancelled: impl Fn() -> bool,
) -> Result<(Mask, f32)> {
    let mut region = Region::around_mask(proposal, 0.25)
        .unwrap_or_else(|| Region::full(frame))
        .including(points, frame.width, frame.height);
    for attempt in 0..3 {
        if cancelled() {
            bail!("Selection cancelled");
        }
        let crop = region.crop(frame).map_err(anyhow::Error::msg)?;
        let local: Vec<_> = points
            .iter()
            .filter_map(|&p| region.local_point(p))
            .collect();
        if local.len() != points.len() {
            bail!("Crop cannot exclude correction points");
        }
        let embedding = engine.encode(&crop)?;
        if cancelled() {
            bail!("Selection cancelled");
        }
        let result = engine.predict(&embedding, &local)?;
        if region.touches_boundary(&result.mask) && region != Region::full(frame) && attempt < 2 {
            region = if attempt == 1 {
                Region::full(frame)
            } else {
                region.expand(frame.width, frame.height)
            };
            continue;
        }
        return Ok((
            region
                .restore(&result.mask, frame.width, frame.height)
                .map_err(anyhow::Error::msg)?,
            result.score,
        ));
    }
    unreachable!()
}
