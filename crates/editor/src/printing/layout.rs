//! Physical page geometry shared by the interactive layout and PDF writer.
use super::{Options, MAX_ITEMS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Frame {
    pub fn valid(self, paper: (f32, f32)) -> bool {
        [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(f32::is_finite)
            && self.x >= 0.0
            && self.y >= 0.0
            && self.width >= 8.0
            && self.height >= 8.0
            && self.x + self.width <= paper.0 + 0.001
            && self.y + self.height <= paper.1 + 0.001
    }
    pub fn moved(self, dx: f32, dy: f32, paper: (f32, f32)) -> Self {
        Self {
            x: (self.x + dx).clamp(0.0, (paper.0 - self.width).max(0.0)),
            y: (self.y + dy).clamp(0.0, (paper.1 - self.height).max(0.0)),
            ..self
        }
    }
    pub fn resized(self, dx: f32, dy: f32, paper: (f32, f32)) -> Self {
        Self {
            width: (self.width + dx).clamp(8.0, (paper.0 - self.x).max(8.0)),
            height: (self.height + dy).clamp(8.0, (paper.1 - self.y).max(8.0)),
            ..self
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub source: usize,
    pub page: usize,
    pub frame: Frame,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Single,
    Pair,
    Four,
    Nine,
    Contact,
    Custom,
}
impl Preset {
    pub const ALL: [Self; 6] = [
        Self::Single,
        Self::Pair,
        Self::Four,
        Self::Nine,
        Self::Contact,
        Self::Custom,
    ];
    pub fn label(self) -> &'static str {
        schist_i18n::t(match self {
            Self::Single => "printing.preset_single",
            Self::Pair => "printing.preset_pair",
            Self::Four => "printing.preset_four",
            Self::Nine => "printing.preset_nine",
            Self::Contact => "printing.preset_contact",
            Self::Custom => "common.custom",
        })
    }
    pub fn grid(self) -> Option<(usize, usize)> {
        match self {
            Self::Single => Some((1, 1)),
            Self::Pair => Some((1, 2)),
            Self::Four => Some((2, 2)),
            Self::Nine => Some((3, 3)),
            Self::Contact => Some((3, 4)),
            Self::Custom => None,
        }
    }
}
pub fn grid(options: &Options, sources: impl IntoIterator<Item = usize>) -> Vec<Placement> {
    let (pw, ph) = options.page_mm();
    let w = (pw - 2.0 * options.margin) / options.columns as f32;
    let h = (ph - 2.0 * options.margin) / options.rows as f32;
    sources
        .into_iter()
        .take(MAX_ITEMS)
        .enumerate()
        .map(|(i, source)| Placement {
            source,
            page: i / options.capacity(),
            frame: Frame {
                x: options.margin + (i % options.columns) as f32 * w,
                y: options.margin + ((i % options.capacity()) / options.columns) as f32 * h,
                width: w,
                height: h,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_paginate_and_every_frame_remains_editable() {
        let mut options = Options::default();
        for preset in Preset::ALL {
            let Some((cols, rows)) = preset.grid() else {
                continue;
            };
            options.columns = cols;
            options.rows = rows;
            let layout = grid(&options, 0..25);
            assert_eq!(layout.len(), 25);
            assert_eq!(
                layout.last().unwrap().page + 1,
                25usize.div_ceil(cols * rows)
            );
            assert!(layout.iter().all(|p| p.frame.valid(options.page_mm())));
            let frame = layout[0].frame;
            assert_ne!(frame.moved(2.0, 3.0, options.page_mm()), frame);
            assert_ne!(frame.resized(-2.0, -3.0, options.page_mm()), frame);
        }
    }
    #[test]
    fn drag_bounds_and_invalid_geometry() {
        let page = (210.0, 297.0);
        let frame = Frame {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 60.0,
        };
        assert!(frame.moved(-1000.0, 1000.0, page).valid(page));
        assert!(frame.resized(1000.0, -1000.0, page).valid(page));
        assert!(!Frame {
            x: f32::NAN,
            ..frame
        }
        .valid(page));
        assert!(!Frame {
            width: 300.0,
            ..frame
        }
        .valid(page));
    }
}
