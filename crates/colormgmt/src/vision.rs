//! Colour-vision-deficiency simulation (View ▸ Color Vision).
//!
//! A display-only hop after the display transform, like soft proofing:
//! it never touches document pixels or exports. The dichromacies and
//! anomalous trichromacies use the matrices Machado, Oliveira and
//! Fernandes published with "A Physiologically-based Model for
//! Simulation of Color Vision Deficiency" (IEEE TVCG 15(6), 2009),
//! tabulated at severities 0.0, 0.1 … 1.0 and linearly interpolated in
//! between, as the paper's supplementary material suggests. They act on
//! linear-light RGB with sRGB primaries, so pixels are decoded through
//! the sRGB curve, multiplied and re-encoded. Achromatopsia (rod
//! monochromacy) keeps Rec. 709 relative luminance alone.
//!
//! The hop assumes the display is close to sRGB: on a wide-gamut panel
//! the simulation is approximately, not exactly, what the paper models.

use std::sync::{Arc, OnceLock};

/// Which kind of colour vision to simulate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisionDeficiency {
    /// No L ("red") cones.
    Protanopia,
    /// No M ("green") cones.
    Deuteranopia,
    /// No S ("blue") cones.
    Tritanopia,
    /// Shifted L cones, at the simulation's severity.
    Protanomaly,
    /// Shifted M cones, at the simulation's severity.
    Deuteranomaly,
    /// Shifted S cones, at the simulation's severity.
    Tritanomaly,
    /// No cone vision: luminance only.
    Achromatopsia,
}

impl VisionDeficiency {
    pub const ALL: [VisionDeficiency; 7] = [
        Self::Protanopia,
        Self::Deuteranopia,
        Self::Tritanopia,
        Self::Protanomaly,
        Self::Deuteranomaly,
        Self::Tritanomaly,
        Self::Achromatopsia,
    ];

    /// Stable identifier, used by keymaps.
    pub fn id(self) -> &'static str {
        match self {
            Self::Protanopia => "protanopia",
            Self::Deuteranopia => "deuteranopia",
            Self::Tritanopia => "tritanopia",
            Self::Protanomaly => "protanomaly",
            Self::Deuteranomaly => "deuteranomaly",
            Self::Tritanomaly => "tritanomaly",
            Self::Achromatopsia => "achromatopsia",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }

    /// Whether the severity setting applies: the anomalous trichromacies
    /// range from near-normal to the matching dichromacy.
    pub fn anomalous(self) -> bool {
        matches!(
            self,
            Self::Protanomaly | Self::Deuteranomaly | Self::Tritanomaly
        )
    }
}

/// A deficiency at a severity, ready to apply to display pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisionSimulation {
    pub deficiency: VisionDeficiency,
    /// 0 is normal vision, 1 the full dichromacy. Only the anomalous
    /// kinds use it; the others always simulate at full strength.
    pub severity: f32,
}

// Machado et al. (2009), supplementary data, each 3x3 matrix's rows in
// order, at severities 0.0, 0.1 … 1.0.
const PROTAN: [[f32; 9]; 11] = [
    [
        1.000000, 0.000000, 0.000000, 0.000000, 1.000000, 0.000000, 0.000000, 0.000000, 1.000000,
    ],
    [
        0.856167, 0.182038, -0.038205, 0.029342, 0.955115, 0.015544, -0.002880, -0.001563, 1.004443,
    ],
    [
        0.734766, 0.334872, -0.069637, 0.051840, 0.919198, 0.028963, -0.004928, -0.004209, 1.009137,
    ],
    [
        0.630323, 0.465641, -0.095964, 0.069181, 0.890046, 0.040773, -0.006308, -0.007724, 1.014032,
    ],
    [
        0.539009, 0.579343, -0.118352, 0.082546, 0.866121, 0.051332, -0.007136, -0.011959, 1.019095,
    ],
    [
        0.458064, 0.679578, -0.137642, 0.092785, 0.846313, 0.060902, -0.007494, -0.016807, 1.024301,
    ],
    [
        0.385450, 0.769005, -0.154455, 0.100526, 0.829802, 0.069673, -0.007442, -0.022190, 1.029632,
    ],
    [
        0.319627, 0.849633, -0.169261, 0.106241, 0.815969, 0.077790, -0.007025, -0.028051, 1.035076,
    ],
    [
        0.259411, 0.923008, -0.182420, 0.110296, 0.804340, 0.085364, -0.006276, -0.034346, 1.040622,
    ],
    [
        0.203876, 0.990338, -0.194214, 0.112975, 0.794542, 0.092483, -0.005222, -0.041043, 1.046265,
    ],
    [
        0.152286, 1.052583, -0.204868, 0.114503, 0.786281, 0.099216, -0.003882, -0.048116, 1.051998,
    ],
];
const DEUTAN: [[f32; 9]; 11] = [
    [
        1.000000, 0.000000, 0.000000, 0.000000, 1.000000, 0.000000, 0.000000, 0.000000, 1.000000,
    ],
    [
        0.866435, 0.177704, -0.044139, 0.049567, 0.939063, 0.011370, -0.003453, 0.007233, 0.996220,
    ],
    [
        0.760729, 0.319078, -0.079807, 0.090568, 0.889315, 0.020117, -0.006027, 0.013325, 0.992702,
    ],
    [
        0.675425, 0.433850, -0.109275, 0.125303, 0.847755, 0.026942, -0.007950, 0.018572, 0.989378,
    ],
    [
        0.605511, 0.528560, -0.134071, 0.155318, 0.812366, 0.032316, -0.009376, 0.023176, 0.986200,
    ],
    [
        0.547494, 0.607765, -0.155259, 0.181692, 0.781742, 0.036566, -0.010410, 0.027275, 0.983136,
    ],
    [
        0.498864, 0.674741, -0.173604, 0.205199, 0.754872, 0.039929, -0.011131, 0.030969, 0.980162,
    ],
    [
        0.457771, 0.731899, -0.189670, 0.226409, 0.731012, 0.042579, -0.011595, 0.034333, 0.977261,
    ],
    [
        0.422823, 0.781057, -0.203881, 0.245752, 0.709602, 0.044646, -0.011843, 0.037423, 0.974421,
    ],
    [
        0.392952, 0.823610, -0.216562, 0.263559, 0.690210, 0.046232, -0.011910, 0.040281, 0.971630,
    ],
    [
        0.367322, 0.860646, -0.227968, 0.280085, 0.672501, 0.047413, -0.011820, 0.042940, 0.968881,
    ],
];
const TRITAN: [[f32; 9]; 11] = [
    [
        1.000000, 0.000000, 0.000000, 0.000000, 1.000000, 0.000000, 0.000000, 0.000000, 1.000000,
    ],
    [
        0.926670, 0.092514, -0.019184, 0.021191, 0.964503, 0.014306, 0.008437, 0.054813, 0.936750,
    ],
    [
        0.895720, 0.133330, -0.029050, 0.029997, 0.945400, 0.024603, 0.013027, 0.104707, 0.882266,
    ],
    [
        0.905871, 0.127791, -0.033662, 0.026856, 0.941251, 0.031893, 0.013410, 0.148296, 0.838294,
    ],
    [
        0.948035, 0.089490, -0.037526, 0.014364, 0.946792, 0.038844, 0.010853, 0.193991, 0.795156,
    ],
    [
        1.017277, 0.027029, -0.044306, -0.006113, 0.958479, 0.047634, 0.006379, 0.248708, 0.744913,
    ],
    [
        1.104996, -0.046633, -0.058363, -0.032137, 0.971635, 0.060503, 0.001336, 0.317922, 0.680742,
    ],
    [
        1.193214, -0.109812, -0.083402, -0.058496, 0.979410, 0.079086, -0.002346, 0.403492,
        0.598854,
    ],
    [
        1.257728, -0.139648, -0.118081, -0.078003, 0.975409, 0.102594, -0.003316, 0.501214,
        0.502102,
    ],
    [
        1.278864, -0.125333, -0.153531, -0.084748, 0.957674, 0.127074, -0.000989, 0.601151,
        0.399838,
    ],
    [
        1.255528, -0.076749, -0.178779, -0.078411, 0.930809, 0.147602, 0.004733, 0.691367, 0.303900,
    ],
];

/// Rec. 709 / sRGB relative luminance weights.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

impl VisionSimulation {
    pub fn new(deficiency: VisionDeficiency, anomaly_severity: f32) -> Self {
        let severity = if deficiency.anomalous() {
            anomaly_severity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        VisionSimulation {
            deficiency,
            severity,
        }
    }

    /// The linear-light RGB matrix, row-major.
    pub fn matrix(&self) -> [f32; 9] {
        use VisionDeficiency::*;
        let table = match self.deficiency {
            Protanopia | Protanomaly => &PROTAN,
            Deuteranopia | Deuteranomaly => &DEUTAN,
            Tritanopia | Tritanomaly => &TRITAN,
            Achromatopsia => {
                let mut m = [0.0; 9];
                for row in m.as_chunks_mut::<3>().0 {
                    row.copy_from_slice(&LUMA);
                }
                return m;
            }
        };
        let at = self.severity.clamp(0.0, 1.0) * 10.0;
        let low = (at.floor() as usize).min(9);
        let t = at - low as f32;
        if t == 0.0 {
            return table[low];
        }
        if t == 1.0 {
            return table[low + 1];
        }
        let mut m = [0.0; 9];
        for (i, out) in m.iter_mut().enumerate() {
            *out = table[low][i] * (1.0 - t) + table[low + 1][i] * t;
        }
        m
    }

    /// The GPU kernel's parameters: this matrix between the sRGB decode
    /// and encode tables of the ICC matrix/TRC shader.
    fn params(&self) -> Option<Vec<f32>> {
        let mut params = srgb_tables()?.as_ref().clone();
        params[..9].copy_from_slice(&self.matrix());
        Some(params)
    }

    /// The hop as a filter operation, for hosts that chain it after the
    /// proof and display transforms on the GPU.
    pub fn gpu_operation(&self) -> Option<schist_fx::FilterOperation> {
        let params = Arc::new(self.params()?);
        Some(schist_fx::FilterOperation::Captured {
            work_per_pixel: 96,
            build: Arc::new(move |w, h| {
                Some(crate::gpu::program(
                    &params,
                    w.checked_mul(h)?.checked_mul(4)?,
                ))
            }),
        })
    }

    /// Simulate on sRGB-encoded, straight-alpha f32 RGBA in place.
    /// Alpha is carried through: it is coverage, not colour.
    ///
    /// Runs on the compute backend when one is installed and the buffer
    /// is worth the trip; otherwise the CPU evaluates the same tables the
    /// shader does, so both paths agree.
    pub fn apply(&self, pixels: &mut [f32]) {
        if pixels.is_empty() || !pixels.len().is_multiple_of(4) {
            return;
        }
        if let Some(operation) = self.gpu_operation() {
            if operation.apply(pixels, pixels.len() / 4, 1) {
                return;
            }
        }
        let Some(params) = self.params() else {
            return;
        };
        for px in pixels.as_chunks_mut::<4>().0 {
            let c = [
                curve(&params, 0, px[0]),
                curve(&params, 1, px[1]),
                curve(&params, 2, px[2]),
            ];
            for (k, out) in px.iter_mut().take(3).enumerate() {
                let m = k * 3;
                *out = curve(
                    &params,
                    k + 3,
                    params[m] * c[0] + params[m + 1] * c[1] + params[m + 2] * c[2],
                );
            }
        }
    }
}

/// `gpu.wgsl`'s table lookup: decode tables truncate, encode tables round.
fn curve(params: &[f32], i: usize, value: f32) -> f32 {
    let base = params[9 + i] as usize;
    let n = params[base] as usize;
    let x = value.clamp(0.0, 1.0) * (n - 1) as f32;
    let j = (x + if i >= 3 { 0.5 } else { 0.0 }) as usize;
    params[base + 1 + j.min(n - 1)]
}

/// sRGB → sRGB kernel parameters: an identity matrix and the curve
/// tables, built once.
fn srgb_tables() -> Option<&'static Arc<Vec<f32>>> {
    static TABLES: OnceLock<Option<Arc<Vec<f32>>>> = OnceLock::new();
    TABLES
        .get_or_init(|| {
            let srgb = crate::Profile::srgb();
            crate::gpu::coefficients(&srgb.profile, &srgb.profile).map(Arc::new)
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    fn encode(v: f32) -> f32 {
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    }

    #[test]
    fn tables_preserve_neutrals() {
        // Every published matrix maps white to white: rows sum to one.
        // This also catches a transcription slip in most entries.
        for table in [&PROTAN, &DEUTAN, &TRITAN] {
            for m in table.iter() {
                for row in m.as_chunks::<3>().0 {
                    let sum: f32 = row.iter().sum();
                    assert!(close(sum, 1.0, 2e-6), "row {row:?} sums to {sum}");
                }
            }
        }
    }

    #[test]
    fn dichromacies_match_the_published_matrices() {
        // Severity is ignored for the dichromacies: always the 1.0 matrix.
        let protan = VisionSimulation::new(VisionDeficiency::Protanopia, 0.2).matrix();
        assert_eq!(
            protan,
            [
                0.152286, 1.052583, -0.204868, 0.114503, 0.786281, 0.099216, -0.003882, -0.048116,
                1.051998
            ]
        );
        let deutan = VisionSimulation::new(VisionDeficiency::Deuteranopia, 0.0).matrix();
        assert_eq!(deutan[0..3], [0.367322, 0.860646, -0.227968]);
        let tritan = VisionSimulation::new(VisionDeficiency::Tritanopia, 0.5).matrix();
        assert_eq!(tritan[6..9], [0.004733, 0.691367, 0.303900]);
    }

    #[test]
    fn anomalies_interpolate_between_tabulated_severities() {
        // Machado's 0.5 deuteranomaly matrix, exactly.
        let half = VisionSimulation::new(VisionDeficiency::Deuteranomaly, 0.5).matrix();
        let expected = [
            0.547494, 0.607765, -0.155259, 0.181692, 0.781742, 0.036566, -0.010410, 0.027275,
            0.983136,
        ];
        for (a, b) in half.iter().zip(expected) {
            assert!(close(*a, b, 1e-6));
        }
        // 0.531 lies 31% of the way from the 0.5 matrix to the 0.6 one.
        let between = VisionSimulation::new(VisionDeficiency::Deuteranomaly, 0.531).matrix();
        assert!(close(between[0], 0.69 * 0.547494 + 0.31 * 0.498864, 1e-5));
        // Zero severity is normal vision; full severity the dichromacy.
        let none = VisionSimulation::new(VisionDeficiency::Protanomaly, 0.0).matrix();
        assert_eq!(none, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            VisionSimulation::new(VisionDeficiency::Tritanomaly, 1.0).matrix(),
            VisionSimulation::new(VisionDeficiency::Tritanopia, 1.0).matrix()
        );
    }

    #[test]
    fn simulation_runs_in_linear_light() {
        // Pure sRGB red seen by a protanope: linear (0.152286, 0.114503,
        // -0.003882) after the matrix, blue clipping at zero, then
        // re-encoded. Applied to the encoded values instead, red would
        // come out near 0.15 rather than 0.43.
        let mut px = [1.0, 0.0, 0.0, 0.5];
        VisionSimulation::new(VisionDeficiency::Protanopia, 1.0).apply(&mut px);
        assert!(close(px[0], encode(0.152286), 2e-3), "{px:?}");
        assert!(close(px[1], encode(0.114503), 2e-3), "{px:?}");
        assert!(close(px[2], 0.0, 1e-6), "{px:?}");
        assert_eq!(px[3], 0.5, "alpha is coverage, not colour");
        // Deuteranopes see pure green as a dull olive.
        let mut px = [0.0, 1.0, 0.0, 1.0];
        VisionSimulation::new(VisionDeficiency::Deuteranopia, 1.0).apply(&mut px);
        assert!(close(px[0], encode(0.860646), 2e-3), "{px:?}");
        assert!(close(px[1], encode(0.672501), 2e-3), "{px:?}");
        assert!(close(px[2], encode(0.042940), 2e-3), "{px:?}");
    }

    #[test]
    fn greys_survive_and_achromatopsia_keeps_luminance() {
        for kind in VisionDeficiency::ALL {
            let mut px = [0.5, 0.5, 0.5, 1.0];
            VisionSimulation::new(kind, 0.6).apply(&mut px);
            for c in &px[..3] {
                assert!(close(*c, 0.5, 3e-3), "{kind:?} moved grey to {px:?}");
            }
        }
        // Pure green's luminance is 0.7152 in linear light.
        let mut px = [0.0, 1.0, 0.0, 1.0];
        VisionSimulation::new(VisionDeficiency::Achromatopsia, 0.3).apply(&mut px);
        assert!(close(px[0], px[1], 1e-6) && close(px[1], px[2], 1e-6));
        assert!(close(px[0], encode(0.7152), 2e-3), "{px:?}");
    }

    #[test]
    fn gpu_operation_is_described() {
        // The browser chains this after the proof and display hops.
        let op = VisionSimulation::new(VisionDeficiency::Tritanomaly, 0.4).gpu_operation();
        assert!(op.is_some_and(|op| op.program(4, 4).is_some()));
    }

    #[test]
    fn ids_round_trip() {
        for kind in VisionDeficiency::ALL {
            assert_eq!(VisionDeficiency::from_id(kind.id()), Some(kind));
        }
        assert_eq!(VisionDeficiency::from_id("normal"), None);
    }
}
