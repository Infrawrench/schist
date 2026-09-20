use super::*;

fn fixture(text: &str, script: &str) -> (TextSpec, LoadedFace) {
    let data: &[u8] = match script {
        "hebrew" => include_bytes!("../../../web/fonts/NotoSansHebrew-Regular.ttf"),
        "arabic" => include_bytes!("../../../web/fonts/NotoSansArabic-Regular.ttf"),
        "japanese" => include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf"),
        _ => include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf"),
    };
    let face = LoadedFace {
        font: Arc::new(fontdue::Font::from_bytes(data, Default::default()).unwrap()),
        data: Arc::new(data.to_vec()),
        index: 0,
        cap_ratio: None,
    };
    let spec = TextSpec {
        text: text.into(),
        family: format!("Schist direction fixture {script}"),
        size: 32.0,
        ..Default::default()
    };
    font_cache()
        .lock()
        .unwrap()
        .insert((spec.family.clone(), false, false), Some(face.clone()));
    (spec, face)
}

fn visual_text(spec: &TextSpec, face: &LoadedFace) -> String {
    let mut chars = layout(spec, face).chars;
    chars.sort_by(|a, b| a.x.min(a.end_x).total_cmp(&b.x.min(b.end_x)));
    chars
        .iter()
        .map(|c| spec.text[c.byte..].chars().next().unwrap())
        .collect()
}

#[test]
fn mixed_hebrew_latin_and_digits_follow_unicode_visual_runs() {
    let (mut spec, face) = fixture("אבג abc 123 דה", "hebrew");
    let (latin, _) = fixture("", "latin");
    let from = spec.text.find("abc").unwrap();
    let to = from + "abc 123".len();
    spec.apply_style(
        from..to,
        &StyleRun {
            family: Some(latin.family),
            ..Default::default()
        },
    );
    assert_eq!(visual_text(&spec, &face), "הד abc 123 גבא");
    let laid = layout(&spec, &face);
    assert!(laid.glyphs.iter().all(|g| g.glyph != 0));
    let first = caret_at(&spec, 0).unwrap();
    let second = caret_at(&spec, "א".len()).unwrap();
    assert!(first.x > second.x, "logical Hebrew advances leftwards");
    let latin_first = caret_at(&spec, from).unwrap();
    let latin_next = caret_at(&spec, from + 1).unwrap();
    assert!(latin_first.x < latin_next.x);
    assert!(
        caret_at(&spec, spec.text.find('1').unwrap()).unwrap().x
            < caret_at(&spec, spec.text.find('3').unwrap()).unwrap().x
    );
    spec.direction = ParagraphDirection::LeftToRight;
    assert_eq!(visual_text(&spec, &face), "גבא abc 123 הד");
}

#[test]
fn bidi_affinity_hit_testing_and_disjoint_selection_share_the_shaped_cells() {
    let (mut spec, face) = fixture("ab אבג cd", "hebrew");
    let (latin, _) = fixture("", "latin");
    spec.apply_style(
        0..3,
        &StyleRun {
            family: Some(latin.family.clone()),
            ..Default::default()
        },
    );
    spec.apply_style(
        9..spec.text.len(),
        &StyleRun {
            family: Some(latin.family),
            ..Default::default()
        },
    );
    let boundary = spec.text.find('א').unwrap();
    let downstream = caret_at(&spec, boundary).unwrap();
    let upstream = caret_at_position(
        &spec,
        CaretPosition {
            byte: boundary,
            affinity: CaretAffinity::Upstream,
        },
    )
    .unwrap();
    assert!((upstream.x - downstream.x).abs() > 20.0);
    for (_, caret) in insertion_points(&spec) {
        let position = hit_test_position(&spec, caret.x, caret.top + caret.height / 2.0).unwrap();
        let clicked = caret_at_position(&spec, position).unwrap();
        assert!((clicked.x - caret.x).abs() < 0.01);
    }
    // Select Latin "b " and the first logical Hebrew letter. The two selected
    // areas are separated by the unselected Hebrew letters in visual space.
    let rects = selection_rects(&spec, 1..boundary + 'א'.len_utf8());
    let mut rects = rects;
    rects.sort_by_key(|r| r.left);
    assert!(rects.windows(2).any(|w| w[0].right + 2 < w[1].left));
    let laid = layout(&spec, &face);
    let alef = laid.chars.iter().find(|c| c.byte == boundary).unwrap();
    let selected = rects.last().unwrap();
    assert!(selected.left <= alef.x.min(alef.end_x).floor() as i32);
    assert!(selected.right >= alef.x.max(alef.end_x).ceil() as i32);
}

#[test]
fn arabic_contextual_forms_marks_and_ligatures_keep_logical_grapheme_positions() {
    let (spec, face) = fixture("سَلَام 123", "arabic");
    let laid = layout(&spec, &face);
    assert!(laid.glyphs.iter().all(|g| g.glyph != 0));
    let nominal: Vec<_> = spec
        .text
        .chars()
        .map(|c| face.font.lookup_glyph_index(c))
        .collect();
    assert!(
        laid.glyphs.iter().any(|g| !nominal.contains(&g.glyph)),
        "Arabic contextual forms must be substituted"
    );
    let (word, word_face) = fixture("سَلَام", "arabic");
    let rb_face = rustybuzz::Face::from_slice(&word_face.data, 0).unwrap();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(&word.text);
    buffer.guess_segment_properties();
    buffer.set_direction(rustybuzz::Direction::RightToLeft);
    let expected = rustybuzz::shape(
        &rb_face,
        &[
            rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"liga"), 0, ..),
            rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"clig"), 0, ..),
        ],
        buffer,
    );
    assert_eq!(
        layout(&word, &word_face)
            .glyphs
            .iter()
            .map(|g| g.glyph as u32)
            .collect::<Vec<_>>(),
        expected
            .glyph_infos()
            .iter()
            .map(|g| g.glyph_id)
            .collect::<Vec<_>>(),
        "the Arabic run must match whole-word contextual shaping, including required ligatures"
    );
    assert!(carets(&spec)
        .iter()
        .all(|(byte, _)| *byte == spec.text.len() || !spec.text[*byte..].starts_with('َ')));
    let start = caret_at(&spec, 0).unwrap();
    let next = move_caret(&spec, 0.into(), CaretMovement::Left);
    assert_eq!(next.byte, "سَ".len());
    assert!(caret_at_position(&spec, next).unwrap().x < start.x);
    assert!(!rasterize(&spec).unwrap().is_empty());
}

#[test]
fn unicode_paragraph_separators_crlf_and_wrapping_preserve_source_offsets() {
    let (mut spec, _) = fixture("אב\r\nגד\u{2029}הו\u{2028}זח\n", "hebrew");
    let spans = line_spans(&spec);
    assert_eq!(spans.len(), 5);
    assert_eq!(
        spans
            .iter()
            .map(|s| &spec.text[s.start..s.end])
            .collect::<Vec<_>>(),
        vec!["אב", "גד", "הו", "זח", ""]
    );
    for span in &spans {
        assert!(caret_at(&spec, span.start).unwrap().top >= span.top);
    }
    spec.text = "אבג דהו זחט יכל".into();
    spec.wrap_width = Some(100.0);
    let spans = line_spans(&spec);
    assert!(spans.len() > 1);
    for span in spans {
        assert!(caret_at(&spec, span.start).unwrap().x > 0.0);
    }
    spec.text = "אבגדהוזחטיכלמנסעפצקרשת".into();
    assert_eq!(
        line_spans(&spec).len(),
        1,
        "unbroken words retain the historical overflow policy"
    );
}

#[test]
fn vertical_cjk_columns_orientation_punctuation_and_carets() {
    let (mut spec, face) = fixture("日本（AB）語\n東京", "japanese");
    spec.writing_mode = WritingMode::VerticalRl;
    let laid = layout(&spec, &face);
    assert!(laid.glyphs.iter().all(|g| g.glyph != 0));
    assert!(
        laid.glyphs.iter().any(|g| g.sideways),
        "Latin rotates clockwise"
    );
    assert!(laid.glyphs.iter().any(|g| !g.sideways), "CJK stays upright");
    let cjk = &laid.glyphs[0];
    assert!(!cjk.sideways);
    let bracket = face.font.lookup_glyph_index('（');
    assert!(
        !laid.glyphs.iter().any(|g| g.glyph == bracket),
        "font vertical punctuation alternates must be used"
    );
    let first = caret_at(&spec, 0).unwrap();
    let next = caret_at(&spec, '日'.len_utf8()).unwrap();
    let second = caret_at(&spec, spec.text.find('東').unwrap()).unwrap();
    assert!(next.top > first.top && (next.x - first.x).abs() < 0.01);
    assert!(
        second.x < first.x,
        "vertical-rl advances columns to the left"
    );
    assert!((first.angle - std::f32::consts::FRAC_PI_2).abs() < 0.01);
    for (byte, c) in carets(&spec) {
        let clicked = hit_test(&spec, c.x - c.height / 2.0, c.top).unwrap();
        assert_eq!(clicked, byte);
    }
    let image = rasterize(&spec).unwrap();
    assert!(!image.is_empty());
    assert!(image.bounds.height() > image.bounds.width());
    spec.writing_mode = WritingMode::VerticalLr;
    let first = caret_at(&spec, 0).unwrap();
    let second = caret_at(&spec, spec.text.find('東').unwrap()).unwrap();
    assert!(
        second.x > first.x,
        "vertical-lr advances columns to the right"
    );
    let moved = move_caret(&spec, 0.into(), CaretMovement::Right);
    assert_eq!(moved.byte, spec.text.find('東').unwrap());
    let moved = move_caret(&spec, 0.into(), CaretMovement::Down);
    assert_eq!(moved.byte, '日'.len_utf8());
    spec.wrap_width = Some(70.0);
    assert!(line_spans(&spec).len() > 2, "CJK can wrap without spaces");
}

#[test]
fn legacy_defaults_and_serialization_keep_direction_settings() {
    let (mut spec, _) = fixture("a", "latin");
    let mut json = serde_json::to_value(&spec).unwrap();
    json.as_object_mut().unwrap().remove("direction");
    json.as_object_mut().unwrap().remove("writing_mode");
    let legacy: TextSpec = serde_json::from_value(json).unwrap();
    assert_eq!(legacy.direction, ParagraphDirection::Auto);
    assert_eq!(legacy.writing_mode, WritingMode::Horizontal);
    spec.direction = ParagraphDirection::RightToLeft;
    spec.writing_mode = WritingMode::VerticalRl;
    assert_eq!(
        serde_json::from_str::<TextSpec>(&serde_json::to_string(&spec).unwrap()).unwrap(),
        spec
    );
}

#[test]
fn soft_wrap_boundary_affinity_does_not_skip_a_line_in_either_direction() {
    let (mut spec, _) = fixture("abc def ghi", "latin");
    spec.direction = ParagraphDirection::LeftToRight;
    spec.wrap_width = Some(65.0);
    let spans = line_spans(&spec);
    assert_eq!(spans.len(), 3);
    let end = CaretPosition {
        byte: spans[0].end,
        affinity: CaretAffinity::Upstream,
    };
    assert_eq!(
        move_caret(&spec, end, CaretMovement::Right),
        CaretPosition::from(spans[1].start)
    );
    assert_eq!(
        move_caret(&spec, spans[1].start.into(), CaretMovement::Left),
        end
    );
    assert_eq!(
        move_caret(&spec, end, CaretMovement::Home).byte,
        spans[0].start
    );
    assert_eq!(
        move_caret(&spec, end, CaretMovement::End).byte,
        spans[0].end
    );
}

#[test]
fn arabic_style_boundary_retains_joining_context() {
    let (mut spec, face) = fixture("ببب", "arabic");
    let full = layout(&spec, &face);
    spec.apply_style(
        2..4,
        &StyleRun {
            size: Some(40.0),
            ..Default::default()
        },
    );
    let mixed = layout(&spec, &face);
    assert_eq!(
        full.glyphs.iter().map(|g| g.glyph).collect::<Vec<_>>(),
        mixed.glyphs.iter().map(|g| g.glyph).collect::<Vec<_>>()
    );
    assert!(
        mixed.chars.iter().find(|c| c.byte == 0).unwrap().x
            > mixed.chars.iter().find(|c| c.byte == 4).unwrap().x
    );
}

#[test]
fn dragging_beyond_a_short_line_keeps_the_nearest_line_or_column() {
    let (mut spec, _) = fixture("a\nlong long line", "latin");
    let first = caret_at(&spec, 0).unwrap();
    assert_eq!(
        hit_test(&spec, 1000.0, first.top + first.height / 2.0),
        Some(1)
    );
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        spec.writing_mode = mode;
        let first = caret_at(&spec, 0).unwrap();
        assert_eq!(
            hit_test(&spec, first.x - first.height / 2.0, 1000.0),
            Some(1)
        );
    }
}

#[test]
fn vertical_punctuation_without_an_alternate_uses_unicode_rotation_fallback() {
    let (mut spec, face) = fixture("（", "japanese");
    spec.writing_mode = WritingMode::VerticalRl;
    assert!(!layout(&spec, &face).glyphs[0].sideways);
    spec.set_feature("vert", false);
    spec.set_feature("vrt2", false);
    let laid = layout(&spec, &face);
    assert!(laid.glyphs[0].sideways);
    assert_eq!(laid.glyphs[0].glyph, face.font.lookup_glyph_index('（'));
    assert!(!rasterize(&spec).unwrap().is_empty());
}
