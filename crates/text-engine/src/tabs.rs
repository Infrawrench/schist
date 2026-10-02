//! Inline tab geometry shared by measurement, painting and caret placement.
use serde::{Deserialize, Serialize};

/// Anchor in the shaped field following a tab, measured along the inline axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabAlignment {
    #[default]
    Leading,
    Trailing,
    Center,
    Character(char),
}

/// Tab stops measured from a column origin, in text-layout units.
/// Explicit stops replace implicit stops before the last explicit position.
/// An ahead-of-pen hanging indent supplies a virtual leading stop before a
/// later explicit stop or the implicit grid.
/// `repeat` is Schist's implicit interval, not a claim about an imported ruler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabStops {
    pub positions: Vec<f32>,
    /// Parallel to `positions`. Omitted entries are leading stops, preserving
    /// the geometry and serialized form of older text specifications.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alignments: Vec<TabAlignment>,
    /// Literal leader patterns parallel to `positions`. Missing entries have
    /// no leader. Generated paint inherits the source tab character's style.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub leaders: Vec<String>,
    pub repeat: f32,
    /// Start of this spec's inline measure relative to the column origin.
    pub origin: f32,
    /// Inline measure for an already composed line. A terminal tab after text
    /// can end at this edge when its stop lies beyond it. This preserves the
    /// source tab while the following field resumes on the next line. Wrapping
    /// supplies its own per-line measure; `None` keeps unbounded tab geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_width: Option<f32>,
    /// Logical leading paragraph indent, relative to the column ruler. It can
    /// be reached from an outdented first line and has no leader of its own.
    /// Native horizontal LTR evidence is in the public `list-markers` PDF;
    /// applying the same logical rule to other axes is Schist policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hanging_indent: Option<f32>,
}

impl Default for TabStops {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            alignments: Vec::new(),
            leaders: Vec::new(),
            repeat: 36.0,
            origin: 0.0,
            line_width: None,
            hanging_indent: None,
        }
    }
}

impl TabStops {
    pub fn valid(&self) -> bool {
        self.origin.is_finite()
            && self.repeat.is_finite()
            && self.repeat > 0.0
            && self.positions.iter().all(|v| v.is_finite())
            && self.alignments.len() <= self.positions.len()
            && self.leaders.len() <= self.positions.len()
            && self
                .line_width
                .is_none_or(|width| width.is_finite() && width > 0.0)
            && self.hanging_indent.is_none_or(f32::is_finite)
    }

    /// Position after a tab with an empty field, relative to the inline start. The extra
    /// start offset lets wrapping use first-line, list and initial indents
    /// without moving the paragraph's tab ruler.
    pub fn next(&self, pen: f32, start: f32) -> Option<f32> {
        self.next_aligned(pen, start, |_| 0.0)
    }

    /// Place the next field without overlapping preceding text. `anchor`
    /// supplies its shaped anchor. Select the first stop ahead of the pen,
    /// clamping a colliding field to the pen rather than skipping that stop.
    /// A tab may therefore have zero advance. Passed stops are skipped; the
    /// implicit leading grid resumes after the final explicit stop. The public
    /// InDesign `tab-breaks` PDF establishes this rule for horizontal LTR text;
    /// the same logical geometry is used for the other writing modes.
    pub(crate) fn next_aligned(
        &self,
        pen: f32,
        start: f32,
        anchor: impl Fn(TabAlignment) -> f32,
    ) -> Option<f32> {
        self.next_stop(pen, start, anchor)
            .map(|(position, _)| position)
    }

    /// The selected explicit record travels with the geometry, including a
    /// zero-advance tab. A passed stop cannot lend its leader to another stop.
    pub(crate) fn next_stop(
        &self,
        pen: f32,
        start: f32,
        anchor: impl Fn(TabAlignment) -> f32,
    ) -> Option<(f32, Option<usize>)> {
        if !self.valid() || !pen.is_finite() || !start.is_finite() {
            return None;
        }
        let origin = f64::from(self.origin) + f64::from(start);
        let absolute = origin + f64::from(pen);
        let explicit = self
            .positions
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, position)| {
                let alignment = self.alignments.get(index).copied().unwrap_or_default();
                let offset = anchor(alignment);
                let aligned = f64::from(position) - f64::from(offset);
                (offset.is_finite() && f64::from(position) > absolute).then_some((
                    position,
                    aligned.max(absolute),
                    index,
                ))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(position, aligned, index)| (f64::from(position), aligned, index));
        let indent = self
            .hanging_indent
            .map(f64::from)
            .filter(|indent| *indent > absolute && explicit.is_none_or(|stop| *indent < stop.0));
        let target = if let Some(indent) = indent {
            (indent, None)
        } else {
            explicit
                .map(|(_, aligned, index)| (aligned, Some(index)))
                .unwrap_or_else(|| {
                    let step = f64::from(self.repeat);
                    let after = self
                        .positions
                        .iter()
                        .fold(absolute, |a, p| a.max(f64::from(*p)));
                    ((after / step).floor().mul_add(step, step), None)
                })
        };
        let next = (target.0 - origin) as f32;
        (next.is_finite() && (next > pen || next == pen && target.1.is_some()))
            .then_some((next, target.1))
    }

    pub fn scaled(&mut self, scale: f32) {
        self.origin *= scale;
        self.repeat *= scale;
        if let Some(width) = &mut self.line_width {
            *width *= scale;
        }
        if let Some(indent) = &mut self.hanging_indent {
            *indent *= scale;
        }
        for position in &mut self.positions {
            *position *= scale;
        }
    }
}

/// Public tab-leader authoring accepts at most eight Unicode scalars. Preserve
/// literal spaces; controls and paragraph/line separators are not inline paint.
pub fn valid_tab_leader(value: &str) -> bool {
    value.chars().count() <= 8
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
}

/// Width and start position for one line, both in inline text-layout units.
/// The final measure repeats for subsequent lines in the paragraph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InlineMeasure {
    pub width: f32,
    pub start: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_family() -> &'static str {
        use std::sync::{Arc, Once};
        const NAME: &str = "Schist tab fixture";
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            // Register a private cache entry without refreshing global discovery;
            // refreshing would erase other concurrently running fixture entries.
            let bytes = include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf");
            let face = crate::LoadedFace {
                font: Arc::new(
                    fontdue::Font::from_bytes(bytes.as_slice(), Default::default()).unwrap(),
                ),
                data: Arc::new(bytes.to_vec()),
                index: 0,
                cap_ratio: None,
            };
            crate::font_cache()
                .lock()
                .unwrap()
                .insert((NAME.into(), None, false, false), Some(face));
        });
        NAME
    }

    #[test]
    fn virtual_indent_stops_keep_the_ruler_and_cannot_borrow_an_explicit_leader() {
        for indent in [-24.0, 0.0, 24.0, 40.0, 72.0, 180.0] {
            for positions in [
                vec![],
                vec![18.0, 60.0, 120.0],
                vec![120.0, 60.0, 18.0],
                vec![indent],
            ] {
                for pen in -48..=200 {
                    // Choose by ruler position; a virtual stop is leading,
                    // while an explicit trailing field may collide with pen.
                    let explicit = positions
                        .iter()
                        .enumerate()
                        .filter(|(_, stop)| **stop > pen as f32)
                        .min_by(|a, b| a.1.total_cmp(b.1));
                    let virtual_stop =
                        indent > pen as f32 && explicit.is_none_or(|(_, stop)| indent < *stop);
                    let expected = if virtual_stop {
                        (indent, None)
                    } else if let Some((index, stop)) = explicit {
                        ((*stop - 20.0).max(pen as f32), Some(index))
                    } else {
                        let after = positions
                            .iter()
                            .fold(pen as f32, |after, stop| after.max(*stop));
                        ((after / 36.0).floor() * 36.0 + 36.0, None)
                    };
                    for origin in [-48.0, 0.0, 17.0, 72.0] {
                        for start in [-12.0, 0.0, 36.0] {
                            let tabs = TabStops {
                                positions: positions.clone(),
                                alignments: vec![TabAlignment::Trailing; positions.len()],
                                leaders: vec![".".into(); positions.len()],
                                origin,
                                hanging_indent: Some(indent),
                                ..Default::default()
                            };
                            let local_pen = pen as f32 - origin - start;
                            let actual = tabs.next_stop(local_pen, start, |_| 20.0).unwrap();
                            assert_eq!((actual.0 + origin + start, actual.1), expected);
                            for scale in [0.5, 2.0, 3.0] {
                                let mut scaled = tabs.clone();
                                scaled.scaled(scale);
                                let actual = scaled
                                    .next_stop(local_pen * scale, start * scale, |_| 20.0 * scale)
                                    .unwrap();
                                assert_eq!((actual.0 / scale + origin + start, actual.1), expected);
                            }
                        }
                    }
                }
            }
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!TabStops {
                hanging_indent: Some(value),
                ..Default::default()
            }
            .valid());
        }
        let old = r#"{"positions":[60,120],"repeat":36,"origin":0}"#;
        let legacy: TabStops = serde_json::from_str(old).unwrap();
        assert_eq!(legacy.hanging_indent, None);
        assert_eq!(
            serde_json::to_string(&legacy).unwrap(),
            r#"{"positions":[60.0,120.0],"repeat":36.0,"origin":0.0}"#
        );
    }

    #[test]
    fn stops_are_column_relative_order_independent_and_scale_with_text() {
        for positions in [vec![], vec![24.0, 60.0, 90.0], vec![90.0, 24.0, 60.0, 24.0]] {
            for absolute in -48..=120 {
                let tabs = TabStops {
                    positions: positions.clone(),
                    ..Default::default()
                };
                let expected = tabs.next(absolute as f32, 0.0).unwrap();
                assert!(expected > absolute as f32);
                if !positions.is_empty() && absolute < 24 {
                    assert_eq!(expected, 24.0);
                }
                for origin in [-48.0, 0.0, 17.0, 72.0] {
                    for start in [-12.0, 0.0, 36.0] {
                        let local = TabStops {
                            origin,
                            ..tabs.clone()
                        };
                        let pen = absolute as f32 - origin - start;
                        assert_eq!(local.next(pen, start).unwrap() + origin + start, expected);
                        for scale in [0.5, 2.0, 3.0] {
                            let mut scaled = local.clone();
                            scaled.scaled(scale);
                            assert_eq!(
                                scaled.next(pen * scale, start * scale),
                                local.next(pen, start).map(|v| v * scale)
                            );
                        }
                    }
                }
            }
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(TabStops::default().next(value, 0.0), None);
            assert_eq!(TabStops::default().next(0.0, value), None);
            assert!(!TabStops {
                positions: vec![value],
                ..Default::default()
            }
            .valid());
            assert!(!TabStops {
                repeat: value,
                ..Default::default()
            }
            .valid());
        }
        for repeat in [0.0, -1.0] {
            assert!(!TabStops {
                repeat,
                ..Default::default()
            }
            .valid());
        }
        // Implicit intervals that cannot advance at f32 precision must fail;
        // only a selected explicit stop may intentionally have zero advance.
        assert_eq!(TabStops::default().next(1e20, 0.0), None);
        for width in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(!TabStops {
                line_width: Some(width),
                ..Default::default()
            }
            .valid());
        }
    }

    #[test]
    fn tab_advance_moves_glyphs_and_carets_together_without_adding_source_positions() {
        use crate::{caret_at, insertion_points, measure, ParagraphDirection, TextSpec};
        for prefix in ["", "é", "A "] {
            for origin in [-12.0, 0.0, 9.0] {
                let text = format!("{prefix}\tH");
                let spec = TextSpec {
                    text: text.clone(),
                    family: fixture_family().into(),
                    size: 12.0,
                    direction: ParagraphDirection::LeftToRight,
                    tabs: Some(TabStops {
                        positions: vec![48.0],
                        origin,
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let after = prefix.len() + 1;
                assert!((caret_at(&spec, after).unwrap().x - (48.0 - origin)).abs() < 0.001);
                let suffix = TextSpec {
                    text: "H".into(),
                    tabs: None,
                    ..spec.clone()
                };
                let width = measure(&suffix).unwrap().width;
                assert!((measure(&spec).unwrap().width - (48.0 - origin + width)).abs() < 0.001);
                let face = crate::load_font(&spec.family, None, false, false).unwrap();
                let actual = crate::layout(&spec, &face);
                let reference = crate::layout(&suffix, &face);
                let glyph = actual.glyphs.iter().find(|g| g.byte == after).unwrap();
                assert_eq!(glyph.glyph, reference.glyphs[0].glyph);
                assert!((glyph.x - reference.glyphs[0].x - (48.0 - origin)).abs() < 0.001);
                assert!(actual.glyphs.iter().all(|g| g.byte != prefix.len()));
                for (position, _) in insertion_points(&spec) {
                    assert!(text.is_char_boundary(position.byte));
                    assert!(position.byte <= text.len());
                }
                assert_eq!(spec.text, text);
            }
        }
    }

    #[test]
    fn integer_tab_offsets_preserve_exact_glyph_stroke_coverage_in_every_writing_mode() {
        use crate::{rasterize_with_paints, StyleRun, TextSpec, TextStroke, WritingMode};
        for writing_mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for scale in [1.0, 2.0, 3.0] {
                for distance in [24.0, 48.0, 96.0, 4096.0] {
                    let mut spec = TextSpec {
                        text: "é".into(),
                        family: fixture_family().into(),
                        size: 12.0 * scale,
                        writing_mode,
                        runs: vec![StyleRun {
                            start: 0,
                            end: 3,
                            color: Some([0, 128, 255, 180]),
                            stroke: Some(TextStroke {
                                width: 0.35 * scale,
                                color: Some([255, 0, 128, 180]),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }],
                        ..Default::default()
                    };
                    let expected = rasterize_with_paints(&spec).unwrap();
                    spec.text.insert(0, '\t');
                    spec.tabs = Some(TabStops {
                        positions: vec![distance * scale],
                        ..Default::default()
                    });
                    let actual = rasterize_with_paints(&spec).unwrap();
                    let offset = (distance * scale) as i32;
                    assert_eq!(
                        actual.bounds,
                        if writing_mode == WritingMode::Horizontal {
                            expected.bounds.translated(offset, 0)
                        } else {
                            expected.bounds.translated(0, offset)
                        }
                    );
                    assert_eq!(actual.paints.len(), expected.paints.len());
                    for (actual, expected) in actual.paints.iter().zip(&expected.paints) {
                        assert_eq!(
                            actual.coverage, expected.coverage,
                            "{writing_mode:?}/{scale}/{distance}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn wrapping_and_individual_line_measurement_share_the_same_tab_origin() {
        use crate::{line_spans_with_measures, measure, ParagraphDirection, TextSpec};
        for width in [65.0, 85.0, 130.0] {
            for start in [-12.0, 0.0, 18.0] {
                let spec = TextSpec {
                    text: "é\tH words é\tH words é\tH words".into(),
                    family: fixture_family().into(),
                    size: 12.0,
                    direction: ParagraphDirection::LeftToRight,
                    tabs: Some(TabStops {
                        positions: vec![24.0, 60.0, 96.0],
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let measures = [
                    InlineMeasure { width, start },
                    InlineMeasure {
                        width: width + 20.0,
                        start: 0.0,
                    },
                ];
                let lines = line_spans_with_measures(&spec, &measures);
                assert!(!lines.is_empty());
                assert_eq!(lines.first().unwrap().start, 0);
                assert_eq!(lines.last().unwrap().end, spec.text.len());
                for pair in lines.windows(2) {
                    assert_eq!(pair[0].end, pair[1].start);
                }
                for (index, line) in lines.iter().enumerate() {
                    let mut part = spec.clone();
                    part.text = spec.text[line.start..line.end].into();
                    part.tabs.as_mut().unwrap().origin += measures[index.min(1)].start;
                    part.tabs.as_mut().unwrap().line_width = Some(measures[index.min(1)].width);
                    assert!(
                        (measure(&part).unwrap().width - line.width).abs() < 0.001,
                        "{width}/{start}/{index}"
                    );
                }
            }
        }
    }

    #[test]
    fn aligned_fields_share_shaped_glyph_and_caret_anchors_in_every_writing_mode() {
        use crate::{
            caret_at, insertion_points, measure, ParagraphDirection, StyleRun, TextSpec,
            WritingMode,
        };
        for writing_mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for text in [
                "12.34",
                ".12.34",
                "é,15",
                "fi:12",
                "e\u{301}35",
                "no delimiter",
            ] {
                for alignment in [
                    TabAlignment::Trailing,
                    TabAlignment::Center,
                    TabAlignment::Character('.'),
                    TabAlignment::Character(','),
                    TabAlignment::Character(':'),
                    TabAlignment::Character('i'),
                    TabAlignment::Character('\u{301}'),
                ] {
                    for scale in [0.5, 1.0, 2.0] {
                        let reference = TextSpec {
                            text: text.into(),
                            family: fixture_family().into(),
                            size: 12.0 * scale,
                            direction: ParagraphDirection::LeftToRight,
                            features: vec![crate::OpenTypeFeature {
                                tag: "liga".into(),
                                value: 1,
                            }],
                            writing_mode,
                            runs: vec![StyleRun {
                                start: text.char_indices().nth(2).map_or(text.len(), |(i, _)| i),
                                end: text.len(),
                                size: Some(15.0 * scale),
                                ..Default::default()
                            }],
                            ..Default::default()
                        };
                        let inline = |caret: crate::Caret| {
                            if writing_mode.is_vertical() {
                                caret.top
                            } else {
                                caret.x
                            }
                        };
                        let width = measure(&reference).unwrap().width;
                        let anchor = match alignment {
                            TabAlignment::Trailing => width,
                            TabAlignment::Center => width / 2.0,
                            TabAlignment::Character(c) => text.find(c).map_or(width, |at| {
                                let boundary = crate::grapheme_boundaries(text)
                                    .take_while(|b| *b <= at)
                                    .last()
                                    .unwrap();
                                inline(caret_at(&reference, boundary).unwrap())
                            }),
                            _ => unreachable!(),
                        };
                        for origin in [-12.0, 0.0, 9.0] {
                            let prefix = "é\t";
                            let mut spec = reference.clone();
                            spec.text = format!("{prefix}{text}");
                            for run in &mut spec.runs {
                                run.start += prefix.len();
                                run.end += prefix.len();
                            }
                            spec.tabs = Some(TabStops {
                                positions: vec![180.0 * scale],
                                alignments: vec![alignment],
                                origin: origin * scale,
                                ..Default::default()
                            });
                            let offset = (180.0 - origin) * scale - anchor;
                            assert!(
                                (inline(caret_at(&spec, prefix.len()).unwrap()) - offset).abs()
                                    < 0.001,
                                "{writing_mode:?}/{alignment:?}/{text}"
                            );
                            assert!(
                                (measure(&spec).unwrap().width - (offset + width)).abs() < 0.001
                            );
                            let face = crate::load_font(&spec.family, None, false, false).unwrap();
                            let actual = crate::layout(&spec, &face);
                            let expected = crate::layout(&reference, &face);
                            let glyphs = actual
                                .glyphs
                                .iter()
                                .filter(|g| g.byte >= prefix.len())
                                .collect::<Vec<_>>();
                            assert_eq!(glyphs.len(), expected.glyphs.len());
                            for (a, b) in glyphs.iter().zip(&expected.glyphs) {
                                assert_eq!(a.glyph, b.glyph);
                                let (x, y) = if writing_mode.is_vertical() {
                                    (0.0, offset)
                                } else {
                                    (offset, 0.0)
                                };
                                assert!((a.x - b.x - x).abs() < 0.001);
                                assert!((a.baseline - b.baseline - y).abs() < 0.001);
                            }
                            for (position, _) in insertion_points(&spec) {
                                assert!(crate::grapheme_boundaries(&spec.text)
                                    .any(|b| b == position.byte));
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn aligned_fields_clamp_to_the_pen_and_only_passed_stops_are_skipped() {
        for reverse in [false, true] {
            let mut tabs = TabStops {
                positions: vec![60.0, 120.0],
                alignments: vec![TabAlignment::Trailing, TabAlignment::Center],
                ..Default::default()
            };
            if reverse {
                tabs.positions.reverse();
                tabs.alignments.reverse();
            }
            for width in [0.0_f32, 20.0, 80.0, 240.0] {
                for pen in 0..180 {
                    let anchor = |a| match a {
                        TabAlignment::Trailing => width,
                        TabAlignment::Center => width / 2.0,
                        _ => 0.0,
                    };
                    let expected = if 60.0 > pen as f32 {
                        (60.0 - width).max(pen as f32)
                    } else if 120.0 > pen as f32 {
                        (120.0 - width / 2.0).max(pen as f32)
                    } else {
                        ((pen as f32).max(120.0) / 36.0).floor() * 36.0 + 36.0
                    };
                    for origin in [-12.0, 0.0, 17.0] {
                        tabs.origin = origin;
                        let value = tabs.next_aligned(pen as f32 - origin, 0.0, anchor).unwrap();
                        assert_eq!(value + origin, expected);
                        assert!(value >= pen as f32 - origin);
                    }
                }
            }
        }
        let legacy: TabStops =
            serde_json::from_str(r#"{"positions":[60,120],"repeat":36,"origin":0}"#).unwrap();
        assert_eq!(legacy.next(20.0, 0.0), Some(60.0));
        let aligned = TabStops {
            alignments: vec![TabAlignment::Character('€')],
            ..legacy
        };
        assert_eq!(
            serde_json::from_str::<TabStops>(&serde_json::to_string(&aligned).unwrap()).unwrap(),
            aligned
        );
    }

    #[test]
    fn public_native_collision_sweeps_select_the_same_stops() {
        // Observed word starts in the public InDesign 20.0.1.32 PDF, page 2,
        // paged-media/core commit 2e3c998e, corpus/generated/tab-breaks.pdf.
        // Its fixture definitions supply the ruler positions, not an external
        // layout algorithm. These are bounding-box observations, so allow
        // 0.04pt for glyph bearings and PDF advance rounding. Full font/raster
        // agreement is outside this geometry test; see docs/idml-format.md.
        let offsets = [
            -3.0, -2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0,
        ];
        for (alignment, center, anchor, first) in [
            (TabAlignment::Leading, 38.0, 0.0, 37.979),
            (TabAlignment::Trailing, 48.0, 10.181, 37.819),
            (TabAlignment::Center, 43.0, 5.101, 37.899),
            (TabAlignment::Character('.'), 42.0, 4.081, 37.919),
        ] {
            for (index, offset) in offsets.into_iter().chain([30.0 - center]).enumerate() {
                let tabs = TabStops {
                    positions: vec![center + offset],
                    alignments: vec![alignment],
                    ..Default::default()
                };
                let observed = if index == 12 || alignment == TabAlignment::Leading && index < 4 {
                    71.979
                } else if index < 4 {
                    37.709
                } else {
                    first + offset
                };
                let actual = tabs.next_aligned(37.709, 0.0, |_| anchor).unwrap();
                assert!(
                    (actual - observed).abs() < 0.04,
                    "{alignment:?}/{index}: {actual} != {observed}"
                );
            }
        }
    }

    #[test]
    fn zero_advance_tabs_keep_source_carets_and_place_fields_without_overlap() {
        use crate::{
            caret_at, insertion_points, measure, ParagraphDirection, TextSpec, WritingMode,
        };
        for writing_mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for direction in [
                ParagraphDirection::LeftToRight,
                ParagraphDirection::RightToLeft,
            ] {
                for scale in [0.5, 1.0, 2.0, 3.0] {
                    let mut spec = TextSpec {
                        text: "WWWW".into(),
                        family: fixture_family().into(),
                        size: 12.0 * scale,
                        writing_mode,
                        direction,
                        ..Default::default()
                    };
                    let pen = measure(&spec).unwrap().width;
                    let prefix = spec.text.len();
                    spec.text.push_str("\t12.34");
                    for alignment in [
                        TabAlignment::Trailing,
                        TabAlignment::Center,
                        TabAlignment::Character('.'),
                    ] {
                        spec.tabs = Some(TabStops {
                            positions: vec![pen + 0.5 * scale],
                            alignments: vec![alignment],
                            leaders: vec![".".into()],
                            ..Default::default()
                        });
                        let width = measure(&spec).unwrap().width;
                        let inline = |caret: crate::Caret| {
                            if writing_mode.is_vertical() {
                                caret.top
                            } else if direction == ParagraphDirection::RightToLeft {
                                width - caret.x
                            } else {
                                caret.x
                            }
                        };
                        assert!((inline(caret_at(&spec, prefix).unwrap()) - pen).abs() < 0.001);
                        let field = TextSpec {
                            text: "12.34".into(),
                            tabs: None,
                            ..spec.clone()
                        };
                        let ordinary = caret_at(&field, 0).unwrap();
                        let actual = caret_at(&spec, prefix + 1).unwrap();
                        // A bidi field's first source character need not be at
                        // its logical leading edge. Compare physical carets.
                        let offset = if !writing_mode.is_vertical()
                            && direction == ParagraphDirection::RightToLeft
                        {
                            0.0
                        } else {
                            pen
                        };
                        let difference = if writing_mode.is_vertical() {
                            actual.top - ordinary.top
                        } else {
                            actual.x - ordinary.x
                        };
                        assert!((difference - offset).abs() < 0.001);
                        assert!((width - pen - measure(&field).unwrap().width).abs() < 0.001);
                        let face = crate::load_font(&spec.family, None, false, false).unwrap();
                        let laid = crate::layout(&spec, &face);
                        assert_eq!(laid.tab_stops, vec![(prefix, 0)]);
                        assert!(crate::tab_leaders::glyphs(
                            &spec,
                            &crate::Faces::resolve(&spec, &face),
                            &laid
                        )
                        .unwrap()
                        .is_empty());
                        for at in [prefix, prefix + 1] {
                            assert!(insertion_points(&spec)
                                .iter()
                                .any(|(position, _)| position.byte == at));
                        }
                        assert_eq!(spec.text, "WWWW\t12.34");
                    }
                }
            }
        }
    }

    #[test]
    fn glyph_pixel_placement_ignores_round_off_but_keeps_fractional_offsets() {
        for integer in (-4096..=4096).step_by(17) {
            let value = integer as f32;
            for close in [
                value.next_down().next_down(),
                value,
                value.next_up().next_up(),
            ] {
                assert_eq!(crate::glyph_pixel_start(close), integer);
            }
            for fraction in [-0.25, -0.01, 0.01, 0.25, 0.99] {
                let value = value + fraction;
                assert_eq!(crate::glyph_pixel_start(value), value.floor() as i32);
            }
        }
    }

    #[test]
    fn empty_and_adjacent_aligned_fields_preserve_every_tab_and_terminal_caret() {
        use crate::{
            caret_at, insertion_points, measure, ParagraphDirection, TextSpec, WritingMode,
        };
        for writing_mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for alignment in [
                TabAlignment::Trailing,
                TabAlignment::Center,
                TabAlignment::Character('.'),
            ] {
                for text in ["\t", "\t\t", "é\t\t", "é\tH\t", "é\t\tH"] {
                    let spec = TextSpec {
                        text: text.into(),
                        family: fixture_family().into(),
                        size: 12.0,
                        direction: ParagraphDirection::LeftToRight,
                        writing_mode,
                        tabs: Some(TabStops {
                            positions: vec![72.0, 144.0, 216.0],
                            alignments: vec![alignment; 3],
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    let inline = |c: crate::Caret| {
                        if writing_mode.is_vertical() {
                            c.top
                        } else {
                            c.x
                        }
                    };
                    for (at, _) in text.match_indices('\t') {
                        assert!(
                            inline(caret_at(&spec, at + 1).unwrap())
                                > inline(caret_at(&spec, at).unwrap())
                        );
                    }
                    let points = insertion_points(&spec);
                    for byte in crate::grapheme_boundaries(text) {
                        assert!(points.iter().any(|(p, _)| p.byte == byte));
                    }
                    assert!(
                        (inline(caret_at(&spec, text.len()).unwrap())
                            - measure(&spec).unwrap().width)
                            .abs()
                            < 0.001
                    );
                }
            }
        }
    }

    #[test]
    fn rtl_tab_fields_share_the_ruler_without_reversing_vertical_progression() {
        use crate::{ParagraphDirection, TextSpec, WritingMode};
        for writing_mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for size in [9.0, 12.0, 24.0] {
                for origin in [-12.0, 0.0, 17.0] {
                    for alignment in [
                        TabAlignment::Leading,
                        TabAlignment::Trailing,
                        TabAlignment::Center,
                        TabAlignment::Character('.'),
                    ] {
                        let text = "אב\t12.34\tfi.é";
                        let spec = TextSpec {
                            text: text.into(),
                            family: fixture_family().into(),
                            size,
                            direction: ParagraphDirection::RightToLeft,
                            writing_mode,
                            features: vec![crate::OpenTypeFeature {
                                tag: "liga".into(),
                                value: 1,
                            }],
                            tabs: Some(TabStops {
                                positions: vec![120.0, 240.0],
                                alignments: vec![alignment; 2],
                                ..Default::default()
                            }),
                            ..Default::default()
                        };
                        let mut spec = spec;
                        spec.tabs.as_mut().unwrap().origin = origin;
                        let face = crate::load_font(&spec.family, None, false, false).unwrap();
                        let actual = crate::layout(&spec, &face);
                        let reverse = !writing_mode.is_vertical();
                        for ((from, field), stop) in
                            [("אב\t".len(), "12.34"), ("אב\t12.34\t".len(), "fi.é")]
                                .into_iter()
                                .zip([120.0, 240.0])
                        {
                            let ordinary = TextSpec {
                                text: field.into(),
                                tabs: None,
                                ..spec.clone()
                            };
                            let expected = crate::layout(&ordinary, &face);
                            // Native field naming is deliberately absent here: the
                            // engine's contract uses logical leading/trailing edges.
                            let anchor = match alignment {
                                TabAlignment::Leading => {
                                    if reverse {
                                        expected.layout_width
                                    } else {
                                        0.0
                                    }
                                }
                                TabAlignment::Trailing => {
                                    if reverse {
                                        0.0
                                    } else {
                                        expected.layout_width
                                    }
                                }
                                TabAlignment::Center => expected.layout_width / 2.0,
                                TabAlignment::Character(c) => {
                                    expected
                                        .chars
                                        .iter()
                                        .find(|p| p.byte == field.find(c).unwrap())
                                        .unwrap()
                                        .x
                                }
                            };
                            let target = if reverse {
                                actual.layout_width - stop + origin
                            } else {
                                stop - origin
                            };
                            let offset = target - anchor;
                            for ch in &expected.chars {
                                let found = actual
                                    .chars
                                    .iter()
                                    .find(|p| p.byte == from + ch.byte)
                                    .unwrap();
                                assert!((found.x - ch.x - offset).abs() < 0.002, "{writing_mode:?}/{size}/{origin}/{alignment:?}/{field}: {found:?}, {ch:?}, offset {offset}");
                                assert!((found.end_x - ch.end_x - offset).abs() < 0.002);
                            }
                            for glyph in &expected.glyphs {
                                let found = actual
                                    .glyphs
                                    .iter()
                                    .find(|g| g.byte == from + glyph.byte && g.glyph == glyph.glyph)
                                    .unwrap();
                                let distance = if writing_mode.is_vertical() {
                                    found.baseline - glyph.baseline
                                } else {
                                    found.x - glyph.x
                                };
                                assert!((distance - offset).abs() < 0.002);
                            }
                        }
                        for byte in crate::grapheme_boundaries(text) {
                            assert!(crate::insertion_points(&spec)
                                .iter()
                                .any(|(p, _)| p.byte == byte));
                        }
                        assert_eq!(spec.text, text);
                    }
                }
            }
        }
    }

    #[test]
    fn mirroring_the_final_rtl_field_preserves_its_exact_glyph_masks() {
        use crate::{ParagraphDirection, StyleRun, TextSpec, TextStroke};
        for size in [9.0, 12.0, 16.0, 18.0, 20.5, 24.0, 32.0] {
            for scale in [0.5, 1.0, 1.35, 2.0, 3.0] {
                for alignment in [
                    TabAlignment::Leading,
                    TabAlignment::Trailing,
                    TabAlignment::Center,
                    TabAlignment::Character(','),
                ] {
                    let spec = TextSpec {
                        text: "\t56,78".into(),
                        family: fixture_family().into(),
                        size: size * scale,
                        direction: ParagraphDirection::RightToLeft,
                        tabs: Some(TabStops {
                            positions: vec![120.0 * scale],
                            alignments: vec![alignment],
                            ..Default::default()
                        }),
                        runs: vec![StyleRun {
                            start: 0,
                            end: 6,
                            color: Some([0, 128, 255, 180]),
                            stroke: Some(TextStroke {
                                width: 0.35 * scale,
                                color: Some([255, 0, 128, 180]),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }],
                        ..Default::default()
                    };
                    let mut ordinary = spec.clone();
                    ordinary.text.remove(0);
                    ordinary.tabs = None;
                    ordinary.runs[0].end -= 1;
                    let actual = crate::rasterize_with_paints(&spec).unwrap();
                    let expected = crate::rasterize_with_paints(&ordinary).unwrap();
                    assert_eq!(
                        actual.bounds, expected.bounds,
                        "{size}/{scale}/{alignment:?}"
                    );
                    assert_eq!(
                        actual.coverage, expected.coverage,
                        "{size}/{scale}/{alignment:?}"
                    );
                    assert_eq!(
                        actual
                            .paints
                            .iter()
                            .map(|p| (p.color, &p.coverage))
                            .collect::<Vec<_>>(),
                        expected
                            .paints
                            .iter()
                            .map(|p| (p.color, &p.coverage))
                            .collect::<Vec<_>>(),
                        "{size}/{scale}/{alignment:?}"
                    );
                }
            }
        }
    }
}
