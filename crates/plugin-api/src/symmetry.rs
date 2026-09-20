//! Document-space paint symmetry. Settings are view state, never canvas content.
use crate::brush::finite_clamp;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SymmetryMode {
    #[default]
    None,
    Vertical,
    Horizontal,
    Radial,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintSymmetry {
    pub mode: SymmetryMode,
    /// Centre as a fraction of the canvas, shared safely between documents.
    pub center: [f32; 2],
    pub segments: u8,
}

impl Default for PaintSymmetry {
    fn default() -> Self {
        Self {
            mode: SymmetryMode::None,
            center: [0.5, 0.5],
            segments: 6,
        }
    }
}

/// An orthogonal transform about the symmetry centre, also applied to tip texture.
#[derive(Debug, Clone, Copy)]
pub struct SymmetryTransform {
    pub matrix: [f32; 4],
    pub center: [f32; 2],
}

impl SymmetryTransform {
    pub fn point(self, x: f32, y: f32) -> (f32, f32) {
        let (x, y) = (x - self.center[0], y - self.center[1]);
        (
            self.center[0] + self.matrix[0] * x + self.matrix[1] * y,
            self.center[1] + self.matrix[2] * x + self.matrix[3] * y,
        )
    }

    /// Undo rotation/reflection for a tip sample's offset from the dab centre.
    pub fn inverse_offset(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.matrix[0] * x + self.matrix[2] * y,
            self.matrix[1] * x + self.matrix[3] * y,
        )
    }
}

impl PaintSymmetry {
    pub fn center_pixels(self, width: u32, height: u32) -> [f32; 2] {
        [
            finite_clamp(self.center[0], 0.0, 1.0, 0.5) * width as f32,
            finite_clamp(self.center[1], 0.0, 1.0, 0.5) * height as f32,
        ]
    }

    pub fn place(&mut self, x: f32, y: f32, width: u32, height: u32) {
        self.center = [
            finite_clamp(x / width.max(1) as f32, 0.0, 1.0, 0.5),
            finite_clamp(y / height.max(1) as f32, 0.0, 1.0, 0.5),
        ];
    }

    pub fn transforms(self, width: u32, height: u32) -> Vec<SymmetryTransform> {
        let center = self.center_pixels(width, height);
        let mut matrices = vec![[1.0, 0.0, 0.0, 1.0]];
        match self.mode {
            SymmetryMode::None => {}
            SymmetryMode::Vertical => matrices.push([-1.0, 0.0, 0.0, 1.0]),
            SymmetryMode::Horizontal => matrices.push([1.0, 0.0, 0.0, -1.0]),
            SymmetryMode::Radial => {
                let n = self.segments.clamp(2, 24);
                for i in 1..n {
                    let (s, c) = (std::f32::consts::TAU * i as f32 / n as f32).sin_cos();
                    matrices.push([c, -s, s, c]);
                }
            }
        }
        matrices
            .into_iter()
            .map(|matrix| SymmetryTransform { matrix, center })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reflection_and_radial_rotation_follow_the_moved_center() {
        let mut symmetry = PaintSymmetry {
            mode: SymmetryMode::Vertical,
            ..Default::default()
        };
        symmetry.place(30.0, 40.0, 100, 100);
        let reflected = symmetry.transforms(100, 100)[1].point(20.0, 45.0);
        assert!((reflected.0 - 40.0).abs() < 0.001);
        assert_eq!(reflected.1, 45.0);
        symmetry.mode = SymmetryMode::Radial;
        symmetry.segments = 4;
        let transforms = symmetry.transforms(100, 100);
        let rotated = transforms[1].point(40.0, 40.0);
        assert!((rotated.0 - 30.0).abs() < 0.001);
        assert!((rotated.1 - 50.0).abs() < 0.001);
        let offset = transforms[1].inverse_offset(0.0, 10.0);
        assert!((offset.0 - 10.0).abs() < 0.001);
        assert!(offset.1.abs() < 0.001);
    }
    #[test]
    fn invalid_settings_are_bounded() {
        let mut symmetry = PaintSymmetry {
            mode: SymmetryMode::Radial,
            center: [f32::NAN, 10.0],
            segments: 255,
        };
        assert_eq!(symmetry.center_pixels(100, 80), [50.0, 80.0]);
        assert_eq!(symmetry.transforms(100, 80).len(), 24);
        symmetry.place(-20.0, 150.0, 100, 80);
        assert_eq!(symmetry.center, [0.0, 1.0]);
    }
}
