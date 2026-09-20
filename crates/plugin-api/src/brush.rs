//! Portable brush recipes shared by the paint engine and preset storage.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrushTip {
    #[default]
    Round,
    Grain,
    Bristles,
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
}

impl Default for BrushDynamics {
    fn default() -> Self {
        Self {
            tip: BrushTip::Round,
            spacing: 0.15,
            scatter: 0.0,
            stabilization: 0.0,
            pressure_gamma: 1.0,
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
}

impl Default for BrushPreset {
    fn default() -> Self {
        Self {
            name: String::new(),
            size: 24.0,
            hardness: 0.5,
            opacity: 1.0,
            dynamics: BrushDynamics::default(),
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
        self
    }

    pub fn apply(&self, state: &mut super::EditorState) {
        let preset = self.clone().sanitized();
        state.brush_size = preset.size;
        state.brush_hardness = preset.hardness;
        state.tool_opacity = preset.opacity;
        state.brush_dynamics = preset.dynamics;
    }
}
