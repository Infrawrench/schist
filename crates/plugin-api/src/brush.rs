//! Portable brush recipes shared by the paint engine and preset storage.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrushTip {
    #[default]
    Round,
    Grain,
    Bristles,
    Bitmap,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushDynamics {
    pub tip: BrushTip,
    /// Dab separation as a fraction of the full brush diameter.
    pub spacing: f32,
    /// Maximum scattered distance as a fraction of the full diameter.
    pub scatter: f32,
    /// Spatial smoothing distance in document pixels; zero disables it.
    pub stabilization: f32,
    /// Size response is pressure.powf(gamma); one is linear.
    pub pressure_gamma: f32,
    /// Multiply dab coverage by raw pressure before stroke coverage accumulation.
    pub pressure_opacity: bool,
    /// Clockwise tip angle in degrees.
    pub rotation: f32,
    /// Add the pen tilt azimuth when the host supplies it.
    pub tilt_rotation: bool,
}

impl Default for BrushDynamics {
    fn default() -> Self {
        Self {
            tip: BrushTip::Round,
            spacing: 0.15,
            scatter: 0.0,
            stabilization: 0.0,
            pressure_gamma: 1.0,
            pressure_opacity: false,
            rotation: 0.0,
            tilt_rotation: false,
        }
    }
}

pub fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

impl BrushDynamics {
    pub fn sanitized(self) -> Self {
        Self {
            tip: self.tip,
            spacing: finite_clamp(self.spacing, 0.02, 2.0, 0.15),
            scatter: finite_clamp(self.scatter, 0.0, 2.0, 0.0),
            stabilization: finite_clamp(self.stabilization, 0.0, 64.0, 0.0),
            pressure_gamma: finite_clamp(self.pressure_gamma, 0.25, 4.0, 1.0),
            pressure_opacity: self.pressure_opacity,
            rotation: finite_clamp(self.rotation, -180.0, 180.0, 0.0),
            tilt_rotation: self.tilt_rotation,
        }
    }

    pub fn pressure_size(self, pressure: f32) -> f32 {
        finite_clamp(pressure, 0.0, 1.0, 0.0).powf(self.sanitized().pressure_gamma)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushPreset {
    pub name: String,
    pub size: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub dynamics: BrushDynamics,
    pub bitmap: Option<std::sync::Arc<BrushBitmap>>,
}

impl Default for BrushPreset {
    fn default() -> Self {
        Self {
            name: String::new(),
            size: 24.0,
            hardness: 0.5,
            opacity: 1.0,
            dynamics: BrushDynamics::default(),
            bitmap: None,
        }
    }
}

impl BrushPreset {
    pub fn capture(name: String, state: &super::EditorState) -> Self {
        Self {
            name,
            size: state.brush_size,
            hardness: state.brush_hardness,
            opacity: state.tool_opacity,
            dynamics: state.brush_dynamics,
            bitmap: state.brush_bitmap.clone(),
        }
        .sanitized()
    }

    pub fn sanitized(mut self) -> Self {
        self.name = self
            .name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        self.size = finite_clamp(self.size, 1.0, 500.0, 24.0);
        self.hardness = finite_clamp(self.hardness, 0.0, 1.0, 0.5);
        self.opacity = finite_clamp(self.opacity, 0.0, 1.0, 1.0);
        self.dynamics = self.dynamics.sanitized();
        if self.bitmap.as_ref().is_some_and(|b| !b.valid()) {
            self.bitmap = None;
        }
        if self.dynamics.tip == BrushTip::Bitmap && self.bitmap.is_none() {
            self.dynamics.tip = BrushTip::Round;
        }
        self
    }

    pub fn apply(&self, state: &mut super::EditorState) {
        let preset = self.clone().sanitized();
        state.brush_size = preset.size;
        state.brush_hardness = preset.hardness;
        state.tool_opacity = preset.opacity;
        state.brush_dynamics = preset.dynamics;
        state.brush_bitmap = preset.bitmap;
    }
}

/// A portable grayscale mask. Zero is transparent; 255 deposits full ink.
/// The longest edge maps to the brush diameter; aspect ratio is preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushBitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub const MAX_BITMAP_SIDE: u32 = 1024;

impl BrushBitmap {
    pub fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= MAX_BITMAP_SIDE
            && self.height <= MAX_BITMAP_SIDE
            && self.pixels.len() == self.width as usize * self.height as usize
    }

    /// Bilinear sample in tip coordinates (-1..1), with transparent exterior.
    pub fn coverage(&self, u: f32, v: f32) -> f32 {
        if !u.is_finite() || !v.is_finite() || !self.valid() {
            return 0.0;
        }
        let scale = self.width.max(self.height) as f32 / 2.0;
        let x = u * scale + self.width as f32 / 2.0 - 0.5;
        let y = v * scale + self.height as f32 / 2.0 - 0.5;
        if x < -1.0 || y < -1.0 || x > self.width as f32 || y > self.height as f32 {
            return 0.0;
        }
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let sample = |x: i32, y: i32| {
            if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
                0.0
            } else {
                self.pixels[y as usize * self.width as usize + x as usize] as f32 / 255.0
            }
        };
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let top = sample(ix, iy) * (1.0 - fx) + sample(ix + 1, iy) * fx;
        let bottom = sample(ix, iy + 1) * (1.0 - fx) + sample(ix + 1, iy + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// Pen tilt in screen-axis degrees, following W3C Pointer Events. A vertical
/// pen, missing data, and invalid input have no azimuth; callers retain angle.
/// Returns radians, clockwise from the positive screen x axis.
pub fn tilt_azimuth(tilt: Option<[f32; 2]>) -> Option<f32> {
    let [x, y] = tilt?;
    if !x.is_finite()
        || !y.is_finite()
        || x.abs() > 90.0
        || y.abs() > 90.0
        || x.abs() + y.abs() < 0.001
    {
        return None;
    }
    // Avoid tan(90 degrees)'s numerical sign discontinuity.
    Some(
        y.clamp(-89.999, 89.999)
            .to_radians()
            .tan()
            .atan2(x.clamp(-89.999, 89.999).to_radians().tan()),
    )
}

/// Rotate screen-axis pen tilt into document axes before passing it to tools.
/// Both input tilt and the result are in degrees; view rotation is radians.
pub fn tilt_in_document(tilt: Option<[f32; 2]>, view_rotation: f32) -> Option<[f32; 2]> {
    let [x, y] = tilt?;
    if !x.is_finite()
        || !y.is_finite()
        || !view_rotation.is_finite()
        || x.abs() > 90.0
        || y.abs() > 90.0
    {
        return None;
    }
    let x = x.clamp(-89.999, 89.999).to_radians().tan();
    let y = y.clamp(-89.999, 89.999).to_radians().tan();
    let (sin, cos) = view_rotation.sin_cos();
    Some([
        (x * cos + y * sin).atan().to_degrees(),
        (-x * sin + y * cos).atan().to_degrees(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tilt_azimuth_uses_projected_pen_direction_and_rejects_bad_samples() {
        assert!(tilt_azimuth(None).is_none());
        assert!(tilt_azimuth(Some([0.0, 0.0])).is_none());
        assert!(tilt_azimuth(Some([f32::NAN, 0.0])).is_none());
        assert!(tilt_azimuth(Some([91.0, 0.0])).is_none());
        assert_eq!(tilt_azimuth(Some([45.0, 0.0])), Some(0.0));
        let angle = tilt_azimuth(Some([30.0, 60.0])).unwrap();
        assert!((angle - 3.0_f32.atan()).abs() < 0.0001);
        assert!(
            (tilt_azimuth(Some([0.0, 90.0])).unwrap() - std::f32::consts::FRAC_PI_2).abs() < 0.0001
        );
    }

    #[test]
    fn pen_direction_tracks_screen_axes_when_the_canvas_is_rotated() {
        let tilt = tilt_in_document(Some([45.0, 0.0]), std::f32::consts::FRAC_PI_2);
        assert!((tilt_azimuth(tilt).unwrap() + std::f32::consts::FRAC_PI_2).abs() < 0.0001);
        assert_eq!(tilt_in_document(Some([0.0, 0.0]), 1.0), Some([0.0, 0.0]));
        assert!(tilt_in_document(Some([0.0, 0.0]), f32::NAN).is_none());
        assert!(tilt_in_document(None, 1.0).is_none());
    }

    #[test]
    fn malformed_bitmap_samples_are_safe_and_presets_reset_missing_masks() {
        let bitmap = BrushBitmap {
            width: u32::MAX,
            height: 1,
            pixels: vec![],
        };
        assert!(!bitmap.valid());
        assert_eq!(bitmap.coverage(0.0, 0.0), 0.0);
        let preset = BrushPreset {
            bitmap: Some(std::sync::Arc::new(bitmap)),
            dynamics: BrushDynamics {
                tip: BrushTip::Bitmap,
                rotation: f32::NAN,
                ..Default::default()
            },
            ..Default::default()
        }
        .sanitized();
        assert_eq!(preset.dynamics.tip, BrushTip::Round);
        assert_eq!(preset.dynamics.rotation, 0.0);
        assert!(preset.bitmap.is_none());
    }
}
