use crate::{Frame, Mask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Region {
    pub fn new(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        frame_width: u32,
        frame_height: u32,
    ) -> Result<Self, String> {
        if width == 0
            || height == 0
            || x.checked_add(width).is_none_or(|r| r > frame_width)
            || y.checked_add(height).is_none_or(|b| b > frame_height)
        {
            return Err("Invalid crop bounds".into());
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }
    pub fn full(frame: &Frame) -> Self {
        Self {
            x: 0,
            y: 0,
            width: frame.width,
            height: frame.height,
        }
    }
    pub fn including(self, points: &[(f32, f32, bool)], width: u32, height: u32) -> Self {
        let (mut x, mut y, mut r, mut b) =
            (self.x, self.y, self.x + self.width, self.y + self.height);
        for &(px, py, _) in points {
            if px.is_finite()
                && py.is_finite()
                && px >= 0.0
                && py >= 0.0
                && px < width as f32
                && py < height as f32
            {
                x = x.min((px as u32).saturating_sub(8));
                y = y.min((py as u32).saturating_sub(8));
                r = r.max((px as u32 + 9).min(width));
                b = b.max((py as u32 + 9).min(height));
            }
        }
        Self {
            x,
            y,
            width: r - x,
            height: b - y,
        }
    }
    pub fn crop(self, frame: &Frame) -> Result<Frame, String> {
        Self::new(
            self.x,
            self.y,
            self.width,
            self.height,
            frame.width,
            frame.height,
        )?;
        let mut rgba = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in self.y..self.y + self.height {
            let start = ((y * frame.width + self.x) * 4) as usize;
            rgba.extend_from_slice(&frame.rgba[start..start + (self.width * 4) as usize]);
        }
        Frame::new(self.width, self.height, rgba)
    }
    pub fn local_point(self, (x, y, positive): (f32, f32, bool)) -> Option<(f32, f32, bool)> {
        if x >= self.x as f32
            && y >= self.y as f32
            && x < (self.x + self.width) as f32
            && y < (self.y + self.height) as f32
        {
            Some((x - self.x as f32, y - self.y as f32, positive))
        } else {
            None
        }
    }
    pub fn restore(self, mask: &Mask, width: u32, height: u32) -> Result<Mask, String> {
        Self::new(self.x, self.y, self.width, self.height, width, height)?;
        if mask.width != self.width || mask.height != self.height {
            return Err("Crop mask dimensions differ".into());
        }
        let mut alpha = vec![0; (width * height) as usize];
        for y in 0..self.height {
            let dst = ((self.y + y) * width + self.x) as usize;
            let src = (y * self.width) as usize;
            alpha[dst..dst + self.width as usize]
                .copy_from_slice(&mask.alpha[src..src + self.width as usize]);
        }
        Mask::new(width, height, alpha)
    }
    pub fn around_mask(mask: &Mask, padding: f32) -> Option<Self> {
        Self::around_threshold(mask, padding, 127)
    }
    pub fn around_visible(mask: &Mask, padding: f32) -> Option<Self> {
        Self::around_threshold(mask, padding, 0)
    }
    fn around_threshold(mask: &Mask, padding: f32, threshold: u8) -> Option<Self> {
        let (mut left, mut top, mut right, mut bottom) = (mask.width, mask.height, 0, 0);
        for y in 0..mask.height {
            for x in 0..mask.width {
                if mask.alpha[(y * mask.width + x) as usize] > threshold {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x + 1);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        if right == 0 {
            return None;
        }
        let px = (((right - left) as f32 * padding).ceil() as u32).max(16);
        let py = (((bottom - top) as f32 * padding).ceil() as u32).max(16);
        let x = left.saturating_sub(px);
        let y = top.saturating_sub(py);
        let r = right.saturating_add(px).min(mask.width);
        let b = bottom.saturating_add(py).min(mask.height);
        Some(Self {
            x,
            y,
            width: r - x,
            height: b - y,
        })
    }
    pub fn expand(self, width: u32, height: u32) -> Self {
        let px = (self.width / 2).max(32);
        let py = (self.height / 2).max(32);
        let x = self.x.saturating_sub(px);
        let y = self.y.saturating_sub(py);
        let r = (self.x + self.width).saturating_add(px).min(width);
        let b = (self.y + self.height).saturating_add(py).min(height);
        Self {
            x,
            y,
            width: r - x,
            height: b - y,
        }
    }
    pub fn touches_boundary(self, mask: &Mask) -> bool {
        (0..mask.width).any(|x| mask.contains(x, 0) || mask.contains(x, mask.height - 1))
            || (0..mask.height).any(|y| mask.contains(0, y) || mask.contains(mask.width - 1, y))
    }
}
