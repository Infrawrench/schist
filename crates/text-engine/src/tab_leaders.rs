//! Generated leader glyphs share the source tab's paint, never its source bytes.
use super::*;

pub(super) struct Pattern {
    text: String,
    style: CharStyle,
    rtl: bool,
    glyphs: Vec<PlacedGlyph>,
    width: f32,
}

/// Cache each distinct pattern/style once per spec, including synthetic caps.
/// Pattern specs have no tab ruler, so resolving their faces cannot recurse.
pub(super) fn resolve(spec: &TextSpec, base: &LoadedFace, faces: &mut Faces) {
    let Some(tabs) = &spec.tabs else { return };
    if tabs.leaders.iter().all(String::is_empty) {
        return;
    }
    for (byte, _) in spec.text.match_indices('\t') {
        let style = spec.style_at(byte);
        let rtl = shaping::paragraph_is_rtl(spec, byte);
        for text in tabs
            .leaders
            .iter()
            .filter(|s| !s.is_empty() && valid_tab_leader(s))
        {
            if faces
                .leaders
                .iter()
                .any(|p| p.text == *text && p.rtl == rtl && p.style.shapes_like(&style))
            {
                continue;
            }
            let mut run = style.as_run();
            run.end = text.len();
            let pattern = TextSpec {
                text: text.clone(),
                family: spec.family.clone(),
                font_style: spec.font_style.clone(),
                bold: spec.bold,
                italic: spec.italic,
                size: spec.size,
                writing_mode: spec.writing_mode,
                direction: if rtl {
                    ParagraphDirection::RightToLeft
                } else {
                    ParagraphDirection::LeftToRight
                },
                runs: vec![run],
                ..Default::default()
            };
            let resolved = Faces::resolve(&pattern, base);
            let (mut glyphs, width) = shaping::leader_pattern(&pattern, &resolved);
            // Spaces and format characters contribute advance but no outline.
            // Avoid enumerating invisible copies, including tiny space periods.
            glyphs.retain(|glyph| {
                let (font, size) = &resolved.faces[glyph.face];
                let bounds = font.font.metrics_indexed(glyph.glyph, *size).bounds;
                bounds.width > 0.0 && bounds.height > 0.0
            });
            let mapping = resolved
                .faces
                .iter()
                .map(|(font, size)| {
                    faces
                        .faces
                        .iter()
                        .position(|(f, s)| Arc::ptr_eq(&font.font, &f.font) && size == s)
                        .unwrap_or_else(|| {
                            let index = faces.faces.len();
                            faces.faces.push((font.clone(), *size));
                            index
                        })
                })
                .collect::<Vec<_>>();
            for glyph in &mut glyphs {
                glyph.face = mapping[glyph.face];
            }
            faces.leaders.push(Pattern {
                text: text.clone(),
                style: style.clone(),
                rtl,
                glyphs,
                width,
            });
        }
    }
}

/// Schist fits complete units against the field's edge, leaving spare advance
/// next to preceding text. This phase policy is not native rendering agreement.
/// Paint work has a finite budget: excessive generated geometry fails the whole
/// raster rather than hanging or silently returning a partially painted page.
pub(super) fn glyphs(spec: &TextSpec, faces: &Faces, laid: &Layout) -> Option<Vec<PlacedGlyph>> {
    const MAX_GLYPHS: usize = 1_000_000;
    let mut result = Vec::new();
    let Some(tabs) = &spec.tabs else {
        return Some(result);
    };
    for &(byte, stop) in &laid.tab_stops {
        let Some(text) = tabs.leaders.get(stop).filter(|s| !s.is_empty()) else {
            continue;
        };
        let style = spec.style_at(byte);
        let rtl = shaping::paragraph_is_rtl(spec, byte);
        let Some(pattern) = faces
            .leaders
            .iter()
            .find(|p| p.text == *text && p.rtl == rtl && p.style.shapes_like(&style))
        else {
            continue;
        };
        if !pattern.width.is_finite() || pattern.width <= 0.0 || pattern.glyphs.is_empty() {
            continue;
        }
        let ch = laid.chars.iter().find(|c| c.byte == byte)?;
        let line = laid
            .lines
            .iter()
            .find(|l| l.start <= byte && byte < l.end)?;
        let (start, end) = (ch.x.min(ch.end_x), ch.x.max(ch.end_x));
        let units = (f64::from(end) - f64::from(start)) / f64::from(pattern.width);
        let nearest = units.round();
        // Endpoints and period arrived as f32. Ignore their arithmetic error
        // at whole units, while preserving real fractional gaps.
        let count =
            if (units - nearest).abs() <= f64::from(4.0 * f32::EPSILON) * units.abs().max(1.0) {
                nearest
            } else {
                units.floor()
            };
        if !count.is_finite()
            || count * pattern.glyphs.len() as f64 > (MAX_GLYPHS - result.len()) as f64
        {
            return None;
        }
        let step = spec.text[line.start..line.end]
            .char_indices()
            .map(|(i, _)| faces.line_metrics_at(spec, line.start + i).1)
            .fold(0.0_f32, f32::max);
        let center = if spec.writing_mode == WritingMode::VerticalRl {
            block_extent(&laid.lines) - line.top - line.height / 2.0
        } else {
            line.top + line.height / 2.0
        };
        for index in 0..count as usize {
            let offset = if ch.end_x >= ch.x {
                f64::from(end) - (count - index as f64) * f64::from(pattern.width)
            } else {
                f64::from(start) + index as f64 * f64::from(pattern.width)
            } as f32;
            for template in &pattern.glyphs {
                let mut glyph = *template;
                glyph.byte = byte;
                if spec.writing_mode.is_vertical() {
                    let cross = if glyph.sideways {
                        -glyph.baseline + (step / 2.0 - (line.baseline - line.top))
                    } else {
                        glyph.baseline
                    };
                    glyph.baseline = glyph.x + offset;
                    glyph.x = center + cross + style.baseline_shift;
                } else {
                    glyph.x += offset;
                    glyph.baseline += line.baseline - style.baseline_shift;
                }
                result.push(glyph);
            }
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family() -> &'static str {
        const NAME: &str = "Schist leader fixture";
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let bytes = include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf");
            font_cache().lock().unwrap().insert(
                (NAME.into(), None, false, false),
                Some(LoadedFace {
                    font: Arc::new(
                        fontdue::Font::from_bytes(bytes.as_slice(), Default::default()).unwrap(),
                    ),
                    data: Arc::new(bytes.to_vec()),
                    index: 0,
                    cap_ratio: None,
                }),
            );
        });
        NAME
    }

    fn spec(mode: WritingMode, scale: f32, alignment: TabAlignment, leader: &str) -> TextSpec {
        TextSpec {
            text: "é\t12.34\tH".into(),
            family: family().into(),
            size: 12.0 * scale,
            direction: ParagraphDirection::LeftToRight,
            writing_mode: mode,
            tabs: Some(TabStops {
                positions: vec![120.0 * scale, 216.0 * scale],
                alignments: vec![alignment; 2],
                leaders: vec![leader.into(), "_ ".into()],
                origin: 9.0 * scale,
                ..Default::default()
            }),
            runs: vec![StyleRun {
                start: 2,
                end: 3,
                size: Some(16.0 * scale),
                baseline_shift: Some(2.0 * scale),
                tracking: Some(0.25 * scale),
                color: Some([12, 128, 250, 128]),
                features: vec![OpenTypeFeature {
                    tag: "liga".into(),
                    value: 1,
                }],
                capitalization: Some(Capitalization::SmallCaps),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn leader_paint_never_changes_source_metrics_wrapping_carets_or_original_glyphs() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for scale in [0.5, 1.0, 1.35, 2.0, 3.0] {
                for alignment in [
                    TabAlignment::Leading,
                    TabAlignment::Trailing,
                    TabAlignment::Center,
                    TabAlignment::Character('.'),
                ] {
                    for leader in [".", ". ", "fi", "é", "—", " "] {
                        let spec = spec(mode, scale, alignment, leader);
                        let mut plain = spec.clone();
                        plain.tabs.as_mut().unwrap().leaders.clear();
                        let face = load_font(&spec.family, None, false, false).unwrap();
                        let laid = layout(&spec, &face);
                        assert_eq!(laid.glyphs, layout(&plain, &face).glyphs);
                        let faces = Faces::resolve(&spec, &face);
                        let generated = glyphs(&spec, &faces, &laid).unwrap();
                        assert!(generated
                            .iter()
                            .all(|g| spec.text.as_bytes()[g.byte] == b'\t'));
                        assert_eq!(line_spans(&spec), line_spans(&plain));
                        assert_eq!(insertion_points(&spec), insertion_points(&plain));
                        let actual = measure(&spec).unwrap();
                        let expected = measure(&plain).unwrap();
                        assert_eq!(
                            (
                                actual.width,
                                actual.height,
                                actual.first_baseline,
                                actual.line_advance
                            ),
                            (
                                expected.width,
                                expected.height,
                                expected.first_baseline,
                                expected.line_advance
                            )
                        );
                        assert_eq!(spec.text, plain.text);
                        for width in [60.0, 120.0, 240.0] {
                            let measures = [InlineMeasure {
                                width: width * scale,
                                start: 12.0 * scale,
                            }];
                            assert_eq!(
                                line_spans_with_measures(&spec, &measures),
                                line_spans_with_measures(&plain, &measures)
                            );
                        }
                        assert!(rasterize_with_paints(&spec).is_some());
                    }
                }
            }
        }
    }

    #[test]
    fn leaders_shape_the_tab_style_and_fit_complete_units_against_the_field_edge() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for leader in [". ", "fi", "abc", "é", "é— "] {
                let spec = spec(mode, 1.0, TabAlignment::Center, leader);
                let face = load_font(&spec.family, None, false, false).unwrap();
                let faces = Faces::resolve(&spec, &face);
                let laid = layout(&spec, &face);
                let actual = glyphs(&spec, &faces, &laid)
                    .unwrap()
                    .into_iter()
                    .filter(|g| g.byte == 2)
                    .collect::<Vec<_>>();
                let mut run = spec.style_at(2).as_run();
                run.end = leader.len();
                let ordinary = TextSpec {
                    text: leader.into(),
                    runs: vec![run],
                    tabs: None,
                    ..spec.clone()
                };
                let ordinary_faces = Faces::resolve(&ordinary, &face);
                let mut reference = layout(&ordinary, &face);
                reference.glyphs.retain(|g| {
                    let (font, size) = &ordinary_faces.faces[g.face];
                    let b = font.font.metrics_indexed(g.glyph, *size).bounds;
                    b.width > 0.0 && b.height > 0.0
                });
                let period = measure(&ordinary).unwrap().width;
                let ch = laid.chars.iter().find(|c| c.byte == 2).unwrap();
                let mut positions = Vec::new();
                let mut end = f64::from(ch.end_x);
                while end - f64::from(period) >= f64::from(ch.x) {
                    end -= f64::from(period);
                    positions.push(end as f32);
                }
                positions.reverse();
                assert_eq!(actual.len(), positions.len() * reference.glyphs.len());
                for (copy, offset) in actual.chunks(reference.glyphs.len()).zip(positions) {
                    for (a, b) in copy.iter().zip(&reference.glyphs) {
                        assert_eq!(a.glyph, b.glyph);
                        assert_eq!(faces.faces[a.face].1, ordinary_faces.faces[b.face].1);
                        assert!(Arc::ptr_eq(
                            &faces.faces[a.face].0.font,
                            &ordinary_faces.faces[b.face].0.font
                        ));
                        let inline = if mode.is_vertical() {
                            a.baseline - b.baseline
                        } else {
                            a.x - b.x
                        };
                        assert!(
                            (inline - offset).abs() < 0.001,
                            "{mode:?}/{leader}/{inline}/{offset}"
                        );
                        let cross = if mode.is_vertical() {
                            a.x - b.x
                        } else {
                            a.baseline - b.baseline
                        };
                        assert!(cross.abs() < 0.001, "{mode:?}/{leader}/{cross}");
                    }
                }
            }
        }
    }

    #[test]
    fn only_the_selected_stop_lends_a_leader_and_implicit_stops_have_none() {
        let mut spec = spec(WritingMode::Horizontal, 1.0, TabAlignment::Trailing, ".");
        spec.text = "A\tWWWW".into();
        spec.runs.clear();
        spec.tabs = Some(TabStops {
            positions: vec![4.0, 120.0],
            alignments: vec![TabAlignment::Trailing; 2],
            leaders: vec![".".into(), "_".into()],
            ..Default::default()
        });
        let face = load_font(&spec.family, None, false, false).unwrap();
        let laid = layout(&spec, &face);
        assert_eq!(laid.tab_stops, vec![(1, 1)]);
        let faces = Faces::resolve(&spec, &face);
        let actual = glyphs(&spec, &faces, &laid).unwrap();
        assert!(!actual.is_empty());
        assert!(actual
            .iter()
            .all(|g| g.glyph == face.font.lookup_glyph_index('_')));
        spec.tabs.as_mut().unwrap().positions = vec![4.0, 6.0];
        let laid = layout(&spec, &face);
        assert!(laid.tab_stops.is_empty());
        assert!(glyphs(&spec, &Faces::resolve(&spec, &face), &laid)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn leader_validation_legacy_data_and_unbounded_generated_work_are_explicit() {
        for value in ["", " ", ".  ", "éééééééé", "abcdefgh"] {
            assert!(valid_tab_leader(value));
        }
        for value in ["abcdefghi", "\t", "\n", "\u{2028}", "\u{2029}"] {
            assert!(!valid_tab_leader(value));
        }
        let old: TabStops =
            serde_json::from_str(r#"{"positions":[120],"repeat":36,"origin":0}"#).unwrap();
        assert!(old.leaders.is_empty());
        assert!(!serde_json::to_string(&old).unwrap().contains("leaders"));
        let mut spec = spec(WritingMode::Horizontal, 1.0, TabAlignment::Leading, ".");
        spec.tabs.as_mut().unwrap().positions = vec![1e30, 2e30];
        assert!(measure(&spec).unwrap().width.is_finite());
        assert!(rasterize_with_paints(&spec).is_none());
        spec.tabs.as_mut().unwrap().positions = vec![30_000_000.0, 60_000_000.0];
        spec.tabs.as_mut().unwrap().leaders.clear();
        assert!(rasterize_with_paints(&spec).is_none());
        spec.tabs.as_mut().unwrap().leaders = vec![" ".into(), " ".into()];
        assert!(rasterize_with_paints(&spec).is_none());
        spec.text = "\t".into();
        spec.runs.clear();
        spec.size = 1e-6;
        spec.tabs.as_mut().unwrap().positions = vec![120.0, 216.0];
        spec.tabs.as_mut().unwrap().leaders = vec![" ".into(), " ".into()];
        assert!(rasterize_with_paints(&spec).is_some());
    }

    #[test]
    fn leader_cache_and_phase_follow_each_source_paragraph_direction() {
        let mut spec = spec(WritingMode::Horizontal, 1.0, TabAlignment::Leading, ".");
        spec.direction = ParagraphDirection::Auto;
        spec.text = "A\tH\nא\tH".into();
        spec.runs.clear();
        let face = load_font(&spec.family, None, false, false).unwrap();
        let faces = Faces::resolve(&spec, &face);
        assert!(faces.leaders.iter().any(|p| !p.rtl && p.text == "."));
        assert!(faces.leaders.iter().any(|p| p.rtl && p.text == "."));
        let laid = layout(&spec, &face);
        let generated = glyphs(&spec, &faces, &laid).unwrap();
        for (byte, _) in spec.text.match_indices('\t') {
            let ch = laid.chars.iter().find(|c| c.byte == byte).unwrap();
            let copies = generated
                .iter()
                .filter(|g| g.byte == byte)
                .collect::<Vec<_>>();
            assert!(!copies.is_empty());
            let pattern = faces
                .leaders
                .iter()
                .find(|p| p.text == "." && p.rtl == (ch.end_x < ch.x))
                .unwrap();
            if ch.end_x < ch.x {
                assert!((copies[0].x - ch.end_x - pattern.glyphs[0].x).abs() < 0.001);
            } else {
                assert!(
                    (copies.last().unwrap().x + pattern.width - ch.end_x - pattern.glyphs[0].x)
                        .abs()
                        < 0.001
                );
            }
        }
    }

    #[test]
    fn whole_unit_counts_survive_f32_scaling_without_rounding_genuine_partial_units_up() {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for size in [9.0, 12.0, 16.0, 18.0, 20.5] {
                let ordinary = TextSpec {
                    text: ".".into(),
                    family: family().into(),
                    size,
                    writing_mode: mode,
                    direction: ParagraphDirection::LeftToRight,
                    ..Default::default()
                };
                let period = measure(&ordinary).unwrap().width;
                let mut spec = TextSpec {
                    text: "\t".into(),
                    tabs: Some(TabStops {
                        positions: vec![120.0],
                        leaders: vec![".".into()],
                        ..Default::default()
                    }),
                    ..ordinary
                };
                let face = load_font(&spec.family, None, false, false).unwrap();
                let faces = Faces::resolve(&spec, &face);
                for copies in 1..=128 {
                    for fraction in [0.0, 0.25, 0.75] {
                        spec.tabs.as_mut().unwrap().positions[0] =
                            period * (copies as f32 + fraction);
                        let laid = layout(&spec, &face);
                        let painted = glyphs(&spec, &faces, &laid).unwrap();
                        assert_eq!(painted.len(), copies, "{mode:?}/{size}/{copies}/{fraction}");
                    }
                }
            }
        }
    }
}
