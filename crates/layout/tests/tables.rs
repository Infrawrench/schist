//! Tables set in text. Row heights follow InDesign's PDF of the public
//! paged-media `tables-rows` sample: a growing row is its top inset, the last
//! baseline of its tallest cell's text below that inset, and its bottom inset
//! (one 12 pt Open Sans line with 4 pt insets: 20.826 pt; three: 49.626 pt).
//! These tests set IBM Plex Sans, whose ascent at 12 pt is 12.3 pt.
use schist_layout::tables::{
    self, CellEdge, CellJustification, CellPaint, Table, TableCell, TableRow,
};
use schist_layout::{
    anchored, authoring, blank_a4, compose::compose_story, History, Ink, Insets, LayoutDocument,
    LayoutObject, ObjectId, PlacedObject, Rect, Story, StoryId, StoryStructure,
};

const ASCENT: f32 = 12.3;
const LEADING: f32 = 14.4;
const FRAME: Rect = Rect::new(72.0, 120.0, 400.0, 600.0);

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
    }
}

fn grows(minimum: f32) -> TableRow {
    TableRow {
        height: minimum,
        minimum,
        maximum: None,
        auto_grow: true,
        keep_with_next: false,
    }
}

/// A frame holding one table: `cells[row][column]` paragraphs per cell.
fn document(
    rows: Vec<TableRow>,
    columns: Vec<f32>,
    cells: Vec<Vec<&[&str]>>,
) -> (LayoutDocument, ObjectId, Table) {
    let mut doc = blank_a4();
    fonts(&mut doc);
    doc.inks.push(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]));
    let frame = authoring::text_frame(&mut doc, &mut History::default(), 0, FRAME).unwrap();
    let mut table_cells = Vec::new();
    for (row, contents) in cells.iter().enumerate() {
        for (column, paragraphs) in contents.iter().enumerate() {
            let mut story = Story::new();
            for paragraph in *paragraphs {
                story.push_paragraph(*paragraph, "Body");
            }
            doc.stories.push(story);
            table_cells.push(TableCell {
                column,
                row,
                columns: 1,
                rows: 1,
                story: StoryId((doc.stories.len() - 1) as u32),
                fill: None,
                insets: Insets::uniform(4.0),
                justification: CellJustification::Top,
                edges: [black(), black(), black(), black()],
            });
        }
    }
    let table = Table {
        header_rows: 0,
        footer_rows: 0,
        rows,
        columns,
        cells: table_cells,
        header_repeat: Default::default(),
        footer_repeat: Default::default(),
        skip_first_header: false,
        skip_last_footer: false,
    };
    let mut host = Story::from_text("", "Body");
    host.structures.push(StoryStructure {
        at: Some(0),
        kind: "Table".into(),
        payload: "<Table />".into(),
        control: None,
        footnote: None,
        table: Some(Box::new(table.clone())),
        anchored: None,
    });
    doc.stories[frame.story.0 as usize] = host;
    (doc, frame.object, table)
}

fn near(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 0.01, "{what}: {a} vs {b}");
}

fn steps(layout: &tables::TableLayout) -> Vec<f32> {
    layout.y.windows(2).map(|w| w[1] - w[0]).collect()
}

#[test]
fn growing_rows_take_their_tallest_cells_last_baseline() {
    let one: &[&str] = &["one"];
    let two: &[&str] = &["one", "two"];
    let three: &[&str] = &["one", "two", "three"];
    let (doc, _, table) = document(
        vec![grows(3.0), grows(3.0)],
        vec![130.0; 3],
        vec![vec![one, two, three], vec![one, one, one]],
    );
    let layout = tables::layout(&doc, &table, 0).unwrap();
    let steps = steps(&layout);
    near(steps[0], 4.0 + ASCENT + 2.0 * LEADING + 4.0, "three lines");
    near(steps[1], 4.0 + ASCENT + 4.0, "one line");
    // Grid lines sit half the outer stroke inside the table's edge.
    near(layout.x[0], 0.5, "left grid line");
    near(layout.y[0], 0.5, "top grid line");
    near(layout.width, 391.0, "width");
    near(layout.height, steps[0] + steps[1] + 1.0, "height");
}

#[test]
fn fixed_rows_floors_and_caps() {
    let one: &[&str] = &["one"];
    let three: &[&str] = &["one", "two", "three"];
    let fixed = TableRow {
        height: 15.0,
        minimum: 3.0,
        maximum: None,
        auto_grow: false,
        keep_with_next: false,
    };
    let floored = grows(40.0);
    let capped = TableRow {
        maximum: Some(25.0),
        ..grows(3.0)
    };
    let (doc, _, table) = document(
        vec![fixed, floored, capped],
        vec![200.0],
        vec![vec![three], vec![one], vec![three]],
    );
    let steps = steps(&tables::layout(&doc, &table, 0).unwrap());
    near(steps[0], 15.0, "fixed");
    near(steps[1], 40.0, "minimum");
    near(steps[2], 25.0, "maximum");
}

#[test]
fn a_cell_spanning_rows_lengthens_the_last_row_it_spans() {
    let one: &[&str] = &["one"];
    let five: &[&str] = &["1", "2", "3", "4", "5"];
    let (doc, _, mut table) = document(
        vec![grows(3.0), grows(3.0)],
        vec![100.0, 100.0],
        vec![vec![five, one], vec![one]],
    );
    // The first cell spans both rows; the second row's only other cell is
    // the right one.
    table.cells[0].rows = 2;
    table.cells[2].column = 1;
    assert!(table.valid());
    let steps = steps(&tables::layout(&doc, &table, 0).unwrap());
    near(steps[0], 4.0 + ASCENT + 4.0, "first row");
    near(
        steps[0] + steps[1],
        4.0 + ASCENT + 4.0 * LEADING + 4.0,
        "both rows",
    );
}

#[test]
fn invalid_tables_are_refused() {
    let one: &[&str] = &["one"];
    let (doc, _, table) = document(vec![grows(3.0)], vec![100.0, 100.0], vec![vec![one, one]]);
    let mut overlapping = table.clone();
    overlapping.cells[0].columns = 2;
    let mut outside = table.clone();
    outside.cells[1].column = 2;
    let mut negative = table.clone();
    negative.columns[0] = -1.0;
    for broken in [overlapping, outside, negative] {
        assert!(!broken.valid());
        assert!(tables::layout(&doc, &broken, 0).is_none());
    }
}

fn placed(doc: &LayoutDocument, frame: ObjectId) -> Vec<PlacedObject> {
    let lines: Vec<_> = compose_story(doc, StoryId(0)).lines().cloned().collect();
    anchored::placements(doc, &doc.stories[0], doc.object(frame).unwrap(), &lines)
}

/// InDesign's PDF of the public `tables` sample: the table's outer stroke
/// edge at the frame's top left, horizontal edges reaching the outer edge,
/// vertical edges stopping at the horizontal strokes, cell text inset from
/// the grid line.
#[test]
fn a_table_is_set_at_its_lines_top_left_and_draws_its_grid() {
    let one: &[&str] = &["A1"];
    let (doc, frame, table) = document(
        vec![grows(28.0), grows(28.0)],
        vec![120.0, 120.0],
        vec![vec![one, one], vec![one, one]],
    );
    let flow = compose_story(&doc, StoryId(0));
    assert_eq!(flow.frames[0].unrendered_structures, 0);
    let objects = placed(&doc, frame);
    let texts: Vec<_> = objects
        .iter()
        .filter(|o| matches!(o.object, LayoutObject::TextFrame { .. }))
        .collect();
    assert_eq!(texts.len(), 4);
    let first = texts.iter().find(|o| o.name == "0:0").unwrap();
    near(first.bounds.x, FRAME.x + 0.5, "cell x");
    near(first.bounds.y, FRAME.y + 0.5, "cell y");
    near(first.bounds.width, 120.0, "cell width");
    // The row, and room below it for the last line's descent.
    assert!(
        first.bounds.height > 28.0 && first.bounds.height < 28.0 + 6.0,
        "{:?}",
        first.bounds
    );
    let lines: Vec<_> = objects
        .iter()
        .filter(|o| {
            matches!(
                o.object,
                LayoutObject::Shape {
                    stroke: Some(_),
                    ..
                }
            )
        })
        .collect();
    // Three horizontal grid lines in two segments, three vertical in two.
    let horizontal: Vec<_> = lines.iter().filter(|o| o.bounds.height == 0.0).collect();
    let vertical: Vec<_> = lines.iter().filter(|o| o.bounds.width == 0.0).collect();
    assert_eq!((horizontal.len(), vertical.len()), (6, 6));
    let top_left = horizontal
        .iter()
        .find(|o| (o.bounds.y - (FRAME.y + 0.5)).abs() < 0.01 && o.bounds.x < FRAME.x + 1.0)
        .unwrap();
    near(top_left.bounds.x, FRAME.x, "outer edge");
    near(top_left.bounds.width, 120.5, "first top segment");
    let left = vertical
        .iter()
        .find(|o| (o.bounds.x - (FRAME.x + 0.5)).abs() < 0.01 && o.bounds.y < FRAME.y + 10.0)
        .unwrap();
    near(
        left.bounds.y,
        FRAME.y + 1.0,
        "vertical starts below the stroke",
    );
    near(
        left.bounds.height,
        27.0,
        "vertical stops at the next stroke",
    );
    // The cell's text: first baseline at the grid line, inset and ascent.
    let composed = schist_layout::compose::compose_object(&doc, first).unwrap();
    let line = composed.lines.first().unwrap();
    near(line.baseline, FRAME.y + 0.5 + 4.0 + ASCENT, "cell baseline");
    let _ = table;
}

#[test]
fn fills_and_justification_place_paint_and_text() {
    let one: &[&str] = &["text"];
    let (mut doc, frame, _) = document(vec![grows(60.0)], vec![100.0, 100.0], vec![vec![one, one]]);
    let host = doc.stories[0].structures[0].table.as_mut().unwrap();
    host.cells[0].fill = Some(CellPaint {
        ink: Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]),
        tint: 0.5,
    });
    host.cells[1].justification = CellJustification::Bottom;
    let objects = placed(&doc, frame);
    let fill = objects
        .iter()
        .find(|o| matches!(o.object, LayoutObject::Shape { fill: Some(_), .. }))
        .unwrap();
    near(fill.bounds.width, 100.0, "fill width");
    near(fill.bounds.height, 60.0, "fill height");
    let LayoutObject::Shape { tints, .. } = &fill.object else {
        unreachable!()
    };
    near(tints.fill, 0.5, "tint");
    // The bottom-justified cell's text frame starts the spare room lower.
    let bottom = objects.iter().find(|o| o.name == "1:0").unwrap();
    let spare = 60.0 - (4.0 + ASCENT + 4.0);
    near(bottom.bounds.y, FRAME.y + 0.5 + spare, "justified top");
}

#[test]
fn a_table_breaks_between_rows_and_what_no_frame_holds_is_overset() {
    let one: &[&str] = &["one"];
    let fixed = |height| TableRow {
        height,
        minimum: 3.0,
        maximum: None,
        auto_grow: false,
        keep_with_next: false,
    };
    // Three 40 pt rows: 121 pt with the outer strokes.
    let (mut doc, frame, _) = document(
        vec![fixed(40.0), fixed(40.0), fixed(40.0)],
        vec![100.0],
        vec![vec![one], vec![one], vec![one]],
    );
    let short = |doc: &mut LayoutDocument, height| {
        doc.objects
            .iter_mut()
            .find(|o| o.id == frame)
            .unwrap()
            .bounds
            .height = height;
    };
    short(&mut doc, 100.0);
    let flow = compose_story(&doc, StoryId(0));
    assert!(flow.frames[0].lost, "taller than its frame");
    // Two rows fit: two cell texts, three horizontal edges, four vertical.
    assert_eq!(placed(&doc, frame).len(), 2 + 3 + 4);
    short(&mut doc, 130.0);
    let flow = compose_story(&doc, StoryId(0));
    assert!(!flow.frames[0].lost);
    // Three cell texts, four horizontal edges, six vertical edges.
    assert_eq!(placed(&doc, frame).len(), 3 + 4 + 6);
}

/// Native review: a row as tall as its last baseline left no room for that
/// line's descent, so cells lost their last line.
#[test]
fn every_line_of_a_growing_row_is_drawn() {
    let one: &[&str] = &["Header"];
    let three: &[&str] = &["three", "short", "lines"];
    let (doc, frame, _) = document(
        vec![grows(3.0), grows(3.0)],
        vec![140.0, 140.0],
        vec![vec![one, one], vec![one, three]],
    );
    for object in placed(&doc, frame) {
        if !matches!(object.object, LayoutObject::TextFrame { .. }) {
            continue;
        }
        let composed = schist_layout::compose::compose_object(&doc, &object).unwrap();
        assert!(!composed.lost, "{}", object.name);
        let LayoutObject::TextFrame { story, .. } = object.object else {
            unreachable!()
        };
        let shown: usize = composed.all_lines().map(|l| l.end - l.start).sum();
        let text = doc.stories[story.0 as usize].text();
        assert!(
            shown + 2 >= text.len(),
            "{}: {shown} of {}",
            object.name,
            text.len()
        );
    }
}
