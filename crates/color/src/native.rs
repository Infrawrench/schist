//! Native raster samples. Ink and encoded Lab values are normalized, alpha
//! is independent. Lab L = sample*100; a/b = sample*255-128 (D50).
use crate::{convert, ColorMode, Rgba};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativePixel {
    pub mode: ColorMode,
    pub color: [f32; 4],
    pub alpha: f32,
}

impl NativePixel {
    pub fn transparent(mode: ColorMode) -> Self {
        Self {
            mode,
            color: [0.0; 4],
            alpha: 0.0,
        }
    }

    pub fn from_rgba(mode: ColorMode, px: Rgba) -> Self {
        let color = match mode {
            ColorMode::Cmyk => convert::rgb_to_cmyk(px),
            ColorMode::Lab => {
                let lab = convert::rgb_to_lab_d50(px);
                [
                    lab[0] / 100.0,
                    (lab[1] + 128.0) / 255.0,
                    (lab[2] + 128.0) / 255.0,
                    0.0,
                ]
            }
            _ => [px.r, px.g, px.b, 0.0],
        };
        Self {
            mode,
            color,
            alpha: px.a,
        }
    }

    pub fn to_rgba(self) -> Rgba {
        match self.mode {
            ColorMode::Cmyk => convert::cmyk_to_rgb(self.color, self.alpha),
            ColorMode::Lab => convert::lab_d50_to_rgb(
                [
                    self.color[0] * 100.0,
                    self.color[1] * 255.0 - 128.0,
                    self.color[2] * 255.0 - 128.0,
                ],
                self.alpha,
            ),
            _ => Rgba::new(self.color[0], self.color[1], self.color[2], self.alpha),
        }
    }

    pub fn converted(self, mode: ColorMode) -> Self {
        if self.mode == mode {
            self
        } else {
            Self::from_rgba(mode, self.to_rgba())
        }
    }

    /// Source-over in native channels, without an RGB round trip.
    pub fn over(self, bottom: Self) -> Self {
        assert_eq!(self.mode, bottom.mode);
        let alpha = self.alpha + bottom.alpha * (1.0 - self.alpha);
        if alpha <= f32::EPSILON {
            return Self::transparent(self.mode);
        }
        let mut out = self;
        for c in 0..4 {
            out.color[c] = (self.color[c] * self.alpha
                + bottom.color[c] * bottom.alpha * (1.0 - self.alpha))
                / alpha;
        }
        out.alpha = alpha;
        out
    }
}
