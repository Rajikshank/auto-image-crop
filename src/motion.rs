//! Time-based feedback with no recurring animation once the transition completes.
pub fn opacity(elapsed_ms: f32, duration_ms: f32, reduced_motion: bool) -> f32 {
    if reduced_motion || duration_ms <= 0. {
        return 1.;
    }
    let t = (elapsed_ms / duration_ms).clamp(0., 1.);
    1. - (1. - t).powi(3)
}
