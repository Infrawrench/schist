//! Page-relative ruler ticks. Positions use the pasteboard transform;
//! raster zoom, resolution and pan never enter this calculation.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RulerUnit {
    #[default]
    Millimetres,
    Points,
    Inches,
}

impl RulerUnit {
    pub fn points(self) -> f64 {
        match self {
            Self::Millimetres => 72.0 / 25.4,
            Self::Points => 1.0,
            Self::Inches => 72.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Millimetres => "design.ruler_mm",
            Self::Points => "design.ruler_pt",
            Self::Inches => "design.ruler_in",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Millimetres => Self::Points,
            Self::Points => Self::Inches,
            Self::Inches => Self::Millimetres,
        }
    }
}

#[derive(Debug)]
pub struct Tick {
    /// Pixels from the canvas edge, including the page's current pan.
    pub offset: f32,
    pub value: f64,
    pub label: Option<String>,
}

/// Major ticks stay at least 64 pixels apart; four minor ticks divide
/// each interval. Integer tick indices keep negative coordinates and
/// fractional units stable without accumulating floating-point steps.
pub fn ticks(origin: f32, scale: f32, extent: f32, unit: RulerUnit) -> Vec<Tick> {
    if !origin.is_finite()
        || !scale.is_finite()
        || scale <= 0.0
        || !extent.is_finite()
        || extent <= 0.0
    {
        return Vec::new();
    }
    let pixels = f64::from(scale) * unit.points();
    let required = 64.0 / pixels;
    let power = 10f64.powf(required.log10().floor());
    let major = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|multiple| multiple * power)
        .find(|step| *step >= required)
        .unwrap_or(power * 10.0);
    let minor = major / 5.0;
    let first = (-f64::from(origin) / (minor * pixels)).ceil();
    let count = ((f64::from(extent) / (minor * pixels)).ceil() as usize).saturating_add(1);
    let digits = (-major.log10().floor()).max(0.0) as usize;
    (0..count.min(4096))
        .filter_map(|i| {
            let index = first + i as f64;
            let value = index * minor;
            let offset = f64::from(origin) + value * pixels;
            if !(0.0..=f64::from(extent)).contains(&offset) {
                return None;
            }
            Some(Tick {
                offset: offset as f32,
                value,
                label: (index.rem_euclid(5.0) == 0.0).then(|| {
                    format!(
                        "{:.*}",
                        digits.min(6),
                        if value == 0.0 { 0.0 } else { value }
                    )
                }),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_track_the_page_transform_at_every_unit_zoom_and_pan() {
        for unit in [RulerUnit::Millimetres, RulerUnit::Points, RulerUnit::Inches] {
            for scale in [0.005, 0.25, 1.0, 3.75, 32.0] {
                for origin in [-1532.5, 0.0, 157.0, 4096.0] {
                    let ticks = ticks(origin, scale, 1000.0, unit);
                    assert!(!ticks.is_empty());
                    for tick in &ticks {
                        let expected =
                            f64::from(origin) + tick.value * unit.points() * f64::from(scale);
                        assert!((f64::from(tick.offset) - expected).abs() < 0.001);
                        assert!((0.0..=1000.0).contains(&tick.offset));
                    }
                    assert!(ticks.windows(2).all(|pair| pair[0].offset < pair[1].offset));
                    let major: Vec<_> = ticks.iter().filter(|tick| tick.label.is_some()).collect();
                    assert!(major
                        .windows(2)
                        .all(|pair| pair[1].offset - pair[0].offset >= 63.99));
                    assert!(major.windows(2).all(|pair| pair[0].label != pair[1].label));
                }
            }
        }
    }

    #[test]
    fn invalid_viewports_produce_no_ticks() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(ticks(bad, 1.0, 500.0, RulerUnit::Points).is_empty());
            assert!(ticks(0.0, bad, 500.0, RulerUnit::Points).is_empty());
            assert!(ticks(0.0, 1.0, bad, RulerUnit::Points).is_empty());
        }
        for bad in [0.0, -1.0] {
            assert!(ticks(0.0, bad, 500.0, RulerUnit::Points).is_empty());
            assert!(ticks(0.0, 1.0, bad, RulerUnit::Points).is_empty());
        }
    }
}
