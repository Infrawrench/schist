//! Tables breaking across frames, as InDesign's PDF of the public paged-media
//! `tables-rows` sample shows: two 250 pt frames side by side, a header row
//! and ten body rows of two or three lines. The first frame holds the header
//! and rows 1 to 5, the second repeats the header and holds rows 6 to 10; with
//! KeepWithNextRow on rows 3 to 6 the first frame stops after row 2 and the
//! second ends after row 7; a row taller than its frame leaves it and the rows
//! after it overset. These tests set IBM Plex Sans (ascent 12.3 pt at 12 pt),
//! so rows are 20.3, 34.7 and 49.1 pt rather than the sample's Open Sans
//! 20.826, 35.226 and 49.626 pt, and the breaks fall at the same rows.
use schist_layout::styles::ParagraphStart;
use schist_layout::tables::{
    self, CellEdge, CellJustification, CellPaint, RepeatRows, Table, TableCell, TableRow,
};
use schist_layout::{
    anchored, authoring, blank_a4, compose::compose_story, threading, History, Ink, Insets,
    LayoutDocument, LayoutObject, ObjectId, PlacedObject, Rect, Story, StoryId, StoryStructure,
};

const FIRST: Rect = Rect::new(36.0, 120.0, 250.0, 250.0);
const SECOND: Rect = Rect::new(310.0, 120.0, 250.0, 250.0);

fn fonts(doc: &mut LayoutDocument) {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let body = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap();
    body.family = Some("IBM Plex Sans".into());
    body.point_size = Some(12.0);
    body.leading = Some(schist_layout::styles::Leading::Auto);
}

fn black() -> CellEdge {
    CellEdge {
        weight: 1.0,
        paint: Some(CellPaint {
            ink: Ink::black(),
            tint: 1.0,
        }),
        source: Default::default(),
    }
}

fn grows() -> TableRow {
    TableRow {
        height: 3.0,
        minimum: 3.0,
        maximum: None,
        auto_grow: true,
        keep_with_next: false,
        start: None,
    }
}

/// The sample's breaking table: a header row and ten body rows, the first
/// cell two lines, the second one to three.
fn sample_rows() -> Vec<Vec<usize>> {
    let mut rows = vec![vec![1, 1]];
    for r in 1..=10 {
        rows.push(vec![2, 1 + r % 3]);
    }
    rows
}

/// Threaded frames holding `before` paragraphs, then a table of 120 pt
/// columns whose cells hold the given numbers of lines.
fn document(
    frames: &[Rect],
    columns: u16,
    before: &[&str],
    header: usize,
    footer: usize,
    lines: &[Vec<usize>],
) -> (LayoutDocument, Vec<ObjectId>, StoryId) {
    let mut doc = blank_a4();
    fonts(&mut doc);
    let mut history = History::default();
    let mut ids = Vec::new();
    for rect in frames {
        let frame = authoring::text_frame(&mut doc, &mut history, 0, *rect).unwrap();
        ids.push(frame.object);
    }
    for pair in ids.windows(2) {
        assert!(threading::link(&mut doc, &mut history, pair[0], pair[1]));
    }
    if columns > 1 {
        for id in &ids {
            let object = doc.objects.iter_mut().find(|o| o.id == *id).unwrap();
            if let LayoutObject::TextFrame {
                columns: count,
                gutter,
                ..
            } = &mut object.object
            {
                *count = columns;
                *gutter = 10.0;
            }
        }
    }
    let story = threading::story_of(&doc, ids[0]).unwrap();
    let mut cells = Vec::new();
    for (row, contents) in lines.iter().enumerate() {
        for (column, count) in contents.iter().enumerate() {
            let mut text = Story::new();
            for line in 0..*count {
                text.push_paragraph(format!("R{row}C{column} · p{}", line + 1), "Body");
            }
            doc.stories.push(text);
            cells.push(TableCell {
                column,
                row,
                columns: 1,
                rows: 1,
                story: StoryId((doc.stories.len() - 1) as u32),
                fill: None,
                insets: Insets::uniform(4.0),
                justification: CellJustification::Top,
                own_fill: false,
                edges: [black(), black(), black(), black()],
            });
        }
    }
    let table = Table {
        header_rows: header,
        footer_rows: footer,
        rows: vec![grows(); lines.len()],
        columns: vec![120.0; lines[0].len()],
        cells,
        header_repeat: RepeatRows::EveryColumn,
        footer_repeat: RepeatRows::EveryColumn,
        skip_first_header: false,
        skip_last_footer: false,
        row_fills: None,
        column_fills: None,
        space_before: 0.0,
        space_after: 0.0,
        stroke_order: Default::default(),
    };
    let mut host = Story::new();
    for paragraph in before {
        host.push_paragraph(*paragraph, "Body");
    }
    let (at, _) = host.push_paragraph("", "Body");
    host.structures.push(StoryStructure {
        at: Some(at),
        kind: "Table".into(),
        payload: "<Table />".into(),
        control: None,
        footnote: None,
        table: Some(Box::new(table)),
        anchored: None,
    });
    doc.stories[story.0 as usize] = host;
    (doc, ids, story)
}

fn table_mut(doc: &mut LayoutDocument, story: StoryId) -> &mut Table {
    doc.stories[story.0 as usize].structures[0]
        .table
        .as_mut()
        .unwrap()
}

/// What each frame of the thread draws for the table.
fn drawn(doc: &LayoutDocument, story: StoryId) -> Vec<Vec<PlacedObject>> {
    let thread = compose_story(doc, story);
    thread
        .frames
        .iter()
        .map(|frame| {
            anchored::placements(
                doc,
                &doc.stories[story.0 as usize],
                doc.object(frame.object).unwrap(),
                &frame.lines,
            )
        })
        .collect()
}

/// The rows whose first cell each frame draws, in drawing order.
fn rows(doc: &LayoutDocument, story: StoryId) -> Vec<Vec<usize>> {
    drawn(doc, story)
        .iter()
        .map(|objects| {
            objects
                .iter()
                .filter(|o| matches!(o.object, LayoutObject::TextFrame { .. }))
                .filter_map(|o| o.name.strip_prefix("0:")?.parse().ok())
                .collect()
        })
        .collect()
}

/// The horizontal grid lines a frame draws, top to bottom.
fn horizontals(objects: &[PlacedObject]) -> Vec<f32> {
    let mut out: Vec<f32> = objects
        .iter()
        .filter(|o| {
            matches!(
                o.object,
                LayoutObject::Shape {
                    stroke: Some(_),
                    ..
                }
            ) && o.bounds.height == 0.0
        })
        .map(|o| o.bounds.y)
        .collect();
    out.sort_by(f32::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    out
}

fn near(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 0.01, "{what}: {a} vs {b}");
}

#[test]
fn a_table_breaks_between_rows_and_repeats_its_header() {
    let (doc, _, story) = document(&[FIRST, SECOND], 1, &[], 1, 0, &sample_rows());
    assert_eq!(
        rows(&doc, story),
        [vec![0, 1, 2, 3, 4, 5], vec![0, 6, 7, 8, 9, 10]]
    );
    let flow = compose_story(&doc, story);
    assert!(!flow.frames.iter().any(|f| f.lost));
    assert_eq!(flow.frames[0].unrendered_structures, 0);
    // Each part's outer edge at its frame's top; rows of 20.3, 34.7 and
    // 49.1 pt as in one piece.
    let drawn = drawn(&doc, story);
    let first = horizontals(&drawn[0]);
    let second = horizontals(&drawn[1]);
    near(first[0], 120.5, "first part's top grid line");
    near(first[1] - first[0], 20.3, "header");
    near(first[3] - first[2], 49.1, "row 2");
    near(
        *first.last().unwrap(),
        120.5 + 20.3 + 3.0 * 34.7 + 2.0 * 49.1,
        "row 5's foot",
    );
    near(second[0], 120.5, "second part's top grid line");
    near(second[1] - second[0], 20.3, "repeated header");
    assert_eq!(second.len(), 7);
}

#[test]
fn rows_kept_with_the_next_move_on_together() {
    let (mut doc, _, story) = document(&[FIRST, SECOND], 1, &[], 1, 0, &sample_rows());
    for row in 3..=6 {
        table_mut(&mut doc, story).rows[row].keep_with_next = true;
    }
    assert_eq!(rows(&doc, story), [vec![0, 1, 2], vec![0, 3, 4, 5, 6, 7]]);
    // Rows 8 to 10 fit nowhere: overset.
    assert!(compose_story(&doc, story).frames[1].lost);
}

#[test]
fn keeps_give_way_where_nothing_else_fills_a_column() {
    let (mut doc, _, story) = document(&[FIRST, SECOND], 1, &[], 1, 0, &sample_rows());
    for row in 1..10 {
        table_mut(&mut doc, story).rows[row].keep_with_next = true;
    }
    // Every row is kept with the next: the table still breaks where a whole
    // column fills.
    assert_eq!(
        rows(&doc, story),
        [vec![0, 1, 2, 3, 4, 5], vec![0, 6, 7, 8, 9, 10]]
    );
}

#[test]
fn a_row_taller_than_every_frame_leaves_the_rest_overset() {
    let short = |r: Rect| Rect::new(r.x, r.y, r.width, 150.0);
    let (doc, _, story) = document(
        &[short(FIRST), short(SECOND)],
        1,
        &[],
        0,
        0,
        &[vec![1, 1], vec![12, 1], vec![1, 1]],
    );
    assert_eq!(rows(&doc, story), [vec![0], vec![]]);
    let flow = compose_story(&doc, story);
    assert!(flow.frames[1].lost);
}

#[test]
fn a_table_after_text_fills_the_room_left() {
    let before = ["Opening paragraph", "Second paragraph", "Third paragraph"];
    // Eight body rows: what the first frame does not hold fits the second.
    let lines = &sample_rows()[..9];
    let (doc, _, story) = document(&[FIRST, SECOND], 1, &before, 1, 0, lines);
    let shown = rows(&doc, story);
    let drawn = drawn(&doc, story);
    let first = horizontals(&drawn[0]);
    let bottom = *first.last().unwrap() + 0.5;
    assert!(bottom <= FIRST.bottom() + 0.01, "{bottom}");
    // The row that moved on would not have fitted below it.
    let next = shown[1][1];
    let next_height = if 1 + next % 3 == 3 { 49.1 } else { 34.7 };
    assert!(bottom + next_height > FIRST.bottom(), "{shown:?}");
    // Every body row is drawn once, the header in both frames.
    let mut body: Vec<usize> = shown.concat().into_iter().filter(|r| *r > 0).collect();
    body.sort();
    assert_eq!(body, (1..=8).collect::<Vec<_>>());
    assert_eq!(shown[1][0], 0);
    // The table sits below the text.
    let thread = compose_story(&doc, story);
    let text_bottom = thread.frames[0]
        .lines
        .iter()
        .filter(|l| l.projected.as_ref().is_none_or(|p| p.tables.is_empty()))
        .map(|l| l.baseline)
        .fold(f32::MIN, f32::max);
    assert!(first[0] > text_bottom, "{} vs {text_bottom}", first[0]);
}

/// The parts set in a thread, in order.
fn parts(doc: &LayoutDocument, story: StoryId) -> Vec<tables::SetPart> {
    compose_story(doc, story)
        .lines()
        .filter_map(|l| l.projected.as_ref())
        .flat_map(|p| p.tables.clone())
        .collect()
}

#[test]
fn headers_repeat_once_per_frame_and_footers_where_asked() {
    // Two frames of two 125 pt columns; one 120 pt column of two-line body
    // rows between a header and a footer.
    let frame = |y: f32| Rect::new(36.0, y, 260.0, 130.0);
    let mut lines: Vec<Vec<usize>> = sample_rows().into_iter().map(|_| vec![2]).collect();
    lines[0] = vec![1];
    lines.push(vec![1]);
    let (mut doc, _, story) = document(&[frame(120.0), frame(400.0)], 2, &[], 1, 1, &lines);
    table_mut(&mut doc, story).header_repeat = RepeatRows::OncePerFrame;
    let set = parts(&doc, story);
    let headers: Vec<bool> = set.iter().map(|s| s.part.header).collect();
    let footers: Vec<bool> = set.iter().map(|s| s.part.footer).collect();
    // A header opens each frame's first part; every part ends in the footer.
    assert_eq!(headers, [true, false, true, false], "{set:?}");
    assert_eq!(footers, [true; 4]);
    // With the header without its 20.3 pt, three rows fit beside two.
    let body: Vec<usize> = set.iter().map(|s| s.part.end - s.part.start).collect();
    assert_eq!(body, [2, 3, 2, 3]);
    assert_eq!(rows(&doc, story)[0], [0, 1, 2, 11, 3, 4, 5, 11]);
    // The first header and the last footer skipped.
    {
        let table = table_mut(&mut doc, story);
        table.header_repeat = RepeatRows::EveryColumn;
        table.skip_first_header = true;
        table.skip_last_footer = true;
    }
    let set = parts(&doc, story);
    assert!(!set[0].part.header && set[1].part.header, "{set:?}");
    assert!(set[0].part.footer && !set.last().unwrap().part.footer);
}

#[test]
fn a_table_sits_on_lines_of_its_own() {
    let (mut doc, _, story) = document(&[FIRST], 1, &[], 0, 0, &[vec![1, 1]]);
    // Text before and after the table in its own paragraph.
    let host = &mut doc.stories[story.0 as usize];
    let table = host.structures[0].clone();
    let mut text = Story::from_text("Before after", "Body");
    text.structures = vec![StoryStructure {
        at: Some(7),
        ..table
    }];
    *host = text;
    let thread = compose_story(&doc, story);
    let lines = &thread.frames[0].lines;
    assert_eq!(lines.len(), 3, "{lines:#?}");
    let table_line = lines
        .iter()
        .position(|l| l.projected.as_ref().is_some_and(|p| !p.tables.is_empty()))
        .unwrap();
    assert_eq!(table_line, 1);
    assert!(lines[0].baseline < lines[1].baseline && lines[1].baseline < lines[2].baseline);
    assert_eq!(lines[0].start, 0);
    assert_eq!(lines[2].end, 12);
    // The table's top is below the first line, its bottom above the last.
    let objects = drawn(&doc, story);
    let grid = horizontals(&objects[0]);
    assert!(grid[0] > lines[0].baseline);
    assert!(*grid.last().unwrap() < lines[2].baseline);
}

#[test]
fn fixed_leading_never_sets_a_table_over_the_text_above() {
    let (mut doc, _, story) = document(&[FIRST], 1, &["Above"], 0, 0, &[vec![3, 1]]);
    let body = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap();
    body.leading = Some(schist_layout::styles::Leading::Points(14.0));
    let thread = compose_story(&doc, story);
    let above = thread.frames[0].lines[0].baseline;
    let grid = horizontals(&drawn(&doc, story)[0]);
    assert!(grid[0] > above, "{} vs {above}", grid[0]);
}

/// InDesign's PDF of the public paged-media `tables-overset` sample: three
/// 120 pt columns of 28 pt rows in a 360 pt frame (the 361 pt table
/// overhangs it) of varying height. Rows fit whole or are overset; a part
/// never stands without a body row, so a frame with room for the header
/// alone shows nothing.
#[test]
fn rows_fit_whole_or_are_overset_as_in_the_overset_sample() {
    let cases: [(f32, usize, &[usize]); 5] = [
        (200.0, 0, &[0, 1, 2, 3]),
        (62.0, 0, &[0, 1]),
        (20.0, 0, &[]),
        (20.0, 1, &[]),
        (30.0, 1, &[]),
    ];
    for (height, header, expected) in cases {
        let frame = Rect::new(117.638, 120.0, 360.0, height);
        let lines = vec![vec![1, 1, 1]; 4];
        let (mut doc, _, story) = document(&[frame], 1, &[], header, 0, &lines);
        for row in &mut table_mut(&mut doc, story).rows {
            row.minimum = 28.0;
        }
        let what = format!("{height} pt frame, {header} header row");
        assert_eq!(rows(&doc, story), [expected.to_vec()], "{what}");
        let lost = compose_story(&doc, story).frames[0].lost;
        assert_eq!(lost, expected.len() < 4, "{what}");
        if expected.len() == 4 {
            let objects = &drawn(&doc, story)[0];
            let grid = horizontals(objects);
            near(grid[0], 120.5, "top grid line");
            near(*grid.last().unwrap(), 232.5, "bottom grid line");
            let right = objects
                .iter()
                .filter(|o| o.bounds.height == 0.0)
                .map(|o| o.bounds.right())
                .fold(f32::MIN, f32::max);
            near(right, 478.638, "the table overhangs its frame");
        }
    }
}

/// A body row whose StartRow asks for the next column, frame or page ends
/// the part before it, and its part starts there with the header repeated,
/// as the public specification describes; nothing in the samples sets one.
#[test]
fn a_row_starting_later_breaks_the_table_before_it() {
    let lines = vec![vec![1, 1]; 6];
    let (mut doc, _, story) = document(&[FIRST, SECOND], 1, &[], 1, 0, &lines);
    assert_eq!(rows(&doc, story), [vec![0, 1, 2, 3, 4, 5], vec![]]);
    table_mut(&mut doc, story).rows[3].start = Some(ParagraphStart::NextFrame);
    assert_eq!(rows(&doc, story), [vec![0, 1, 2], vec![0, 3, 4, 5]]);
    // The part starts at its frame's top, no empty line above it.
    let drawn = drawn(&doc, story);
    near(
        horizontals(&drawn[1])[0],
        120.5,
        "second part's top grid line",
    );
    assert!(!compose_story(&doc, story).frames.iter().any(|f| f.lost));
    // In two columns of one frame, the next column.
    let (mut doc, _, story) = document(&[FIRST], 2, &[], 1, 0, &lines);
    table_mut(&mut doc, story).rows[2].start = Some(ParagraphStart::NextColumn);
    assert_eq!(rows(&doc, story), [vec![0, 1, 0, 2, 3, 4, 5]]);
    let parts = parts(&doc, story);
    assert_eq!(parts.len(), 2);
    assert_eq!((parts[1].part.start, parts[1].part.end), (2, 6));
}

/// A table whose first row asks to start in the next frame moves on from
/// the text before it, and stays put with nothing before it.
#[test]
fn a_table_whose_first_row_starts_later_moves_on() {
    let lines = vec![vec![1, 1]; 3];
    let (mut doc, _, story) = document(&[FIRST, SECOND], 1, &["Opening"], 0, 0, &lines);
    table_mut(&mut doc, story).rows[0].start = Some(ParagraphStart::NextFrame);
    assert_eq!(rows(&doc, story), [vec![], vec![0, 1, 2]]);
    near(
        horizontals(&drawn(&doc, story)[1])[0],
        120.5,
        "table at the second frame's top",
    );
    let (mut doc, _, story) = document(&[FIRST, SECOND], 1, &[], 0, 0, &lines);
    table_mut(&mut doc, story).rows[0].start = Some(ParagraphStart::NextFrame);
    assert_eq!(rows(&doc, story), [vec![0, 1, 2], vec![]]);
}
