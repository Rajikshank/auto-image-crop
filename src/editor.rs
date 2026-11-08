//! Accepted masks and one authoritative, quantized cutout. UI decoration never enters this result.
use crate::{Frame, Mask};
use std::sync::Arc;
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}
impl Default for View {
    fn default() -> Self {
        Self {
            scale: 1.,
            x: 0.,
            y: 0.,
        }
    }
}
impl View {
    pub fn image_point(self, (x, y): (f32, f32)) -> (f32, f32) {
        (x / self.scale + self.x, y / self.scale + self.y)
    }
    pub fn screen_point(self, (x, y): (f32, f32)) -> (f32, f32) {
        ((x - self.x) * self.scale, (y - self.y) * self.scale)
    }
    pub fn zoom_at(&mut self, scale: f32, cursor: (f32, f32), size: (u32, u32)) {
        let image = self.image_point(cursor);
        self.scale = scale.clamp(1., 8.);
        self.x =
            (image.0 - cursor.0 / self.scale).clamp(0., size.0 as f32 * (1. - 1. / self.scale));
        self.y =
            (image.1 - cursor.1 / self.scale).clamp(0., size.1 as f32 * (1. - 1. / self.scale));
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    Checker,
    Dark,
    Light,
}
impl Background {
    pub fn next(self) -> Self {
        match self {
            Self::Checker => Self::Dark,
            Self::Dark => Self::Light,
            Self::Light => Self::Checker,
        }
    }
}
#[derive(Clone)]
pub struct Ready {
    pub image: Arc<Frame>,
    pub x: u32,
    pub y: u32,
    pub revision: u64,
}
enum Change {
    Stroke(Vec<(usize, Option<u8>, Option<u8>)>),
    Base(Vec<(usize, u8, u8)>),
}
impl Change {
    fn bytes(&self) -> usize {
        match self {
            Self::Stroke(d) => {
                d.capacity() * std::mem::size_of::<(usize, Option<u8>, Option<u8>)>()
            }
            Self::Base(d) => d.capacity() * std::mem::size_of::<(usize, u8, u8)>(),
        }
    }
}
pub struct Editor {
    frame: Arc<Frame>,
    base: Mask,
    overrides: Vec<Option<u8>>,
    undo: Vec<Change>,
    redo: Vec<Change>,
    revision: u64,
    colors: bool,
    ready: Option<Ready>,
}
impl Editor {
    pub fn new(frame: Arc<Frame>, mask: Mask, colors: bool) -> Result<Self, String> {
        if (frame.width, frame.height) != (mask.width, mask.height)
            || mask.alpha.len() != frame.width as usize * frame.height as usize
        {
            return Err("Selection dimensions differ".into());
        }
        let overrides = vec![None; mask.alpha.len()];
        Ok(Self {
            frame,
            base: mask,
            overrides,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 1,
            colors,
            ready: None,
        })
    }
    pub fn set_colors(&mut self, colors: bool) {
        if self.colors != colors {
            self.colors = colors;
            self.changed();
        }
    }
    pub fn mask(&self) -> Mask {
        let mut mask = self.base.clone();
        for (i, alpha) in self.overrides.iter().enumerate() {
            if let Some(alpha) = alpha {
                mask.alpha[i] = *alpha;
            }
        }
        mask
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.ready = None;
    }
    fn push(&mut self, change: Change) {
        self.redo.clear();
        self.undo.push(change);
        while self.history_bytes() > 64 * 1024 * 1024 {
            self.undo.remove(0);
        }
        self.changed();
    }
    pub fn history_bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(Change::bytes).sum()
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn paint(&mut self, points: &[(f32, f32)], radius: f32, alpha: u8) -> Result<(), String> {
        if points.len() > 4096
            || points.iter().any(|p| {
                p.0 < 0.
                    || p.1 < 0.
                    || p.0 >= self.frame.width as f32
                    || p.1 >= self.frame.height as f32
            })
            || points.is_empty()
            || !radius.is_finite()
            || !(0.25..=256.).contains(&radius)
            || points.iter().any(|p| !p.0.is_finite() || !p.1.is_finite())
        {
            return Err("Invalid brush stroke".into());
        }
        let mut touched = vec![false; self.base.alpha.len()];
        let mut delta = Vec::new();
        let w = self.frame.width as i32;
        let h = self.frame.height as i32;
        let mut stamp = |x: f32, y: f32| {
            for py in ((y - radius).floor() as i32).max(0)..=((y + radius).ceil() as i32).min(h - 1)
            {
                for px in
                    ((x - radius).floor() as i32).max(0)..=((x + radius).ceil() as i32).min(w - 1)
                {
                    if (px as f32 - x).powi(2) + (py as f32 - y).powi(2) <= radius * radius {
                        let i = (py * w + px) as usize;
                        if !touched[i] {
                            touched[i] = true;
                            if self.overrides[i] != Some(alpha) {
                                delta.push((i, self.overrides[i], Some(alpha)));
                            }
                        }
                    }
                }
            }
        };
        stamp(points[0].0, points[0].1);
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let distance = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let steps = (distance / (radius * 0.5).max(0.5)).ceil() as usize;
            for step in 1..=steps {
                let t = step as f32 / steps as f32;
                stamp(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            }
        }
        for &(i, _, new) in &delta {
            self.overrides[i] = new;
        }
        if !delta.is_empty() {
            self.push(Change::Stroke(delta));
        }
        Ok(())
    }
    pub fn accept(&mut self, mask: Mask) -> Result<(), String> {
        if (mask.width, mask.height) != (self.frame.width, self.frame.height)
            || mask.alpha.len() != self.base.alpha.len()
        {
            return Err("Prediction dimensions differ".into());
        }
        let delta = self
            .base
            .alpha
            .iter()
            .zip(&mask.alpha)
            .enumerate()
            .filter(|(_, p)| p.0 != p.1)
            .map(|(i, (&a, &b))| (i, a, b))
            .collect::<Vec<_>>();
        self.base = mask;
        if !delta.is_empty() {
            self.push(Change::Base(delta));
        }
        Ok(())
    }
    pub fn accept_smart(&mut self, prediction: Mask, add: bool) -> Result<(), String> {
        self.accept_smart_in(prediction, add, None)
    }
    pub fn accept_smart_in(
        &mut self,
        mut prediction: Mask,
        add: bool,
        region: Option<crate::roi::Region>,
    ) -> Result<(), String> {
        if let Some(r) = region {
            crate::roi::Region::new(
                r.x,
                r.y,
                r.width,
                r.height,
                self.frame.width,
                self.frame.height,
            )?;
        }

        if (prediction.width, prediction.height) != (self.frame.width, self.frame.height)
            || prediction.alpha.len() != self.base.alpha.len()
        {
            return Err("Prediction dimensions differ".into());
        }
        let current = self.mask();
        for (i, (new, old)) in prediction.alpha.iter_mut().zip(current.alpha).enumerate() {
            let x = i as u32 % prediction.width;
            let y = i as u32 / prediction.width;
            let inside = region
                .is_none_or(|r| x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height);
            *new = if !inside {
                old
            } else if add {
                (*new).max(old)
            } else {
                (*new).min(old)
            };
        }
        self.accept(prediction)
    }
    fn apply(&mut self, change: &Change, forward: bool) {
        match change {
            Change::Stroke(delta) => {
                for &(i, a, b) in delta {
                    self.overrides[i] = if forward { b } else { a };
                }
            }
            Change::Base(delta) => {
                for &(i, a, b) in delta {
                    self.base.alpha[i] = if forward { b } else { a };
                }
            }
        }
        self.changed();
    }
    pub fn undo(&mut self) -> bool {
        if let Some(c) = self.undo.pop() {
            self.apply(&c, false);
            self.redo.push(c);
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(c) = self.redo.pop() {
            self.apply(&c, true);
            self.undo.push(c);
            true
        } else {
            false
        }
    }
    pub fn source(&self) -> Arc<Frame> {
        self.frame.clone()
    }
    pub fn ready_ref(&self) -> Option<&Ready> {
        self.ready.as_ref()
    }
    pub fn ready(&mut self) -> Result<&Ready, String> {
        if self.ready.is_none() {
            let mask = self.mask();
            let recovered;
            let source = if self.colors {
                recovered = crate::quality::recover_colors(&self.frame, &mask);
                &recovered
            } else {
                self.frame.as_ref()
            };
            let image = crate::extract(source, &mask)?;
            let mut x = mask.width;
            let mut y = mask.height;
            for (i, &alpha) in mask.alpha.iter().enumerate() {
                if u16::from(alpha) * u16::from(self.frame.rgba[i * 4 + 3]) / 255 > 0 {
                    x = x.min(i as u32 % mask.width);
                    y = y.min(i as u32 / mask.width);
                }
            }
            self.ready = Some(Ready {
                image: Arc::new(image),
                x,
                y,
                revision: self.revision,
            });
        }
        Ok(self.ready.as_ref().unwrap())
    }
    pub fn inspect(&mut self, background: Background) -> Result<Vec<u8>, String> {
        let width = self.frame.width;
        let height = self.frame.height;
        let ready = self.ready()?;
        let mut output = vec![0; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let bg = match background {
                    Background::Dark => 24,
                    Background::Light => 245,
                    Background::Checker => {
                        if (x / 12 + y / 12) % 2 == 0 {
                            60
                        } else {
                            84
                        }
                    }
                };
                let dst = ((y * width + x) * 4) as usize;
                output[dst..dst + 4].copy_from_slice(&[bg, bg, bg, 255]);
                if x >= ready.x
                    && y >= ready.y
                    && x < ready.x + ready.image.width
                    && y < ready.y + ready.image.height
                {
                    let i = (((y - ready.y) * ready.image.width + x - ready.x) * 4) as usize;
                    let a = ready.image.rgba[i + 3] as u32;
                    for c in 0..3 {
                        output[dst + c] =
                            ((ready.image.rgba[i + 2 - c] as u32 * a + bg as u32 * (255 - a) + 127)
                                / 255) as u8;
                    }
                }
            }
        }
        Ok(output)
    }
}
