use crate::{
    Frame, Mask,
    inference::{Embedding, Segmenter},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::Duration,
};

pub enum Event {
    Ready(String),
    Status(u64, String),
    Candidate(u64, Mask),
    Selection(u64, u64, Mask),
    Error(u64, Option<u64>, String),
    Finished(u64),
}
struct Start {
    id: u64,
    frame: Arc<Frame>,
    cursor: (f32, f32),
}
pub type EdgeRequest = (Mask, Vec<(f32, f32)>, f32);
#[derive(Default)]
pub struct Hint {
    pub region: Option<crate::roi::Region>,
    pub bbox: Option<[f32; 4]>,
    pub edge: Option<EdgeRequest>,
}
struct Prompt {
    id: u64,
    revision: u64,
    points: Vec<(f32, f32, bool)>,
    mode: crate::settings::QualityMode,
    hint: Hint,
}
#[derive(Default)]
struct Pending {
    start: Option<Start>,
    prompt: Option<Prompt>,
    active: u64,
    revision: u64,
    shutdown: bool,
}
pub struct Worker {
    shared: Arc<(Mutex<Pending>, Condvar)>,
    pub events: Receiver<Event>,
}
impl Worker {
    pub fn spawn(models: PathBuf, gpu: bool) -> Self {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let (tx, events) = mpsc::sync_channel(64);
        let copy = shared.clone();
        thread::spawn(move || run(copy, tx, models, gpu));
        Self { shared, events }
    }
    pub fn start(&self, id: u64, frame: Arc<Frame>, cursor: (f32, f32)) {
        let (lock, cv) = &*self.shared;
        let mut p = lock.lock().unwrap();
        p.active = id;
        p.revision = 0;
        p.start = Some(Start { id, frame, cursor });
        p.prompt = None;
        cv.notify_one();
    }
    pub fn prompt(
        &self,
        id: u64,
        revision: u64,
        points: Vec<(f32, f32, bool)>,
        mode: crate::settings::QualityMode,
    ) {
        self.prompt_with_hint(id, revision, points, mode, Hint::default());
    }
    pub fn prompt_with_hint(
        &self,
        id: u64,
        revision: u64,
        points: Vec<(f32, f32, bool)>,
        mode: crate::settings::QualityMode,
        hint: Hint,
    ) {
        let (lock, cv) = &*self.shared;
        let mut p = lock.lock().unwrap();
        if p.active == id {
            p.revision = revision;
            p.prompt = Some(Prompt {
                id,
                revision,
                points,
                mode,
                hint,
            });
            cv.notify_one();
        }
    }
    pub fn invalidate(&self, id: u64, revision: u64) {
        let mut p = self.shared.0.lock().unwrap();
        if p.active == id {
            p.revision = revision;
            p.prompt = None;
        }
    }
    pub fn cancel(&self) {
        let mut p = self.shared.0.lock().unwrap();
        p.active = 0;
        p.start = None;
        p.prompt = None;
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, cv) = &*self.shared;
        let mut p = lock.lock().unwrap();
        p.shutdown = true;
        cv.notify_one();
    }
}

fn run(shared: Arc<(Mutex<Pending>, Condvar)>, tx: SyncSender<Event>, models: PathBuf, gpu: bool) {
    let mut engine = match Segmenter::load(&models, gpu) {
        Ok(e) => e,
        Err(e) => {
            let _ = tx.send(Event::Error(
                0,
                None,
                format!("Model loading failed: {e:#}"),
            ));
            return;
        }
    };
    let _ = tx.send(Event::Ready(format!(
        "{}{}",
        engine.provider,
        if engine.provider == "DirectML" {
            crate::device::preferred().map_or(String::new(), |a| format!(" · {}", a.name))
        } else {
            String::new()
        }
    )));
    let mut embedding: Option<(u64, Embedding)> = None;
    let mut samples = Vec::new();
    let mut frame: Option<Arc<Frame>> = None;
    let mut precision: Option<crate::iterative::IterativeSam> = None;
    let mut matte: Option<crate::advanced::Matte> = None;
    let mut detector: Option<crate::advanced::Detector> = None;
    let mut precision_region: Option<(u64, crate::roi::Region, usize)> = None;
    loop {
        let (start, prompt, active) = {
            let (lock, cv) = &*shared;
            let mut p = lock.lock().unwrap();
            if p.shutdown {
                return;
            }
            if p.start.is_none() && p.prompt.is_none() && samples.is_empty() {
                p = cv.wait_timeout(p, Duration::from_millis(250)).unwrap().0;
            }
            if p.shutdown {
                return;
            }
            (p.start.take(), p.prompt.take(), p.active)
        };
        if active == 0 {
            embedding = None;
            frame = None;
            samples.clear();
            continue;
        }
        if let Some(start) = start {
            let _ = tx.try_send(Event::Status(start.id, "Understanding the screen…".into()));
            if models.join("rfdetr/model.onnx").exists() {
                let _ = tx.try_send(Event::Status(
                    start.id,
                    "Finding objects · RF-DETR on CPU".into(),
                ));
                let discovered = (|| -> anyhow::Result<Vec<Mask>> {
                    if detector.is_none() {
                        detector = Some(crate::advanced::Detector::load(&models, false)?);
                    }
                    detector.as_mut().unwrap().discover(&start.frame)
                })();
                match discovered {
                    Ok(masks) => {
                        for mask in masks {
                            if shared.0.lock().unwrap().active != start.id {
                                break;
                            }
                            let _ = tx.try_send(Event::Candidate(start.id, mask));
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .try_send(Event::Status(start.id, format!("Discovery fallback: {e}")));
                    }
                }
            }
            if shared.0.lock().unwrap().active != start.id {
                continue;
            }
            match engine.encode(&start.frame) {
                Ok(e) => {
                    samples.clear();
                    for row in (0..4).rev() {
                        for col in (0..6).rev() {
                            samples.push((
                                (col as f32 + 0.5) * e.width as f32 / 6.0,
                                (row as f32 + 0.5) * e.height as f32 / 4.0,
                            ));
                        }
                    }
                    samples.push(start.cursor);
                    frame = Some(start.frame);
                    embedding = Some((start.id, e));
                }
                Err(e) => {
                    let _ = tx.try_send(Event::Error(start.id, None, e.to_string()));
                    embedding = None;
                    continue;
                }
            }
        }
        let Some((id, e)) = &embedding else {
            continue;
        };
        if *id != shared.0.lock().unwrap().active {
            samples.clear();
            embedding = None;
            continue;
        }
        if let Some(prompt) = prompt {
            if prompt.id == *id {
                let cancelled = || {
                    let pending = shared.0.lock().unwrap();
                    pending.active != prompt.id
                        || pending.revision != prompt.revision
                        || pending.shutdown
                        || pending.start.is_some()
                        || pending
                            .prompt
                            .as_ref()
                            .is_some_and(|p| p.revision > prompt.revision)
                };
                let result = (|| {
                    if let Some((mask, stroke, radius)) = &prompt.hint.edge {
                        if matte.is_none() {
                            matte = Some(crate::advanced::Matte::load(&models, gpu)?);
                        }
                        let result = matte.as_mut().unwrap().finish_detail(
                            frame.as_ref().unwrap(),
                            mask,
                            Some(stroke),
                            *radius,
                            &cancelled,
                        )?;
                        return Ok((result, 1.0));
                    }
                    if prompt.mode != crate::settings::QualityMode::Fast {
                        if precision.is_none() {
                            precision = Some(crate::iterative::IterativeSam::load(&models, gpu)?);
                        }
                        let frame = frame.as_ref().unwrap();
                        let mut region = prompt
                            .hint
                            .region
                            .unwrap_or_else(|| crate::roi::Region::full(frame))
                            .including(&prompt.points, frame.width, frame.height);
                        if let Some((old_id, old_region, old_count)) = precision_region
                            && old_id == prompt.id
                            && old_count > 1
                            && prompt.points.len() > old_count
                            && old_region.including(&prompt.points, frame.width, frame.height)
                                == old_region
                        {
                            region = old_region;
                        }
                        let crop = region.crop(frame).map_err(anyhow::Error::msg)?;
                        let local: Vec<_> = prompt
                            .points
                            .iter()
                            .filter_map(|&p| region.local_point(p))
                            .collect();
                        let bbox = prompt.hint.bbox.map(|[x, y, r, b]| {
                            [
                                x - region.x as f32,
                                y - region.y as f32,
                                r - region.x as f32,
                                b - region.y as f32,
                            ]
                        });
                        precision.as_mut().unwrap().set_context(prompt.id, region);
                        let mut prediction =
                            precision.as_mut().unwrap().predict(&crop, &local, bbox)?;
                        for attempt in 0..3 {
                            if !region.touches_boundary(&prediction.mask)
                                || region == crate::roi::Region::full(frame)
                            {
                                break;
                            }
                            region = if attempt == 2 {
                                crate::roi::Region::full(frame)
                            } else {
                                region.expand(frame.width, frame.height)
                            };
                            let crop = region.crop(frame).map_err(anyhow::Error::msg)?;
                            let local: Vec<_> = prompt
                                .points
                                .iter()
                                .filter_map(|&p| region.local_point(p))
                                .collect();
                            precision.as_mut().unwrap().set_context(prompt.id, region);
                            prediction =
                                precision.as_mut().unwrap().predict(&crop, &local, None)?;
                        }
                        prediction.mask = region
                            .restore(&prediction.mask, frame.width, frame.height)
                            .map_err(anyhow::Error::msg)?;
                        precision_region = Some((prompt.id, region, prompt.points.len()));
                        if cancelled() {
                            anyhow::bail!("Selection cancelled");
                        }
                        if prompt.mode == crate::settings::QualityMode::Balanced {
                            return Ok((prediction.mask, prediction.score));
                        }
                        if matte.is_none() {
                            matte = Some(crate::advanced::Matte::load(&models, gpu)?);
                        }
                        let mut mask = matte.as_mut().unwrap().finish_detail(
                            frame,
                            &prediction.mask,
                            None,
                            6.,
                            &cancelled,
                        )?;
                        // Segmentation geometry was validated above. Physical opacity may be below 50%.
                        for &(x, y, positive) in &prompt.points {
                            if !positive {
                                mask.alpha[y as usize * mask.width as usize + x as usize] = 0;
                            }
                        }
                        if mask.alpha.iter().all(|&a| a == 0) {
                            anyhow::bail!(
                                "Matting found no visible pixels; try Balanced or exact Add"
                            );
                        }
                        return Ok((mask, prediction.score));
                    }
                    engine.segment(e, &prompt.points)
                })();
                if cancelled() {
                    continue;
                }
                match result {
                    Ok((mask, _)) => {
                        let _ = tx.send(Event::Selection(*id, prompt.revision, mask));
                    }
                    Err(err) => {
                        let _ = tx.send(Event::Error(*id, Some(prompt.revision), err.to_string()));
                    }
                }
            }
            continue;
        }
        if let Some((x, y)) = samples.pop() {
            if let Ok((mask, score)) = engine.segment(e, &[(x, y, true)]) {
                let fraction = mask.area() as f32 / (mask.width as f32 * mask.height as f32);
                if score >= 0.82 && (0.0004..0.90).contains(&fraction) {
                    let _ = tx.try_send(Event::Candidate(*id, mask));
                }
            }
            if samples.is_empty() {
                let _ = tx.try_send(Event::Finished(*id));
            }
        }
    }
}
