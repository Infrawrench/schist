//! Independently inherited underline/strikethrough paints and stroke patterns.
use crate::{styles::ResolvedCharacter, Ink};
use serde::{Deserialize, Serialize};

/// Explicit Auto resets an inherited point value; None on the style inherits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DecorationMeasure {
    Auto,
    Points(f32),
}
impl DecorationMeasure {
    pub fn points(self) -> Option<f32> {
        match self {
            Self::Points(value) if value.is_finite() => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DecorationPaint {
    /// Use the current character's paint, even when an ancestor named an ink.
    Text,
    None,
    Ink(Ink),
}
impl DecorationPaint {
    pub fn ink(&self) -> Option<&Ink> {
        if let Self::Ink(ink) = self {
            Some(ink)
        } else {
            None
        }
    }
}

/// A named native stroke resource, carried with its definition like an Ink.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecorationStroke {
    pub name: String,
    #[serde(default)]
    pub fitting: schist_text_engine::DecorationFit,
    pub pattern: schist_text_engine::TextDecorationPattern,
}
impl DecorationStroke {
    pub fn valid(&self) -> bool {
        use schist_text_engine::{DecorationFit, TextDecorationPattern};
        self.pattern.valid()
            && match self.pattern {
                TextDecorationPattern::Solid | TextDecorationPattern::Stripes(_) => {
                    self.fitting == DecorationFit::None
                }
                TextDecorationPattern::Dots(_) => self.fitting != DecorationFit::Dashes,
                TextDecorationPattern::Dashes(_) => true,
            }
    }
    pub fn solid() -> Self {
        Self {
            name: "Solid".into(),
            fitting: Default::default(),
            pattern: schist_text_engine::TextDecorationPattern::Solid,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecorationStyle {
    pub stroke: Option<DecorationStroke>,
    pub gap_paint: Option<DecorationPaint>,
    pub gap_tint: Option<f32>,
    pub gap_overprint: Option<bool>,
    pub paint: Option<DecorationPaint>,
    pub weight: Option<DecorationMeasure>,
    /// Points from the baseline: positive below for underline, above for strike.
    /// In vertical text, points from the column center toward its outside edge.
    pub offset: Option<DecorationMeasure>,
    pub tint: Option<f32>,
    pub overprint: Option<bool>,
}
impl DecorationStyle {
    pub fn over(&self, base: &Self) -> Self {
        let paint = self.paint.clone().or_else(|| {
            base.paint.as_ref().map(|paint| {
                if let DecorationPaint::Ink(ink) = paint {
                    if self.tint.is_some() {
                        return DecorationPaint::Ink(ink.base_color().into_owned());
                    }
                }
                paint.clone()
            })
        });
        let gap_paint = self.gap_paint.clone().or_else(|| {
            base.gap_paint.as_ref().map(|paint| {
                if let DecorationPaint::Ink(ink) = paint {
                    if self.gap_tint.is_some() {
                        return DecorationPaint::Ink(ink.base_color().into_owned());
                    }
                }
                paint.clone()
            })
        });
        Self {
            stroke: self.stroke.clone().or_else(|| base.stroke.clone()),
            gap_paint,
            gap_tint: self.gap_tint.or(base.gap_tint),
            gap_overprint: self.gap_overprint.or(base.gap_overprint),
            paint,
            weight: self.weight.or(base.weight),
            offset: self.offset.or(base.offset),
            tint: self.tint.or(base.tint),
            overprint: self.overprint.or(base.overprint),
        }
    }

    /// Effective ink/tint/overprint without converting through preview RGB.
    pub fn paint(&self, text: &ResolvedCharacter) -> Option<(Ink, f32, bool)> {
        let (ink, tint) = match &self.paint {
            Some(DecorationPaint::None) => return None,
            Some(DecorationPaint::Ink(ink)) => (ink.clone(), self.tint.unwrap_or(1.0)),
            None | Some(DecorationPaint::Text) => {
                if text.fill_disabled {
                    return None;
                }
                let ink = crate::styles::inherited_paint(&None, self.tint, text.fill.as_ref())
                    .unwrap_or_else(Ink::black);
                (ink, self.tint.or(text.fill_tint).unwrap_or(1.0))
            }
        };
        Some((
            ink,
            tint,
            self.overprint.or(text.overprint_fill).unwrap_or(false),
        ))
    }

    /// Missing gap paint means transparent; explicit Text follows the glyph ink.
    pub fn gap_paint(&self, text: &ResolvedCharacter) -> Option<(Ink, f32, bool)> {
        let paint = self.gap_paint.clone()?;
        Self {
            paint: Some(paint),
            tint: self.gap_tint,
            overprint: self.gap_overprint,
            ..Default::default()
        }
        .paint(text)
    }

    pub fn preview(&self, text: &ResolvedCharacter) -> schist_text_engine::TextDecoration {
        let paint = self.paint(text);
        schist_text_engine::TextDecoration {
            fitting: self.stroke.as_ref().map(|s| s.fitting).unwrap_or_default(),
            pattern: self
                .stroke
                .as_ref()
                .map(|s| s.pattern.clone())
                .unwrap_or_default(),
            gap_color: self.gap_paint(text).map(|(ink, tint, _)| {
                let rgb = ink.preview_at_tint(tint);
                [
                    (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (text.opacity.unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8,
                ]
            }),
            weight: self.weight.and_then(DecorationMeasure::points),
            offset: self.offset.and_then(DecorationMeasure::points),
            disabled: paint.is_none(),
            color: paint.map(|(ink, tint, _)| {
                let rgb = ink.preview_at_tint(tint);
                [
                    (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (text.opacity.unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8,
                ]
            }),
        }
    }
}
