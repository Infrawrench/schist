//! Display casing keeps source bytes intact, including expanding uppercase maps.
use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Capitalization {
    #[default]
    Normal,
    AllCaps,
    SmallCaps,
    /// Native OpenType all-small-caps; no synthetic fallback.
    OpenTypeAllSmallCaps,
}

impl Capitalization {
    pub fn from_flags(all: bool, small: bool) -> Self {
        match (all, small) {
            (false, false) => Self::Normal,
            (true, false) => Self::AllCaps,
            (false, true) => Self::SmallCaps,
            (true, true) => Self::OpenTypeAllSmallCaps,
        }
    }

    pub fn flags(self) -> (bool, bool) {
        match self {
            Self::Normal => (false, false),
            Self::AllCaps => (true, false),
            Self::SmallCaps => (false, true),
            Self::OpenTypeAllSmallCaps => (true, true),
        }
    }
}

pub(super) fn features(style: &CharStyle) -> Vec<OpenTypeFeature> {
    let defaults = match style.capitalization {
        Capitalization::SmallCaps => vec![
            OpenTypeFeature {
                tag: "smcp".into(),
                value: 1,
            },
            OpenTypeFeature {
                tag: "c2sc".into(),
                value: 0,
            },
        ],
        Capitalization::OpenTypeAllSmallCaps => vec![
            OpenTypeFeature {
                tag: "smcp".into(),
                value: 1,
            },
            OpenTypeFeature {
                tag: "c2sc".into(),
                value: 1,
            },
        ],
        _ => Vec::new(),
    };
    merged_features(&defaults, &style.features)
}

/// Probe actual substitutions for this grapheme, rather than assuming a font's
/// smcp table covers every alphabet. Marks travel with their lowercase base.
pub(super) fn has_small_caps(face: &LoadedFace, text: &str, language: &str) -> bool {
    let Some(face) = rustybuzz::Face::from_slice(&face.data, face.index) else {
        return false;
    };
    let shape = |enabled| {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        if let Ok(language) = language.parse() {
            buffer.set_language(language);
        }
        let feature = rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"smcp"), enabled, ..);
        rustybuzz::shape(&face, &[feature], buffer)
            .glyph_infos()
            .iter()
            .map(|g| g.glyph_id)
            .collect::<Vec<_>>()
    };
    shape(0) != shape(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str, native: bool, mode: WritingMode) -> TextSpec {
        // Private cache names keep concurrent geometry tests independent of
        // system discovery and of the process-wide preferred font family.
        let data: &[u8] = if native {
            include_bytes!("../../../web/fonts/NotoSans-Regular.ttf")
        } else {
            include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf")
        };
        let family = if native {
            "Schist caps native fixture"
        } else {
            "Schist caps synthetic fixture"
        };
        let face = LoadedFace {
            font: Arc::new(fontdue::Font::from_bytes(data, Default::default()).unwrap()),
            data: Arc::new(data.to_vec()),
            index: 0,
            cap_ratio: None,
        };
        font_cache()
            .lock()
            .unwrap()
            .insert((family.into(), None, false, false), Some(face));
        TextSpec {
            text: text.into(),
            family: family.into(),
            size: 32.0,
            direction: ParagraphDirection::LeftToRight,
            writing_mode: mode,
            leading: Some(48.0),
            ..Default::default()
        }
    }
    const MODES: [WritingMode; 3] = [
        WritingMode::Horizontal,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ];
    fn same_pixels(a: &TextSpec, b: &TextSpec) {
        let a = rasterize(a).unwrap();
        let b = rasterize(b).unwrap();
        assert_eq!(a.bounds, b.bounds);
        assert_eq!(a.coverage, b.coverage);
        assert_eq!(a.first_baseline, b.first_baseline);
        assert_eq!(a.line_advance, b.line_advance);
    }

    #[test]
    fn turkic_display_casing_preserves_source_boundaries_and_matches_independent_capitals() {
        for mode in MODES {
            for language in ["tr", "TR_tr", "az-Latn-AZ"] {
                let text = "diyarbakır i I ı İ";
                let uppercase = "DİYARBAKIR İ I I İ";
                let mut actual = spec(text, false, mode);
                actual.language = language.into();
                actual.runs.push(StyleRun {
                    start: 0,
                    end: text.len(),
                    capitalization: Some(Capitalization::AllCaps),
                    ..Default::default()
                });
                let expected = spec(uppercase, false, mode);
                same_pixels(&actual, &expected);
                assert_eq!(actual.text, text);
                let source_boundaries: Vec<_> = text
                    .grapheme_indices(true)
                    .map(|(i, _)| i)
                    .chain([text.len()])
                    .collect();
                let upper_boundaries: Vec<_> = uppercase
                    .grapheme_indices(true)
                    .map(|(i, _)| i)
                    .chain([uppercase.len()])
                    .collect();
                for (a, b) in source_boundaries.iter().zip(upper_boundaries) {
                    assert_eq!(caret_at(&actual, *a), caret_at(&expected, b));
                }
            }
        }
    }

    #[test]
    fn language_shaping_uses_real_localized_glyphs_and_range_resets_do_not_rewrite_the_source() {
        for mode in MODES {
            let mut actual = spec("ŞŢşţ", false, mode);
            actual.language = "ro-RO".into();
            same_pixels(&actual, &spec("ȘȚșț", false, mode));
            let mut actual = spec("şşş", false, mode);
            actual.language = "ro".into();
            actual.apply_style(
                2..4,
                &StyleRun {
                    language: Some(String::new()),
                    ..Default::default()
                },
            );
            let mut expected = spec("șşș", false, mode);
            expected.language = "ro".into();
            expected.apply_style(
                2..4,
                &StyleRun {
                    language: Some(String::new()),
                    ..Default::default()
                },
            );
            same_pixels(&actual, &expected);
            assert_eq!(actual.text, "şşş");
            assert_eq!(actual.style_at(0).language, "ro");
            assert_eq!(actual.style_at(2).language, "");
            assert_eq!(actual.style_at(4).language, "ro");
            let reset = TextSpec::default().base_style().as_run();
            actual.apply_style(0..actual.text.len(), &reset);
            assert!(actual.style_at(0).language.is_empty());
        }
    }

    #[test]
    fn languages_shape_auto_direction_and_lithuanian_casing_keeps_grapheme_carets() {
        for mode in MODES {
            let mut actual = spec("ŞŢşţ", false, mode);
            actual.direction = ParagraphDirection::Auto;
            actual.language = "ro".into();
            same_pixels(&actual, &spec("ȘȚșț", false, mode));
            let text = "i\u{307}\u{301} j\u{328}\u{307} į\u{307}";
            let expected_text = "I\u{301} J\u{328} Į";
            let mut actual = spec(text, false, mode);
            actual.language = "lt-LT".into();
            actual.apply_style(
                0..text.len(),
                &StyleRun {
                    capitalization: Some(Capitalization::AllCaps),
                    ..Default::default()
                },
            );
            let expected = spec(expected_text, false, mode);
            same_pixels(&actual, &expected);
            for ((a, _), (b, _)) in text
                .grapheme_indices(true)
                .zip(expected_text.grapheme_indices(true))
            {
                assert_eq!(caret_at(&actual, a), caret_at(&expected, b));
            }
            assert_eq!(
                caret_at(&actual, text.len()),
                caret_at(&expected, expected_text.len())
            );
            assert_eq!(actual.text, text);
            let mut neutral = spec("AVAV", false, mode);
            neutral.apply_style(
                1..3,
                &StyleRun {
                    language: Some("und".into()),
                    ..Default::default()
                },
            );
            same_pixels(&neutral, &spec("AVAV", false, mode));
        }
    }

    #[test]
    fn display_uppercase_expansions_keep_source_clusters_and_match_uppercase_glyphs() {
        for mode in MODES {
            for text in ["Straße ﬃ café", "a\u{301} ſ ǆ", "abc def\nghi ß", "אב ß אב"] {
                let mut actual = spec(text, false, mode);
                actual.runs.push(StyleRun {
                    start: 0,
                    end: text.len(),
                    capitalization: Some(Capitalization::AllCaps),
                    ..Default::default()
                });
                actual.wrap_width = Some(140.0);
                let mut expected = spec(&text.to_uppercase(), false, mode);
                expected.wrap_width = actual.wrap_width;
                same_pixels(&actual, &expected);
                assert_eq!(actual.text, text);
                let boundaries: Vec<_> = text
                    .grapheme_indices(true)
                    .map(|(at, _)| at)
                    .chain([text.len()])
                    .collect();
                for (at, caret) in carets(&actual) {
                    assert!(
                        boundaries.contains(&at),
                        "caret {at} inside a source grapheme"
                    );
                    assert!(caret.x.is_finite() && caret.top.is_finite());
                    let upper_offset = text[..at].to_uppercase().len();
                    assert_eq!(caret_at(&actual, at), caret_at(&expected, upper_offset));
                }
                for span in line_spans(&actual) {
                    assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
                }
            }
        }
    }

    #[test]
    fn real_small_caps_match_explicit_features_and_synthetic_caps_keep_nominal_cells() {
        for mode in MODES {
            for caps in [
                Capitalization::SmallCaps,
                Capitalization::OpenTypeAllSmallCaps,
            ] {
                let mut actual = spec("Abc Def", true, mode);
                actual.runs.push(StyleRun {
                    start: 0,
                    end: actual.text.len(),
                    capitalization: Some(caps),
                    ..Default::default()
                });
                let mut expected = spec("Abc Def", true, mode);
                expected.set_feature("smcp", true);
                expected.set_feature("c2sc", caps == Capitalization::OpenTypeAllSmallCaps);
                same_pixels(&actual, &expected);
                assert_ne!(
                    rasterize(&actual).unwrap().coverage,
                    rasterize(&spec("Abc Def", true, mode)).unwrap().coverage
                );
            }
            for scale in [0.4, 0.7, 1.2] {
                let text = "Abß e\u{301}";
                let mut actual = spec(text, false, mode);
                actual.runs.push(StyleRun {
                    start: 0,
                    end: text.len(),
                    capitalization: Some(Capitalization::SmallCaps),
                    small_cap_scale: Some(scale),
                    ..Default::default()
                });
                let mut expected = spec(&text.to_uppercase(), false, mode);
                let mut offset = 0;
                for grapheme in text.graphemes(true) {
                    let upper = grapheme.to_uppercase();
                    if grapheme.chars().any(char::is_lowercase) {
                        expected.runs.push(StyleRun {
                            start: offset,
                            end: offset + upper.len(),
                            size: Some(32.0 * scale),
                            metric_size: Some(32.0),
                            ..Default::default()
                        });
                    }
                    offset += upper.len();
                }
                same_pixels(&actual, &expected);
                let spans = line_spans(&actual);
                let plain = line_spans(&spec(text, false, mode));
                assert_eq!(spans[0].height, plain[0].height);
                assert_eq!(spans[0].baseline, plain[0].baseline);
            }
        }
    }

    #[test]
    fn synthetic_caps_keep_automatic_decoration_thickness_and_cross_axis_position() {
        for mode in MODES {
            for scale in [0.4, 0.7, 1.2] {
                let mut actual = spec("Aa Aa", false, mode);
                actual.size = 80.0;
                actual.runs.push(StyleRun {
                    start: 0,
                    end: actual.text.len(),
                    capitalization: Some(Capitalization::SmallCaps),
                    small_cap_scale: Some(scale),
                    underline: Some(true),
                    strikethrough: Some(true),
                    ..Default::default()
                });
                let base = load_font(&actual.family, None, false, false).unwrap();
                let faces = Faces::resolve(&actual, &base);
                let laid = layout(&actual, &base);
                let lines = decoration_rasters(&actual, &faces, &laid);
                for kind in [PaintKind::Underline, PaintKind::Strike] {
                    let dimensions: Vec<_> = lines
                        .iter()
                        .filter(|r| r.kind == kind)
                        .map(|r| {
                            if mode.is_vertical() {
                                (r.rect.left, r.rect.right)
                            } else {
                                (r.rect.top, r.rect.bottom)
                            }
                        })
                        .collect();
                    assert_eq!(dimensions.len(), 5);
                    assert!(
                        dimensions.iter().all(|d| *d == dimensions[0]),
                        "{mode:?} {scale} {dimensions:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn capitalization_edits_preserve_offsets_normal_resets_and_legacy_defaults() {
        let mut actual = spec("aß b", false, WritingMode::Horizontal);
        let len = actual.text.len();
        actual.apply_style(
            0..len,
            &StyleRun {
                capitalization: Some(Capitalization::SmallCaps),
                small_cap_scale: Some(0.6),
                ..Default::default()
            },
        );
        assert_eq!(actual.style_at(0).capitalization, Capitalization::SmallCaps);
        let normal = actual.base_style().as_run();
        actual.apply_style(1..3, &normal);
        assert_eq!(actual.style_at(1).capitalization, Capitalization::Normal);
        assert_eq!(actual.style_at(3).capitalization, Capitalization::SmallCaps);
        actual.normalize_runs();
        assert_eq!(actual.text, "aß b");
        let legacy: StyleRun = serde_json::from_str(r#"{"start":0,"end":2}"#).unwrap();
        assert!(legacy.is_plain());
    }
}
