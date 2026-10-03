//! Text hit testing uses the same shaped line specifications as painting.
use super::{tools, DesignState};
use schist_layout::{Display, ObjectId, Point, Rect};

pub fn line_origin(spec: &schist_text_engine::TextSpec, rect: Rect) -> Point {
    let width = schist_text_engine::measure(spec).map_or(0.0, |m| m.width);
    schist_layout::compose::aligned_origin(rect, width, spec.align, spec.writing_mode)
}

pub fn hit(state: &DesignState, at: Point, object: Option<ObjectId>) -> Option<(ObjectId, usize)> {
    let board = state.plan()?;
    let at = state.to_pasteboard(at);
    let id = object.or_else(|| super::select::hit_test(&board, at).object())?;
    schist_layout::threading::story_of(&state.document, id)?;
    let (_, _, _, byte) = board
        .pages
        .iter()
        .flat_map(|p| &p.objects)
        .filter_map(|d| {
            let Display::Text {
                generated: false,
                object,
                rect,
                spec,
                start,
                positions,
                transform,
                ..
            } = d
            else {
                return None;
            };
            if *object != id {
                return None;
            }
            let local = schist_layout::affine::point(transform.invert()?, at);
            let origin = line_origin(spec, *rect);
            // The visible caret segments include each run's baseline offset.
            // Unshifted line boxes can pick an adjacent line after a large shift.
            let points = schist_text_engine::insertion_points(spec);
            let (x, y) = (local.x - origin.x, local.y - origin.y);
            let (left, top, right, bottom) = points
                .iter()
                .flat_map(|(_, caret)| {
                    [
                        (caret.x, caret.top),
                        (
                            caret.x - caret.angle.sin() * caret.height,
                            caret.top + caret.angle.cos() * caret.height,
                        ),
                    ]
                })
                .fold(
                    (
                        f32::INFINITY,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        f32::NEG_INFINITY,
                    ),
                    |(l, t, r, b), (x, y)| (l.min(x), t.min(y), r.max(x), b.max(y)),
                );
            // Prefer the text extent containing the click. A drop cap spans
            // several body lines; its nearest edge need not be the nearest caret.
            let outside =
                x < left - 0.01 || x > right + 0.01 || y < top - 0.01 || y > bottom + 0.01;
            points
                .into_iter()
                .map(|(position, caret)| {
                    let (cross, inline) = caret.hit_distance(x, y);
                    (
                        outside,
                        cross,
                        inline,
                        positions
                            .as_ref()
                            .map_or(start + position.byte, |p| p.source(position.byte)),
                    )
                })
                .min_by(compare_text_hits)
        })
        .min_by(compare_text_hits)?;
    Some((id, byte))
}

fn compare_text_hits(
    a: &(bool, f32, f32, usize),
    b: &(bool, f32, f32, usize),
) -> std::cmp::Ordering {
    a.0.cmp(&b.0)
        .then(a.1.total_cmp(&b.1))
        .then(a.2.total_cmp(&b.2))
}

pub fn press(state: &mut DesignState, at: Point) -> bool {
    let Some((object, offset)) = hit(state, at, None) else {
        return false;
    };
    if !tools::begin_typing(state, object, offset) {
        return false;
    }
    state.text_selecting = true;
    true
}

pub fn drag(state: &mut DesignState, at: Point) {
    let Some(typing) = state.typing else {
        return;
    };
    let target = hit(state, at, None)
        .filter(|(id, _)| {
            schist_layout::threading::story_of(&state.document, *id) == Some(typing.story)
        })
        .or_else(|| hit(state, at, Some(typing.object)));
    if let Some((object, at)) = target {
        tools::select_to(state, at, true);
        if let Some(typing) = state.typing.as_mut() {
            typing.object = object;
        }
    }
}

/// Claim a thread-target click or start one from an output port.
pub fn thread_press(state: &mut DesignState, at: Point) -> Option<bool> {
    let board = state.plan()?;
    let at = state.to_pasteboard(at);
    if let Some(source) = state.thread_source {
        let Some(target) = super::select::hit_test(&board, at).object() else {
            return Some(false);
        };
        let changed =
            schist_layout::threading::link(&mut state.document, &mut state.history, source, target);
        if changed {
            state.thread_source = None;
            state.selection = vec![target];
        }
        return Some(changed);
    }
    for display in board.pages.iter().flat_map(|p| &p.objects) {
        if let Display::Ports { object, outlet, .. } = display {
            if outlet.contains(at) && !state.document.object_locked(*object) {
                state.thread_source = Some(*object);
                state.typing = None;
                state.selection = vec![*object];
                return Some(false);
            }
        }
    }
    None
}

/// Choose one caret line at shared frame/line boundaries. Prefer the clicked
/// frame at its empty tail, otherwise the downstream line owns a boundary.
pub fn caret_line(
    board: &schist_layout::Pasteboard,
    typing: super::Typing,
) -> Option<(ObjectId, usize)> {
    board
        .pages
        .iter()
        .flat_map(|p| &p.objects)
        .filter_map(|display| {
            let Display::Text {
                generated: false,
                object,
                story,
                start,
                end,
                ..
            } = display
            else {
                return None;
            };
            (*story == typing.story && *start <= typing.at && typing.at <= *end).then_some((
                (*object, *start),
                (*object == typing.object, typing.at < *end, *start),
            ))
        })
        .max_by_key(|(_, priority)| *priority)
        .map(|(line, _)| line)
}

/// Move between composed lines, including columns and other threaded frames.
pub fn adjacent_line_caret(state: &DesignState, forward: bool) -> Option<usize> {
    let typing = state.typing?;
    let thread = schist_layout::compose::compose_story(&state.document, typing.story);
    let lines: Vec<_> = thread
        .frames
        .iter()
        .flat_map(|f| &f.lines)
        .filter(|l| !l.is_generated())
        .collect();
    let index = lines.iter().rposition(|l| l.start <= typing.at)?;
    let next = if forward {
        index.checked_add(1)?
    } else {
        index.checked_sub(1)?
    };
    let Some(target) = lines.get(next) else {
        return Some(state.text_buffer.len());
    };
    let story = state.document.story(typing.story)?;
    let spec = |line: &schist_layout::compose::ComposedLine| {
        schist_layout::compose::line_spec(line, story, &state.document)
    };
    let current = lines[index];
    let current_spec = spec(current);
    let target_spec = spec(target);
    let current_origin = line_origin(&current_spec, current.bounds);
    let target_origin = line_origin(&target_spec, target.bounds);
    let caret = schist_text_engine::caret_at(&current_spec, current.visual_byte(typing.at))?;
    // Keep the inline position relative to its column when crossing an indent,
    // alignment change, column boundary or another frame in the thread.
    let inline = if current_spec.writing_mode == schist_text_engine::WritingMode::Horizontal {
        current_origin.x + caret.x
    } else {
        current_origin.y + caret.top
    } - current.inline_origin
        + target.inline_origin;
    let (x, y) = if target_spec.writing_mode == schist_text_engine::WritingMode::Horizontal {
        (inline - target_origin.x, 0.0)
    } else {
        (0.0, inline - target_origin.y)
    };
    Some(target.source_byte(schist_text_engine::hit_test(&target_spec, x, y).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::{authoring, threading};

    #[test]
    fn footnote_reference_hits_and_navigation_stay_in_original_source_coordinates() {
        use schist_layout::{footnotes::*, Story, StoryStructure};
        let content = "Aé words and more words that wrap.";
        for anchor in [0, 1, 3, content.len()] {
            for scale in [0.7, 1.8] {
                for affine in [
                    schist_core::Affine::IDENTITY,
                    schist_core::Affine::rotate(0.12),
                ] {
                    let mut state = DesignState::new();
                    let frame = authoring::text_frame(
                        &mut state.document,
                        &mut state.history,
                        0,
                        Rect::new(30.0, 30.0, 100.0, 200.0),
                    )
                    .unwrap();
                    state.document.footnotes.no_splitting = Some(true);
                    state.document.footnotes.start_at = Some(12);
                    let mut story = Story::from_text(content, "Body");
                    story.structures.push(StoryStructure {
                        control: None,
                        at: Some(anchor),
                        kind: "Footnote".into(),
                        payload: "retained".into(),
                        footnote: Some(FootnoteBody {
                            story: Story::from_text(" Note body", "Body"),
                            markers: vec![FootnoteMarker {
                                at: 0,
                                character_style: String::new(),
                            }],
                            reference_paragraph_style: "Body".into(),
                            reference_character_style: String::new(),
                        }),
                    });
                    state.document.stories[frame.story.0 as usize] = story;
                    state.document.objects[0].transform = affine;
                    state.view.scale = scale;
                    let board = state.plan().unwrap();
                    let mut notes = 0;
                    for display in board.objects() {
                        let Display::Text {
                            generated,
                            positions,
                            spec,
                            rect,
                            transform,
                            ..
                        } = display
                        else {
                            continue;
                        };
                        if *generated {
                            notes += 1;
                            continue;
                        }
                        let positions = positions.as_ref().unwrap();
                        let origin = line_origin(spec, *rect);
                        for (position, caret) in schist_text_engine::insertion_points(spec) {
                            let at = schist_layout::affine::point(
                                *transform,
                                Point::new(
                                    origin.x + caret.x,
                                    origin.y + caret.top + caret.height / 2.0,
                                ),
                            );
                            assert_eq!(
                                hit(&state, state.to_page(at), Some(frame.object)),
                                Some((frame.object, positions.source(position.byte)))
                            );
                        }
                    }
                    assert!(notes > 0);
                    assert!(tools::begin_typing(&mut state, frame.object, 0));
                    for _ in 0..10 {
                        let next = adjacent_line_caret(&state, true).unwrap();
                        assert!(content.is_char_boundary(next));
                        tools::select_to(&mut state, next, false);
                        if next == content.len() {
                            break;
                        }
                    }
                    assert_eq!(state.text_buffer, content);
                    assert_eq!(state.typing.unwrap().at, content.len());
                }
            }
        }
    }

    #[test]
    fn generated_list_ink_never_becomes_a_caret_or_vertical_navigation_stop() {
        use schist_layout::{
            authoring, compose,
            lists::{ListKind, ListStyle},
            ParagraphStyle, Story,
        };
        for content in ["", "aé e\u{301}"] {
            for scale in [0.5, 1.0, 3.0] {
                for transform in [
                    schist_core::Affine::IDENTITY,
                    schist_core::Affine::rotate(0.2),
                    schist_core::Affine::skew(0.1, -0.2),
                ] {
                    let mut state = DesignState::new();
                    state.document.styles.add_paragraph(ParagraphStyle {
                        name: "List".into(),
                        point_size: Some(12.0),
                        left_indent: Some(30.0),
                        first_line_indent: Some(-30.0),
                        list: ListStyle {
                            kind: Some(ListKind::Numbered),
                            start: Some(999),
                            numbering_alignment: Some(schist_layout::lists::MarkerAlignment::Right),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                    let frame = authoring::text_frame(
                        &mut state.document,
                        &mut state.history,
                        0,
                        Rect::new(50.0, 50.0, 200.0, 180.0),
                    )
                    .unwrap();
                    let mut story = Story::from_text(content, "List");
                    story.push_paragraph("second", "List");
                    state.document.stories[frame.story.0 as usize] = story;
                    state.document.objects[0].transform = transform;
                    state.view.scale = scale;
                    let board = state.plan().unwrap();
                    for display in board.objects() {
                        if let Display::Text {
                            generated: true,
                            rect,
                            transform,
                            start,
                            ..
                        } = display
                        {
                            let at = schist_layout::affine::point(
                                *transform,
                                Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
                            );
                            assert_eq!(
                                super::super::select::hit_test(&board, at).object(),
                                Some(frame.object)
                            );
                            assert_eq!(
                                hit(&state, state.to_page(at), None),
                                Some((frame.object, *start))
                            );
                        }
                        let Display::Text {
                            generated: false,
                            spec,
                            rect,
                            transform,
                            start,
                            ..
                        } = display
                        else {
                            continue;
                        };
                        let origin = line_origin(spec, *rect);
                        for (position, caret) in schist_text_engine::insertion_points(spec) {
                            let at = schist_layout::affine::point(
                                *transform,
                                Point::new(
                                    origin.x + caret.x,
                                    origin.y + caret.top + caret.height / 2.0,
                                ),
                            );
                            assert_eq!(
                                hit(&state, state.to_page(at), Some(frame.object)),
                                Some((frame.object, start + position.byte))
                            );
                        }
                        assert!(tools::begin_typing(&mut state, frame.object, *start));
                        assert_eq!(
                            caret_line(&board, state.typing.unwrap()),
                            Some((frame.object, *start))
                        );
                    }
                    let flow = compose::compose_story(&state.document, frame.story);
                    let starts = flow
                        .lines()
                        .filter(|l| l.generated.is_none())
                        .map(|l| l.start)
                        .collect::<Vec<_>>();
                    assert_eq!(starts.len(), 2);
                    assert!(tools::begin_typing(&mut state, frame.object, starts[0]));
                    assert_eq!(adjacent_line_caret(&state, true), Some(starts[1]));
                }
            }
        }
    }

    #[test]
    fn path_glyphs_and_empty_baselines_are_clickable_and_keep_grapheme_carets_under_affines() {
        use schist_layout::{affine, text_path, ShapePath, Story};
        for curve in [false, true] {
            let mut state = DesignState::new();
            let mut path = if curve {
                ShapePath::ellipse(200.0, 100.0)
            } else {
                ShapePath {
                    subpaths: vec![schist_layout::SubPath {
                        points: vec![Point::ZERO, Point::new(200.0, 0.0)],
                        ..Default::default()
                    }],
                    even_odd: false,
                }
            };
            path.map_points(|p| p + Point::new(50.0, 90.0));
            let id = authoring::path_shape(
                &mut state.document,
                &mut state.history,
                0,
                path,
                authoring::Paint::none(),
            )
            .unwrap();
            let frame = text_path::attach(&mut state.document, &mut state.history, id).unwrap();
            for content in ["", "aé e\u{301} xyz"] {
                state.document.stories[frame.story.0 as usize] =
                    Story::from_text(content, "Default");
                for scale in [0.5, 1.0, 3.0] {
                    state.view.scale = scale;
                    for transform in [
                        affine::Affine::IDENTITY,
                        affine::Affine::rotate(0.3),
                        affine::Affine::skew(0.2, -0.1),
                    ] {
                        state.document.objects[0].transform = transform;
                        let board = state.plan().unwrap();
                        let (spec, rect, matrix) = board
                            .objects()
                            .find_map(|d| match d {
                                Display::Text {
                                    object,
                                    spec,
                                    rect,
                                    transform,
                                    ..
                                } if *object == id => Some((spec, rect, transform)),
                                _ => None,
                            })
                            .unwrap();
                        let origin = line_origin(spec, *rect);
                        for (byte, caret) in schist_text_engine::carets(spec) {
                            let point = affine::point(
                                *matrix,
                                Point::new(
                                    origin.x + caret.x - caret.angle.sin() * caret.height / 2.0,
                                    origin.y + caret.top + caret.angle.cos() * caret.height / 2.0,
                                ),
                            );
                            assert_eq!(
                                super::super::select::hit_test(&board, point).object(),
                                Some(id),
                                "{curve} {content:?} {scale} {byte}, point={point:?}, matrix={matrix:?}, frames={:?}", board.objects().filter_map(|d| d.frame()).collect::<Vec<_>>()
                            );
                            let point = state.to_page(point);
                            assert_eq!(
                                hit(&state, point, None),
                                Some((id, byte)),
                                "{curve} {content:?} {scale} {byte}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn shifted_text_hits_its_visible_line_at_every_zoom_and_frame_transform() {
        use schist_layout::{
            affine::{self, Affine},
            compose,
            styles::BaselineShift,
            ParagraphStyle, Story, WritingMode,
        };
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRightToLeft,
            WritingMode::VerticalLeftToRight,
        ] {
            for shift in [-25.0, 25.0] {
                let mut state = DesignState::new();
                state.document.styles.add_paragraph(ParagraphStyle {
                    name: "Shifted".into(),
                    point_size: Some(12.0),
                    leading: Some(schist_layout::styles::Leading::Points(18.0)),
                    writing_mode: Some(mode),
                    baseline_shift: Some(BaselineShift::Offset(shift)),
                    ..Default::default()
                });
                let frame = authoring::text_frame(
                    &mut state.document,
                    &mut state.history,
                    0,
                    Rect::new(40.0, 40.0, 150.0, 150.0),
                )
                .unwrap();
                *state.document.story_mut(frame.story) =
                    Story::from_text("abc e\u{301} fg hi jkl mn ".repeat(10), "Shifted");
                let thread = compose::compose_story(&state.document, frame.story);
                assert!(thread.lines().count() > 2);
                for line in thread.lines().skip(1).take(3) {
                    let spec = compose::line_spec(
                        line,
                        state.document.story(frame.story).unwrap(),
                        &state.document,
                    );
                    let origin = line_origin(&spec, line.bounds);
                    let boundaries: Vec<_> =
                        schist_text_engine::grapheme_boundaries(&spec.text).collect();
                    let byte = boundaries[boundaries.len() / 2];
                    let caret = schist_text_engine::caret_at(&spec, byte).unwrap();
                    let local = Point::new(
                        origin.x + caret.x - caret.angle.sin() * caret.height / 2.0,
                        origin.y + caret.top + caret.angle.cos() * caret.height / 2.0,
                    );
                    for scale in [0.5, 1.0, 3.0] {
                        state.view.scale = scale;
                        for matrix in [
                            Affine::IDENTITY,
                            Affine::rotate(0.4),
                            Affine::skew(0.2, -0.1),
                        ] {
                            state.document.objects[0].transform = matrix;
                            let point =
                                affine::point(state.document.objects[0].content_transform(), local);
                            assert_eq!(
                                hit(&state, point, Some(frame.object)),
                                Some((frame.object, line.start + byte))
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn vertical_hits_and_cross_column_carets_follow_shaped_graphemes() {
        use schist_layout::{
            affine::{self, Affine},
            compose, ParagraphStyle, Story, WritingMode,
        };
        for mode in [
            WritingMode::VerticalRightToLeft,
            WritingMode::VerticalLeftToRight,
        ] {
            for align in [
                schist_layout::styles::Align::Left,
                schist_layout::styles::Align::Center,
                schist_layout::styles::Align::Right,
            ] {
                let mut state = DesignState::new();
                state.document.styles.add_paragraph(ParagraphStyle {
                    name: "Vertical".into(),
                    point_size: Some(11.0),
                    leading: Some(schist_layout::styles::Leading::Points(17.0)),
                    writing_mode: Some(mode),
                    align: Some(align),
                    left_indent: Some(5.0),
                    first_line_indent: Some(8.0),
                    keep_lines: Some(1),
                    ..Default::default()
                });
                let frame = authoring::text_frame(
                    &mut state.document,
                    &mut state.history,
                    0,
                    Rect::new(40.0, 50.0, 200.0, 130.0),
                )
                .unwrap();
                *state.document.story_mut(frame.story) =
                    Story::from_text("abc e\u{301} fg hi jkl mn ".repeat(4), "Vertical");
                let thread = compose::compose_story(&state.document, frame.story);
                assert!(!thread.has_overflow());
                let lines: Vec<_> = thread.lines().collect();
                assert!(lines.len() > 2);
                for (index, line) in lines.iter().enumerate() {
                    let spec = compose::line_spec(
                        line,
                        state.document.story(frame.story).unwrap(),
                        &state.document,
                    );
                    let origin = line_origin(&spec, line.bounds);
                    let bytes: Vec<_> =
                        schist_text_engine::grapheme_boundaries(&spec.text).collect();
                    let offset = bytes[bytes.len() / 2];
                    let caret = schist_text_engine::caret_at(&spec, offset).unwrap();
                    let local = Point::new(
                        origin.x + caret.x - caret.height / 2.0,
                        origin.y + caret.top,
                    );
                    for zoom in [0.5, 1.0, 3.0] {
                        state.view.scale = zoom;
                        for matrix in [
                            Affine::IDENTITY,
                            Affine::rotate(0.4),
                            Affine::skew(0.2, -0.1),
                        ] {
                            state.document.objects[0].transform = matrix;
                            let at =
                                affine::point(state.document.objects[0].content_transform(), local);
                            assert_eq!(
                                hit(&state, at, Some(frame.object)),
                                Some((frame.object, line.start + offset))
                            );
                        }
                    }
                    assert!(tools::begin_typing(
                        &mut state,
                        frame.object,
                        line.start + offset
                    ));
                    for forward in [false, true] {
                        let target = if forward {
                            lines.get(index + 1)
                        } else {
                            index.checked_sub(1).and_then(|i| lines.get(i))
                        };
                        let Some(target) = target else {
                            continue;
                        };
                        let target_spec = compose::line_spec(
                            target,
                            state.document.story(frame.story).unwrap(),
                            &state.document,
                        );
                        let target_origin = line_origin(&target_spec, target.bounds);
                        let at = adjacent_line_caret(&state, forward).unwrap() - target.start;
                        let desired = local.y - line.inline_origin + target.inline_origin;
                        let distance = |offset| {
                            let caret = schist_text_engine::caret_at(&target_spec, offset).unwrap();
                            (target_origin.y + caret.top - desired).abs()
                        };
                        for boundary in schist_text_engine::grapheme_boundaries(&target_spec.text) {
                            assert!(distance(at) <= distance(boundary) + 0.01);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn enlarged_initials_keep_grapheme_hit_testing_and_one_step_text_undo() {
        for writing in [
            schist_layout::WritingMode::Horizontal,
            schist_layout::WritingMode::VerticalLeftToRight,
            schist_layout::WritingMode::VerticalRightToLeft,
        ] {
            for prefix in ["A", "E\u{301}", "Éc"] {
                let mut state = DesignState::new();
                state
                    .document
                    .styles
                    .add_paragraph(schist_layout::ParagraphStyle {
                        name: "Initial".into(),
                        writing_mode: Some(writing),
                        point_size: Some(11.0),
                        leading: Some(schist_layout::styles::Leading::Points(14.0)),
                        drop_caps_lines: Some(3),
                        drop_caps_characters: Some(
                            schist_text_engine::grapheme_boundaries(prefix).count() - 1,
                        ),
                        ..Default::default()
                    });
                let frame = authoring::text_frame(
                    &mut state.document,
                    &mut state.history,
                    0,
                    Rect::new(40.0, 50.0, 200.0, 300.0),
                )
                .unwrap();
                let text = format!(
                    "{prefix} few words to make an initial wrap with three lines of body text. "
                )
                .repeat(3);
                *state.document.story_mut(frame.story) =
                    schist_layout::Story::from_text(&text, "Initial");
                let composed = schist_layout::compose::compose_story(&state.document, frame.story);
                let line = composed.lines().next().unwrap();
                let ink = line.initial.unwrap().ink;
                for zoom in [0.5, 1.0, 3.0] {
                    state.view.scale = zoom;
                    for fraction in [0.1, 0.5, 0.9] {
                        let at = if writing == schist_layout::WritingMode::Horizontal {
                            Point::new(ink.x + ink.width * fraction, ink.y + ink.height * 0.5)
                        } else {
                            Point::new(ink.x + ink.width * 0.5, ink.y + ink.height * fraction)
                        };
                        let (_, offset) = hit(&state, at, Some(frame.object)).unwrap();
                        assert!(
                            schist_text_engine::grapheme_boundaries(prefix).any(|b| b == offset)
                        );
                        assert!(tools::begin_typing(&mut state, frame.object, offset));
                        let before = state.document.clone();
                        let depth = state.history.undo_depth();
                        assert!(tools::type_text(&mut state, "字"));
                        assert_eq!(state.history.undo_depth(), depth + 1);
                        assert!(state.history.undo(&mut state.document));
                        assert_eq!(state.document, before);
                    }
                }
            }
        }
    }

    #[test]
    fn empty_paragraphs_have_clickable_carets_and_vertical_navigation() {
        for blanks in 1..5 {
            let mut state = DesignState::new();
            let frame = authoring::text_frame(
                &mut state.document,
                &mut state.history,
                0,
                Rect::new(40.0, 50.0, 180.0, 300.0),
            )
            .unwrap();
            let mut story = schist_layout::Story::from_text("head", "Body");
            for _ in 0..blanks {
                story.push_paragraph("", "Body");
            }
            story.push_paragraph("tail", "Body");
            let offsets = story.point_offsets();
            state.document.stories[frame.story.0 as usize] = story;
            let composed = schist_layout::compose_object(
                &state.document,
                state.document.object(frame.object).unwrap(),
            )
            .unwrap();
            for zoom in [0.5, 1.0, 3.0] {
                state.view.scale = zoom;
                for line in composed.lines.iter().filter(|l| l.forced_break) {
                    let at = Point::new(
                        line.bounds.x + 2.0,
                        line.bounds.y + line.bounds.height / 2.0,
                    );
                    assert_eq!(
                        hit(&state, at, Some(frame.object)),
                        Some((frame.object, line.start))
                    );
                    assert!(tools::begin_typing(&mut state, frame.object, line.start));
                    assert_eq!(
                        caret_line(&state.plan().unwrap(), state.typing.unwrap()),
                        Some((frame.object, line.start))
                    );
                    let before = state.document.clone();
                    let depth = state.history.undo_depth();
                    assert!(tools::type_text(&mut state, "字"));
                    assert_eq!(state.history.undo_depth(), depth + 1);
                    let mut expected = before.story(frame.story).unwrap().text();
                    expected.insert(line.start, '字');
                    assert_eq!(authoring::text_of(&state.document, frame.story), expected);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
            }
            for (index, at) in offsets.iter().enumerate() {
                assert!(tools::begin_typing(&mut state, frame.object, *at));
                if index + 1 < offsets.len() {
                    assert_eq!(adjacent_line_caret(&state, true), Some(offsets[index + 1]));
                }
                if index > 0 {
                    assert_eq!(adjacent_line_caret(&state, false), Some(offsets[index - 1]));
                }
            }
        }
    }

    #[test]
    fn selecting_across_frames_replaces_one_story_in_one_undo() {
        for text in [
            "one two three four five six seven eight nine",
            "héllö 世界 tail",
            "אבג text τέλος",
        ] {
            let mut state = DesignState::new();
            let a = authoring::text_frame(
                &mut state.document,
                &mut state.history,
                0,
                Rect::new(10.0, 10.0, 100.0, 20.0),
            )
            .unwrap();
            let b = authoring::text_frame(
                &mut state.document,
                &mut state.history,
                0,
                Rect::new(10.0, 50.0, 100.0, 100.0),
            )
            .unwrap();
            authoring::set_text(&mut state.document, &mut state.history, a.story, text);
            assert!(threading::link(
                &mut state.document,
                &mut state.history,
                a.object,
                b.object
            ));
            let boundaries: Vec<_> = text
                .char_indices()
                .map(|(i, _)| i)
                .chain([text.len()])
                .collect();
            for start in &boundaries {
                for (end, replacement) in boundaries
                    .iter()
                    .filter(|end| *end >= start)
                    .flat_map(|end| ["漢字", "\t"].map(|replacement| (end, replacement)))
                {
                    assert!(tools::begin_typing(&mut state, a.object, *start));
                    tools::select_to(&mut state, *end, true);
                    let before = state.document.clone();
                    let depth = state.history.undo_depth();
                    assert!(tools::type_text(&mut state, replacement));
                    let mut expected = text.to_owned();
                    expected.replace_range(*start..*end, replacement);
                    assert_eq!(authoring::text_of(&state.document, a.story), expected);
                    assert_eq!(state.history.undo_depth(), depth + 1);
                    state.history.undo(&mut state.document);
                    assert_eq!(state.document, before);
                }
            }
        }
    }

    #[test]
    fn every_text_hit_and_shared_boundary_has_one_valid_caret() {
        let mut state = DesignState::new();
        let a = authoring::text_frame(
            &mut state.document,
            &mut state.history,
            0,
            Rect::new(10.0, 10.0, 130.0, 100.0),
        )
        .unwrap();
        authoring::set_text(
            &mut state.document,
            &mut state.history,
            a.story,
            "héllo 世界
second line",
        );
        for zoom in [0.5, 1.0, 3.0] {
            state.view.scale = zoom;
            state.view.origin = Point::new(83.0, -13.0);
            for x in (0..140).step_by(3) {
                let (object, at) = hit(&state, Point::new(x as f32, 15.0), Some(a.object)).unwrap();
                assert!(tools::begin_typing(&mut state, object, at));
                assert!(state.text_buffer.is_char_boundary(at));
                assert!(caret_line(&state.plan().unwrap(), state.typing.unwrap()).is_some());
            }
        }
    }
    #[test]
    fn affine_text_hits_match_untransformed_hits_at_every_zoom() {
        use schist_layout::affine::{self, Affine};
        let mut state = DesignState::new();
        let frame = authoring::text_frame(
            &mut state.document,
            &mut state.history,
            0,
            Rect::new(80.0, 80.0, 160.0, 90.0),
        )
        .unwrap();
        authoring::set_text(
            &mut state.document,
            &mut state.history,
            frame.story,
            "héllo 世界 text wraps inside this frame.",
        );
        for zoom in [0.25, 1.0, 3.0] {
            state.view.scale = zoom;
            state.view.origin = Point::new(31.0, -23.0);
            for matrix in [
                Affine::rotate(0.7),
                Affine::skew(0.4, -0.2),
                Affine::scale(-1.2, 0.8),
            ] {
                for x in (85..230).step_by(7) {
                    let at = Point::new(x as f32, 86.0);
                    state.document.objects[0].transform = Affine::IDENTITY;
                    let expected = hit(&state, at, Some(frame.object)).unwrap();
                    state.document.objects[0].transform = matrix;
                    let transformed =
                        affine::point(state.document.objects[0].content_transform(), at);
                    assert_eq!(hit(&state, transformed, Some(frame.object)), Some(expected));
                    let plan = state.plan().unwrap();
                    assert_eq!(
                        super::super::select::hit_test(&plan, state.to_pasteboard(transformed))
                            .object(),
                        Some(frame.object)
                    );
                }
            }
        }
    }
}
