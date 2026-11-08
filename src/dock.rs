//! Shared paint/hit geometry in physical pixels, scaled from device-independent units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Select,
    Brush,
    Mode,
    Copy,
    Save,
    Close,
    Refine,
    Inspect,
    Undo,
    Redo,
    More,
    Add,
    Remove,
    Edge,
    Done,
    Lasso,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Discover,
    Selected,
    Refine,
}
pub type Bounds = (i32, i32, i32, i32);
pub struct Dock {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    scale: f32,
    columns: i32,
    actions: Vec<Action>,
    stage: Option<Stage>,
}
impl Dock {
    pub fn new(width: u32, scale: f32) -> Self {
        let scale = scale.clamp(0.75, 4.0);
        let available = (width as i32 - 16).max(6);
        let columns = if available >= (640.0 * scale) as i32 {
            6
        } else {
            3
        };
        let w = ((640.0 * scale * columns as f32 / 6.0) as i32).min(available);
        let left = (width as i32 - w) / 2;
        let top = (20.0 * scale) as i32;
        let height = ((if columns == 6 { 96.0 } else { 148.0 }) * scale) as i32;
        Self {
            left,
            top,
            right: left + w,
            bottom: top + height,
            scale,
            columns,
            actions: vec![
                Action::Select,
                Action::Brush,
                Action::Mode,
                Action::Copy,
                Action::Save,
                Action::Close,
            ],
            stage: None,
        }
    }
    pub fn context(
        width: u32,
        height: u32,
        scale: f32,
        stage: Stage,
        selection: Option<Bounds>,
    ) -> Self {
        let mut actions = match stage {
            Stage::Discover => vec![Action::Select, Action::Lasso, Action::Close],
            Stage::Selected => vec![
                Action::Refine,
                Action::Inspect,
                Action::Copy,
                Action::More,
                Action::Close,
            ],
            Stage::Refine => vec![
                Action::Add,
                Action::Remove,
                Action::Edge,
                Action::Undo,
                Action::Redo,
                Action::Done,
            ],
        };
        let scale = scale.clamp(0.75, 4.);
        let available = (width as i32 - 16).max(6);
        if stage == Stage::Refine && (height as f32) < 200. * scale {
            actions.retain(|a| !matches!(a, Action::Undo | Action::Redo));
        }
        let count = actions.len() as i32;
        let columns =
            (((available as f32 - 16. * scale) / (48. * scale)).floor() as i32).clamp(1, count);
        let w = ((columns as f32 * 84. + 16.) * scale) as i32;
        let w = w.min(available);
        let rows = (count + columns - 1) / columns;
        let footer = match stage {
            Stage::Discover => 36.,
            Stage::Selected => 16.,
            Stage::Refine => 52.,
        };
        let h = ((rows as f32 * 48. + footer) * scale) as i32;
        let mut left = (width as i32 - w) / 2;
        let mut top = 16;
        if let Some((x, y, r, b)) = selection {
            left = ((x + r - w) / 2).clamp(8, (width as i32 - w - 8).max(8));
            top = if b + 12 + h < height as i32 {
                b + 12
            } else if y - h - 12 > 8 {
                y - h - 12
            } else {
                height as i32 - h - 8
            };
        }
        top = top.clamp(8, (height as i32 - h - 8).max(8));
        Self {
            left,
            top,
            right: left + w,
            bottom: top + h,
            scale,
            columns,
            actions,
            stage: Some(stage),
        }
    }
    pub fn slider_step(&self) -> i32 {
        (28. * self.scale) as i32
    }
    pub fn slider(&self) -> Option<Bounds> {
        if self.stage == Some(Stage::Refine) {
            let y = self.buttons().last()?.1.3 + (6. * self.scale) as i32;
            Some((
                self.left + (96. * self.scale) as i32,
                y,
                self.right - (12. * self.scale) as i32,
                y + (24. * self.scale) as i32,
            ))
        } else {
            None
        }
    }
    pub fn place(&mut self, x: i32, y: i32) {
        let dx = x - self.left;
        let dy = y - self.top;
        self.left += dx;
        self.right += dx;
        self.top += dy;
        self.bottom += dy;
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    pub fn buttons(&self) -> Vec<(Action, Bounds)> {
        let padding = (8.0 * self.scale) as i32;
        let gap = (4.0 * self.scale) as i32;
        let w = (self.right - self.left - 2 * padding) / self.columns;
        let h = (44.0 * self.scale) as i32;
        self.actions
            .iter()
            .copied()
            .enumerate()
            .map(|(i, a)| {
                let x = self.left + padding + (i as i32 % self.columns) * w;
                let y = self.top + padding + (i as i32 / self.columns) * (h + gap);
                (a, (x, y, x + w - gap, y + h))
            })
            .collect()
    }
    pub fn hit(&self, x: i32, y: i32) -> Option<Action> {
        self.buttons()
            .into_iter()
            .find(|(_, r)| x >= r.0 && y >= r.1 && x < r.2 && y < r.3)
            .map(|b| b.0)
    }
}
