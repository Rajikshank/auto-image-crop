#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}
#[derive(Default)]
pub struct Candidates {
    masks: Vec<Mask>,
}
impl Candidates {
    pub fn insert(&mut self, mask: Mask) {
        if self.masks.len() >= 48 || mask.area() == 0 {
            return;
        }
        let duplicate = self.masks.iter().any(|other| {
            if (other.width, other.height) != (mask.width, mask.height) {
                return false;
            }
            let (mut intersection, mut union) = (0, 0);
            let step = if mask.alpha.len() > 4096 { 16 } else { 1 };
            for (&a, &b) in mask.alpha.iter().zip(&other.alpha).step_by(step) {
                if a > 127 || b > 127 {
                    union += 1;
                }
                if a > 127 && b > 127 {
                    intersection += 1;
                }
            }
            union > 0 && intersection as f32 / union as f32 > 0.92
        });
        if !duplicate {
            self.masks.push(mask);
            self.masks.sort_by_cached_key(Mask::area);
        }
    }
    pub fn cycle(&self, index: usize) -> Option<&Mask> {
        if self.masks.is_empty() {
            None
        } else {
            self.masks.get(index % self.masks.len())
        }
    }
    pub fn at(&self, x: u32, y: u32, cycle: usize) -> Option<&Mask> {
        let matches: Vec<_> = self.masks.iter().filter(|m| m.contains(x, y)).collect();
        if matches.is_empty() {
            None
        } else {
            Some(matches[cycle % matches.len()])
        }
    }
    pub fn into_masks(self) -> Vec<Mask> {
        self.masks
    }
    pub fn len(&self) -> usize {
        self.masks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }
}
impl Frame {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        validate(width, height, rgba.len(), 4)?;
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
}
impl Mask {
    pub fn new(width: u32, height: u32, alpha: Vec<u8>) -> Result<Self, String> {
        validate(width, height, alpha.len(), 1)?;
        Ok(Self {
            width,
            height,
            alpha,
        })
    }
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height && self.alpha[(y * self.width + x) as usize] > 127
    }
    pub fn area(&self) -> usize {
        self.alpha.iter().filter(|&&a| a > 127).count()
    }
    pub fn interior_point(&self) -> Option<(u32, u32)> {
        self.point_above(127)
    }
    pub fn visible_point(&self) -> Option<(u32, u32)> {
        self.point_above(0)
    }
    fn point_above(&self, threshold: u8) -> Option<(u32, u32)> {
        let (mut sum_x, mut sum_y, mut count) = (0u64, 0u64, 0u64);
        for (i, &alpha) in self.alpha.iter().enumerate() {
            if alpha > threshold {
                sum_x += i as u64 % u64::from(self.width);
                sum_y += i as u64 / u64::from(self.width);
                count += 1;
            }
        }
        if count == 0 {
            return None;
        }
        let center = (sum_x / count, sum_y / count);
        self.alpha
            .iter()
            .enumerate()
            .filter(|(_, a)| **a > threshold)
            .min_by_key(|(i, _)| {
                let x = *i as u64 % u64::from(self.width);
                let y = *i as u64 / u64::from(self.width);
                x.abs_diff(center.0).pow(2) + y.abs_diff(center.1).pow(2)
            })
            .map(|(i, _)| (i as u32 % self.width, i as u32 / self.width))
    }
}
fn validate(width: u32, height: u32, len: usize, channels: usize) -> Result<(), String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(channels));
    if width == 0 || height == 0 || expected != Some(len) || len > 512 * 1024 * 1024 {
        return Err("Invalid image dimensions or buffer length".into());
    }
    Ok(())
}
pub fn extract(frame: &Frame, mask: &Mask) -> Result<Frame, String> {
    validate(frame.width, frame.height, frame.rgba.len(), 4)?;
    validate(mask.width, mask.height, mask.alpha.len(), 1)?;
    if (frame.width, frame.height) != (mask.width, mask.height) {
        return Err("Mask does not belong to this frame".into());
    }
    let mut left = frame.width;
    let mut top = frame.height;
    let mut right = 0;
    let mut bottom = 0;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let i = (y * frame.width + x) as usize;
            if u16::from(mask.alpha[i]) * u16::from(frame.rgba[i * 4 + 3]) / 255 > 0 {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    if right == 0 {
        return Err("The selection is empty".into());
    }
    let (width, height) = (right - left, bottom - top);
    let mut rgba = vec![0; (width * height * 4) as usize];
    for y in top..bottom {
        for x in left..right {
            let src = (y * frame.width + x) as usize;
            let dst = ((y - top) * width + x - left) as usize * 4;
            let alpha =
                (u16::from(mask.alpha[src]) * u16::from(frame.rgba[src * 4 + 3]) / 255) as u8;
            if alpha > 0 {
                rgba[dst..dst + 3].copy_from_slice(&frame.rgba[src * 4..src * 4 + 3]);
                rgba[dst + 3] = alpha;
            }
        }
    }
    Frame::new(width, height, rgba)
}
#[cfg(windows)]
pub mod app;
pub mod inference;
#[cfg(windows)]
pub mod native_probe;
#[cfg(windows)]
pub mod platform;
pub mod settings;
pub mod worker;

#[derive(Default)]
pub struct Session {
    serial: u64,
    active: bool,
    revision: u64,
}
impl Session {
    pub fn begin(&mut self) -> u64 {
        self.serial = self.serial.wrapping_add(1);
        self.active = true;
        self.revision = 0;
        self.serial
    }
    pub fn cancel(&mut self) {
        self.active = false;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn accepts(&self, id: u64) -> bool {
        self.active && self.serial == id
    }
    pub fn accepts_result(&self, id: u64, revision: u64) -> bool {
        self.accepts(id) && self.revision == revision
    }
    pub fn revise(&mut self) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        self.revision
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn id(&self) -> u64 {
        self.serial
    }
}
pub mod advanced;
pub mod dock;
pub mod highlight;
pub mod quality;
pub mod refinement;
pub mod roi;

pub mod device;
pub mod editor;
pub mod evaluation;
pub mod iterative;
pub mod native_dock;
pub mod tiles;

pub mod motion;
pub mod presentation;

pub mod file_export;
pub mod ui_paint;
