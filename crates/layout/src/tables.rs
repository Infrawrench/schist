//! Tables set in text. IDML keeps a table inside its story at a character
//! position; its XML is retained for saving, and it is typed here for
//! composition and drawing. Each cell's text is a story of its own.
//!
//! A table is set on lines of its own, one [`Part`] per line: its outer
//! stroke edge at the line's top left, its space before above that line
//! unless the line starts a column, its space after below it. Rows that grow
//! take the height their cells' text needs: the top inset, the last line's
//! baseline below the cell's content top, and the bottom inset. A table that
//! does not fit breaks between whole rows into parts, each repeating the
//! header and footer rows as the table asks (see `table_flow`). Fills
//! alternate by row or column as the table or its style asks; cell styles,
//! the table's border and its alternating strokes are resolved into the
//! cells' edges when a table is read, and a grid line two cells share draws
//! one of their edges (see [`objects`]). The rules and their evidence are in
//! `docs/idml-format.md`.
use crate::{Ink, LayoutDocument, ObjectId, PlacedObject, Pt, Rect, StoryId};
use serde::{Deserialize, Serialize};

/// A typed table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub header_rows: usize,
    pub footer_rows: usize,
    pub rows: Vec<TableRow>,
    /// Column widths in points.
    pub columns: Vec<Pt>,
    pub cells: Vec<TableCell>,
    /// Where header rows repeat when the table breaks (BreakHeaders).
    #[serde(default)]
    pub header_repeat: RepeatRows,
    /// Where footer rows repeat (BreakFooters).
    #[serde(default)]
    pub footer_repeat: RepeatRows,
    /// The first part shows no header rows (SkipFirstHeader).
    #[serde(default)]
    pub skip_first_header: bool,
    /// The last part shows no footer rows (SkipLastFooter).
    #[serde(default)]
    pub skip_last_footer: bool,
    /// Alternating fills of the body rows, from the table or its style.
    #[serde(default)]
    pub row_fills: Option<Alternation>,
    /// Alternating fills of the columns, header and footer rows included.
    #[serde(default)]
    pub column_fills: Option<Alternation>,
    /// Space above the table (SpaceBefore), kept only below a line of its
    /// column: a table starting a column, a frame or a cell starts at its top.
    #[serde(default)]
    pub space_before: Pt,
    /// Space below the table (SpaceAfter), between it and the line after it
    /// in its column.
    #[serde(default)]
    pub space_after: Pt,
    /// Which strokes are in front where row and column strokes cross.
    #[serde(default)]
    pub stroke_order: StrokeOrder,
}

/// IDML's StrokeOrderTypes: which strokes are in front where row and column
/// strokes cross.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeOrder {
    /// Row strokes in front where the colours differ (BestJoins).
    #[default]
    BestJoins,
    /// Row strokes in front (RowOnTop).
    RowOnTop,
    /// Column strokes in front (ColumnOnTop).
    ColumnOnTop,
    /// Row strokes in front where the colours differ
    /// (Indesign2Compatibility).
    InDesign2Compatibility,
}

impl StrokeOrder {
    /// Whether column strokes run through the crossings, row strokes
    /// stopping at them. Solid strokes of one colour look the same either
    /// way, so only ColumnOnTop puts columns in front.
    pub fn columns_in_front(self) -> bool {
        self == Self::ColumnOnTop
    }
}

/// An alternating pattern of fills: the first `first` rows or columns take
/// `first_paint`, the next `next` take `next_paint`, over and over, after
/// the first `skip_first` and before the last `skip_last`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alternation {
    pub first: usize,
    pub first_paint: Option<CellPaint>,
    pub next: usize,
    pub next_paint: Option<CellPaint>,
    pub skip_first: usize,
    pub skip_last: usize,
}

impl Alternation {
    /// The paint of the row or column at `index` of `count`.
    pub fn paint(&self, index: usize, count: usize) -> Option<&CellPaint> {
        let cycle = self.first + self.next;
        if cycle == 0 || index < self.skip_first || index + self.skip_last >= count {
            return None;
        }
        if (index - self.skip_first) % cycle < self.first {
            self.first_paint.as_ref()
        } else {
            self.next_paint.as_ref()
        }
    }
}

/// Where repeated header or footer rows appear: IDML's
/// HeaderFooterBreakTypes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RepeatRows {
    /// In every text column the table reaches (InAllTextColumns).
    #[default]
    EveryColumn,
    /// Once in each text frame (OncePerTextFrame).
    OncePerFrame,
    /// Once on each page (OncePerPage).
    OncePerPage,
}

/// The rows one part of a broken table shows: its header rows or not, a run
/// of body rows, and its footer rows or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub header: bool,
    /// Body rows, as table row indices.
    pub start: usize,
    pub end: usize,
    pub footer: bool,
}

/// The parts each table of a story is broken into, by structure index. A
/// table not listed is set whole.
pub type Parts = std::collections::BTreeMap<usize, Vec<Part>>;

/// A table part set in a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetPart {
    /// The box's position in the line's spec.
    pub at: usize,
    /// The story structure holding the table.
    pub structure: usize,
    /// Which of the table's parts this is, and its rows.
    pub index: usize,
    pub part: Part,
}

/// A row's sizing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TableRow {
    /// SingleRowHeight: the height of a row that does not grow.
    pub height: Pt,
    /// MinimumHeight: the floor of a row that grows.
    pub minimum: Pt,
    /// MaximumHeight, when stated.
    pub maximum: Option<Pt>,
    /// AutoGrow: the row takes the height its cells' text needs.
    pub auto_grow: bool,
    /// KeepWithNextRow: the table does not break after this row.
    #[serde(default)]
    pub keep_with_next: bool,
    /// StartRow, when not Anywhere: the row starts at the top of the next
    /// column, frame or page (see [`Table::start`]).
    #[serde(default)]
    pub start: Option<crate::styles::ParagraphStart>,
}

/// Paint on a cell or one of its edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellPaint {
    pub ink: Ink,
    /// 0 to 1.
    pub tint: f32,
}

/// One edge of a cell. A weight of zero or no paint draws nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellEdge {
    pub weight: Pt,
    pub paint: Option<CellPaint>,
    /// What states the edge, which decides the stroke of a grid line two
    /// cells share.
    #[serde(default)]
    pub source: EdgeSource,
}

/// What states a cell's edge, weakest first: where two cells share a grid
/// line, the edge from the stronger source is drawn, the lower or right
/// cell's when they tie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum EdgeSource {
    /// Nothing: InDesign's 1 pt black.
    #[default]
    Default,
    /// The table or its style: its border on the table's outside, else its
    /// alternating row or column strokes.
    Table,
    /// The cell or one of its cell styles.
    Cell,
}

/// Where a cell's text sits when the cell is taller than it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CellJustification {
    #[default]
    Top,
    Center,
    Bottom,
}

/// A cell: its grid position and span, its story, paint and insets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub column: usize,
    pub row: usize,
    pub columns: usize,
    pub rows: usize,
    pub story: StoryId,
    pub fill: Option<CellPaint>,
    /// The cell or its cell style decides its fill, none included, so the
    /// table's alternating fills leave it.
    #[serde(default)]
    pub own_fill: bool,
    pub insets: crate::Insets,
    pub justification: CellJustification,
    /// Top, left, bottom and right.
    pub edges: [CellEdge; 4],
}

const TOP: usize = 0;
const LEFT: usize = 1;
const BOTTOM: usize = 2;
const RIGHT: usize = 3;

impl Table {
    /// What fills `cell`: its own fill, or else the table's alternating
    /// fill for its first row among the body rows (header and footer rows
    /// have none) or for its first column.
    pub fn fill<'a>(&'a self, cell: &'a TableCell) -> Option<&'a CellPaint> {
        if cell.own_fill || cell.fill.is_some() {
            return cell.fill.as_ref();
        }
        let body = self.body();
        let rows = self
            .row_fills
            .as_ref()
            .filter(|_| body.contains(&cell.row))
            .and_then(|fills| fills.paint(cell.row - body.start, body.len()));
        rows.or_else(|| {
            self.column_fills
                .as_ref()
                .and_then(|fills| fills.paint(cell.column, self.columns.len()))
        })
    }

    /// The body rows: those between the header and footer rows.
    pub fn body(&self) -> std::ops::Range<usize> {
        self.header_rows
            ..self
                .rows
                .len()
                .saturating_sub(self.footer_rows)
                .max(self.header_rows)
    }

    /// The table unbroken: every row, less a header or footer its first or
    /// last occurrence skips.
    pub fn whole(&self) -> Part {
        let body = self.body();
        Part {
            header: self.header_rows > 0 && !self.skip_first_header,
            start: body.start,
            end: body.end,
            footer: self.footer_rows > 0 && !self.skip_last_footer,
        }
    }

    /// The rows `part` shows, in order.
    pub fn shown(&self, part: &Part) -> Vec<usize> {
        let footer = self.body().end..self.rows.len();
        (0..self.header_rows)
            .filter(|_| part.header)
            .chain(part.start..part.end)
            .chain(footer.filter(|_| part.footer))
            .collect()
    }

    /// Whether a cell spans the grid line above `row`, so the table cannot
    /// break there.
    pub fn joined(&self, row: usize) -> bool {
        self.cells
            .iter()
            .any(|c| c.row < row && row < c.row + c.rows)
    }

    /// Where a part whose first body row is `row` must begin, by the row's
    /// StartRow, as the specification describes it: at the top of the next
    /// column, frame, page, odd or even page. The table's first part begins
    /// as its first row asks, or its first body row under header rows; a
    /// later body row a cell spans into begins anywhere, as the table cannot
    /// break above it. Header and footer rows below the first row have no
    /// place to begin.
    pub fn start(&self, row: usize) -> Option<crate::styles::ParagraphStart> {
        let body = self.body();
        let policy = if row == body.start {
            self.rows.first()?.start.or(self.rows.get(row)?.start)
        } else if body.contains(&row) && !self.joined(row) {
            self.rows[row].start
        } else {
            None
        };
        policy.filter(|p| *p != crate::styles::ParagraphStart::Anywhere)
    }

    /// Finite, positive sizes and cells that tile part of the grid without
    /// overlapping or leaving it.
    pub fn valid(&self) -> bool {
        let finite = |v: Pt| v.is_finite() && v >= 0.0;
        if self.rows.is_empty()
            || self.columns.is_empty()
            || !self.space_before.is_finite()
            || !self.space_after.is_finite()
            || self.header_rows + self.footer_rows > self.rows.len()
            || !self.columns.iter().all(|w| finite(*w) && *w > 0.0)
            || !self
                .rows
                .iter()
                .all(|r| finite(r.height) && finite(r.minimum) && r.maximum.is_none_or(finite))
            || !self
                .row_fills
                .iter()
                .chain(&self.column_fills)
                .flat_map(|a| a.first_paint.iter().chain(&a.next_paint))
                .all(|p| (0.0..=1.0).contains(&p.tint))
        {
            return false;
        }
        let mut taken = vec![false; self.rows.len() * self.columns.len()];
        self.cells.iter().all(|cell| {
            let inside = cell.rows > 0
                && cell.columns > 0
                && cell.row + cell.rows <= self.rows.len()
                && cell.column + cell.columns <= self.columns.len();
            let sane = [
                cell.insets.top,
                cell.insets.left,
                cell.insets.bottom,
                cell.insets.right,
            ]
            .into_iter()
            .all(finite)
                && cell.edges.iter().all(|e| finite(e.weight))
                && cell
                    .fill
                    .iter()
                    .chain(cell.edges.iter().filter_map(|e| e.paint.as_ref()))
                    .all(|p| (0.0..=1.0).contains(&p.tint));
            inside
                && sane
                && (cell.row..cell.row + cell.rows).all(|r| {
                    (cell.column..cell.column + cell.columns).all(|c| {
                        let slot = &mut taken[r * self.columns.len() + c];
                        !std::mem::replace(slot, true)
                    })
                })
        })
    }
}

/// A table's grid, relative to its outer top left.
#[derive(Debug, Clone, PartialEq)]
pub struct TableLayout {
    /// Grid line positions: columns + 1 of them, at stroke centers.
    pub x: Vec<Pt>,
    /// Rows + 1 of them.
    pub y: Vec<Pt>,
    /// Outer width and height, strokes included.
    pub width: Pt,
    pub height: Pt,
    /// The height each cell's text needs below its top inset (the last
    /// baseline), by cell index.
    content: Vec<Pt>,
    /// How far each cell's last line reaches below its baseline: the row
    /// does not count it, so the cell's text frame is given that room.
    overhang: Vec<Pt>,
    /// By row: the heaviest top edge of the cells starting there, and the
    /// heaviest bottom edge of the cells ending there.
    top_weight: Vec<Pt>,
    bottom_weight: Vec<Pt>,
}

/// The grid of one part, relative to its outer top left.
#[derive(Debug, Clone, PartialEq)]
pub struct PartGrid {
    /// The table rows shown, in order.
    pub rows: Vec<usize>,
    /// Grid line positions: rows + 1 of them.
    pub y: Vec<Pt>,
    /// Outer height, strokes included.
    pub height: Pt,
}

impl TableLayout {
    /// Row `row`'s height.
    pub fn row_height(&self, row: usize) -> Pt {
        self.y[row + 1] - self.y[row]
    }

    /// The outer height of the rows `part` shows, half its outer strokes
    /// included.
    pub fn part_height(&self, table: &Table, part: &Part) -> Pt {
        let body = table.body();
        let last = table.rows.len();
        let runs = [
            (0, table.header_rows, part.header),
            (part.start, part.end, true),
            (body.end, last, part.footer),
        ];
        let mut height = 0.0;
        let mut ends = None;
        for (start, end, shown) in runs {
            if !shown || start >= end {
                continue;
            }
            height += self.y[end] - self.y[start];
            let first = ends.map_or(start, |(first, _)| first);
            ends = Some((first, end - 1));
        }
        ends.map_or(0.0, |(first, last)| {
            self.top_weight[first] / 2.0 + height + self.bottom_weight[last] / 2.0
        })
    }

    /// The grid `part` draws: its rows one after another, its grid lines
    /// half their outer strokes inside its edges, as the whole table's are.
    pub fn part(&self, table: &Table, part: &Part) -> PartGrid {
        let rows = table.shown(part);
        let Some(first) = rows.first() else {
            return PartGrid {
                rows,
                y: vec![0.0],
                height: 0.0,
            };
        };
        let mut y = vec![self.top_weight[*first] / 2.0];
        for row in &rows {
            y.push(y.last().unwrap() + self.row_height(*row));
        }
        PartGrid {
            rows,
            y,
            height: self.part_height(table, part),
        }
    }
}

/// The id of a cell's text frame while it is measured or drawn; no document
/// object has it.
const CELL: ObjectId = ObjectId(u32::MAX - 1);

/// A cell's text frame: the story in `bounds`, inset by the cell's insets.
fn cell_frame(cell: &TableCell, bounds: Rect, page: usize) -> PlacedObject {
    PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: CELL,
        page,
        bounds,
        transform: crate::affine::Affine::IDENTITY,
        object: crate::LayoutObject::TextFrame {
            story: cell.story,
            footnotes: Default::default(),
            balance_columns: None,
            text_path: None,
            columns: 1,
            gutter: 0.0,
            insets: cell.insets,
            overflow: crate::FrameOverflow::Clip,
        },
        rotation: 0.0,
        // IDML's own cell name, "column:row", for Preflight.
        name: format!("{}:{}", cell.column, cell.row),
        locked: false,
        overprint: false,
        transparency: 1.0,
    }
}

/// How far below the cell's top inset its last baseline falls, and how far
/// its last line reaches below that baseline, composed at `width` without a
/// height limit.
fn content_height(
    doc: &LayoutDocument,
    cell: &TableCell,
    width: Pt,
    page: usize,
) -> Option<(Pt, Pt)> {
    let mut measured = cell.clone();
    measured.insets.left = 0.0;
    measured.insets.right = 0.0;
    let frame = cell_frame(&measured, Rect::new(0.0, 0.0, width, 100_000.0), page);
    let composed = crate::compose::compose_object(doc, &frame)?;
    let last = composed.all_lines().filter(|l| !l.is_generated()).last()?;
    Some((
        (last.baseline - cell.insets.top).max(0.0),
        (last.bounds.bottom() - last.baseline).max(0.0),
    ))
}

/// The table's grid: rows that grow take their cells' text, a cell spanning
/// rows lengthens the last of them that grows, and maximums cap growth.
pub fn layout(doc: &LayoutDocument, table: &Table, page: usize) -> Option<TableLayout> {
    if !table.valid() {
        return None;
    }
    let span_width = |cell: &TableCell| -> Pt {
        table.columns[cell.column..cell.column + cell.columns]
            .iter()
            .sum()
    };
    let mut content = Vec::with_capacity(table.cells.len());
    let mut overhang = Vec::with_capacity(table.cells.len());
    for cell in &table.cells {
        let width = span_width(cell) - cell.insets.left - cell.insets.right;
        let (needed, below) = if width > 0.0 {
            content_height(doc, cell, width, page)?
        } else {
            (0.0, 0.0)
        };
        content.push(needed);
        overhang.push(below);
    }
    let mut heights: Vec<Pt> = table
        .rows
        .iter()
        .map(|r| if r.auto_grow { r.minimum } else { r.height })
        .collect();
    let need = |index: usize| {
        let cell = &table.cells[index];
        cell.insets.top + content[index] + cell.insets.bottom
    };
    for (index, cell) in table.cells.iter().enumerate() {
        if cell.rows == 1 && table.rows[cell.row].auto_grow {
            heights[cell.row] = heights[cell.row].max(need(index));
        }
    }
    for (index, cell) in table.cells.iter().enumerate() {
        let span = cell.row..cell.row + cell.rows;
        if cell.rows > 1 {
            let deficit = need(index) - heights[span.clone()].iter().sum::<Pt>();
            if let Some(last) = span.rev().find(|r| table.rows[*r].auto_grow) {
                heights[last] += deficit.max(0.0);
            }
        }
    }
    for (height, row) in heights.iter_mut().zip(&table.rows) {
        if let Some(maximum) = row.maximum.filter(|_| row.auto_grow) {
            *height = height.min(maximum);
        }
    }
    let outer = |side: usize, on: &dyn Fn(&TableCell) -> bool| {
        table
            .cells
            .iter()
            .filter(|c| on(c))
            .map(|c| c.edges[side].weight)
            .fold(0.0, Pt::max)
    };
    let last_row = table.rows.len();
    let last_column = table.columns.len();
    let top = outer(TOP, &|c| c.row == 0);
    let left = outer(LEFT, &|c| c.column == 0);
    let bottom = outer(BOTTOM, &|c| c.row + c.rows == last_row);
    let right = outer(RIGHT, &|c| c.column + c.columns == last_column);
    let mut x = vec![left / 2.0];
    for width in &table.columns {
        x.push(x.last().unwrap() + width);
    }
    let mut y = vec![top / 2.0];
    for height in &heights {
        y.push(y.last().unwrap() + height);
    }
    let mut top_weight = vec![0.0; last_row];
    let mut bottom_weight = vec![0.0; last_row];
    for cell in &table.cells {
        let end = cell.row + cell.rows - 1;
        top_weight[cell.row] = Pt::max(top_weight[cell.row], cell.edges[TOP].weight);
        bottom_weight[end] = Pt::max(bottom_weight[end], cell.edges[BOTTOM].weight);
    }
    Some(TableLayout {
        width: x.last().unwrap() + right / 2.0,
        height: y.last().unwrap() + bottom / 2.0,
        x,
        y,
        content,
        overhang,
        top_weight,
        bottom_weight,
    })
}

/// Whether composing `table` would compose story `host` again: one of its
/// cells is `host`, or anchors or holds, at any depth, something that leads
/// there.
pub fn reaches(doc: &LayoutDocument, table: &Table, host: StoryId) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    let mut pending: Vec<StoryId> = table.cells.iter().map(|c| c.story).collect();
    while let Some(id) = pending.pop() {
        if id == host {
            return true;
        }
        if !seen.insert(id) {
            continue;
        }
        let Some(story) = doc.story(id) else {
            continue;
        };
        for structure in &story.structures {
            if let Some(table) = &structure.table {
                pending.extend(table.cells.iter().map(|c| c.story));
            }
            if let Some(crate::LayoutObject::TextFrame { story, .. }) =
                structure.anchored.as_ref().map(|a| &a.object.object)
            {
                pending.push(*story);
            }
        }
    }
    false
}

/// A filled rectangle or a stroked line as a page item.
fn shape(
    path: crate::ShapePath,
    bounds: Rect,
    fill: Option<&CellPaint>,
    stroke: Option<(&CellPaint, Pt)>,
    page: usize,
) -> PlacedObject {
    PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: CELL,
        page,
        bounds,
        transform: crate::affine::Affine::IDENTITY,
        object: crate::LayoutObject::Shape {
            path,
            fill: fill.map(|p| p.ink.clone()),
            stroke: stroke.map(|(p, _)| p.ink.clone()),
            stroke_width: stroke.map_or(0.0, |(_, w)| w),
            fill_overprint: false,
            stroke_overprint: false,
            tints: crate::PaintTints {
                fill: fill.map_or(1.0, |p| p.tint),
                stroke: stroke.map_or(1.0, |(p, _)| p.tint),
            },
        },
        rotation: 0.0,
        name: String::new(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    }
}

/// A stroke along a grid line between two crossings: where the line lies
/// across, and where the stroke starts and ends along it.
struct Segment<'a> {
    edge: &'a CellEdge,
    across: Pt,
    from: Pt,
    to: Pt,
}

/// Whether two strokes look the same.
fn same(a: &CellEdge, b: &CellEdge) -> bool {
    a.weight == b.weight && a.paint == b.paint
}

/// The strokes in front, which run through the crossings. `lines[line][k]`
/// is what the grid line at `at[line]` draws over the `k`th cell along it,
/// between the crossings at `span[k]` and `span[k + 1]`. A stroke reaches
/// the table's outer edge (0 and `outer`) at the table's sides; at a crossing
/// it meets the same stroke flush, and runs on by half its weight where
/// another stroke or none continues the line.
fn in_front<'a>(
    lines: &[Vec<Option<&'a CellEdge>>],
    at: &[Pt],
    span: &[Pt],
    outer: Pt,
) -> Vec<Segment<'a>> {
    let mut out = Vec::new();
    for (line, strokes) in lines.iter().enumerate() {
        for (k, edge) in strokes.iter().enumerate() {
            let Some(edge) = *edge else {
                continue;
            };
            let past = |next: Option<&Option<&CellEdge>>| match next.copied().flatten() {
                Some(next) if same(next, edge) => 0.0,
                _ => edge.weight / 2.0,
            };
            let from = if k == 0 {
                0.0
            } else {
                span[k] - past(strokes.get(k - 1))
            };
            let to = if k + 1 == strokes.len() {
                outer
            } else {
                span[k + 1] + past(strokes.get(k + 1))
            };
            out.push(Segment {
                edge,
                across: at[line],
                from,
                to,
            });
        }
    }
    out
}

/// The strokes behind, which stop at the strokes in front crossing them:
/// half the weight short of the crossing of the same stroke there, else of
/// the heaviest, and at the crossing when none crosses. `front[k]` is the
/// front line at `span[k]`, by the cells along it.
fn behind<'a>(
    lines: &[Vec<Option<&'a CellEdge>>],
    at: &[Pt],
    span: &[Pt],
    front: &[Vec<Option<&CellEdge>>],
) -> Vec<Segment<'a>> {
    let mut out = Vec::new();
    for (line, strokes) in lines.iter().enumerate() {
        // The front strokes either side of this line where front line `k`
        // crosses it.
        let cut = |k: usize, edge: &CellEdge| {
            let crossing: Vec<&CellEdge> = [line.checked_sub(1), Some(line)]
                .into_iter()
                .flatten()
                .filter_map(|cell| front[k].get(cell).copied().flatten())
                .collect();
            crossing
                .iter()
                .find(|c| same(c, edge))
                .or_else(|| crossing.iter().max_by(|a, b| a.weight.total_cmp(&b.weight)))
                .map_or(0.0, |c| c.weight / 2.0)
        };
        for (k, edge) in strokes.iter().enumerate() {
            let Some(edge) = *edge else {
                continue;
            };
            let from = span[k] + cut(k, edge);
            let to = span[k + 1] - cut(k + 1, edge);
            if to > from {
                out.push(Segment {
                    edge,
                    across: at[line],
                    from,
                    to,
                });
            }
        }
    }
    out
}

/// What the rows of `grid` draw with its outer top left at `origin`, in the
/// frame's space: cell fills, then strokes, then cell text. Strokes are drawn
/// by grid line and column or row, as InDesign's PDF of the public
/// paged-media `tables` sample draws them, a cell spanning columns included.
/// A grid line two cells share draws the edge of the stronger source (see
/// [`EdgeSource`]), the lower or right cell's when they tie, except that a
/// header row's bottom edge is drawn above the body and a footer row's top
/// edge below it. Page 5 of that PDF draws a 3 pt magenta cell's edges over
/// its neighbours' unstated 1 pt black ones on all four sides, and page 12
/// the header's black bottom edge, not the 2 pt magenta top edge the body
/// cell below it states; the footer follows the header (a Schist reading).
/// Row strokes are in front unless the table's StrokeOrder is ColumnOnTop:
/// they reach the table's outer stroke edge at its sides, and vertical
/// strokes stop at them. A part's last row draws its bottom edges as the
/// table's last row does.
pub fn objects(
    table: &Table,
    layout: &TableLayout,
    grid: &PartGrid,
    origin: crate::Point,
    page: usize,
) -> Vec<PlacedObject> {
    let (ox, oy) = (origin.x, origin.y);
    // Each shown cell's first and past-last grid line in the part.
    let mut line_of = vec![None; table.rows.len()];
    for (index, row) in grid.rows.iter().enumerate() {
        line_of[*row] = Some(index);
    }
    let spans: Vec<Option<(usize, usize)>> = table
        .cells
        .iter()
        .map(|cell| {
            let first = line_of[cell.row]?;
            let last = line_of[cell.row + cell.rows - 1]?;
            (last + 1 == first + cell.rows).then_some((first, last + 1))
        })
        .collect();
    let rect_of = |cell: &TableCell, (first, end): (usize, usize)| {
        let (x0, x1) = (layout.x[cell.column], layout.x[cell.column + cell.columns]);
        let (y0, y1) = (grid.y[first], grid.y[end]);
        Rect::new(ox + x0, oy + y0, x1 - x0, y1 - y0)
    };
    // The cell covering each shown row and column.
    let (rows, columns) = (grid.rows.len(), table.columns.len());
    let mut covers = vec![None; rows * columns];
    for (index, span) in spans.iter().enumerate() {
        let Some((first, end)) = *span else {
            continue;
        };
        let cell = &table.cells[index];
        for row in first..end {
            for column in cell.column..cell.column + cell.columns {
                covers[row * columns + column] = Some(index);
            }
        }
    }
    let cell_at = |row: usize, column: usize| -> Option<usize> {
        (row < rows && column < columns)
            .then(|| covers[row * columns + column])
            .flatten()
    };
    let body = table.body();
    let drawn = |edge: &CellEdge| edge.weight > 0.0 && edge.paint.is_some();
    // The stroke of a grid line between cells `before` and `after` (above
    // and below, or left and right), from their `sides` facing it.
    let shared = |before: Option<usize>, after: Option<usize>, sides: (usize, usize)| {
        let edge = match (before, after) {
            (Some(a), Some(b)) if a == b => return None,
            (Some(a), Some(b)) => {
                let (first, second) = (&table.cells[a], &table.cells[b]);
                let (ending, starting) = (&first.edges[sides.0], &second.edges[sides.1]);
                let between_rows = sides.0 == BOTTOM;
                if between_rows && first.row < table.header_rows && second.row >= table.header_rows
                {
                    ending
                } else if between_rows && second.row >= body.end && first.row < body.end {
                    starting
                } else if ending.source > starting.source {
                    ending
                } else {
                    starting
                }
            }
            (Some(a), None) => &table.cells[a].edges[sides.0],
            (None, Some(b)) => &table.cells[b].edges[sides.1],
            (None, None) => return None,
        };
        drawn(edge).then_some(edge)
    };
    let horizontal: Vec<Vec<Option<&CellEdge>>> = (0..=rows)
        .map(|line| {
            (0..columns)
                .map(|column| {
                    let above = line.checked_sub(1).and_then(|row| cell_at(row, column));
                    shared(above, cell_at(line, column), (BOTTOM, TOP))
                })
                .collect()
        })
        .collect();
    let vertical: Vec<Vec<Option<&CellEdge>>> = (0..=columns)
        .map(|line| {
            (0..rows)
                .map(|row| {
                    let left = line.checked_sub(1).and_then(|column| cell_at(row, column));
                    shared(left, cell_at(row, line), (RIGHT, LEFT))
                })
                .collect()
        })
        .collect();
    let columns_in_front = table.stroke_order.columns_in_front();
    let (mut front, back) = if columns_in_front {
        (
            in_front(&vertical, &layout.x, &grid.y, grid.height),
            behind(&horizontal, &grid.y, &layout.x, &vertical),
        )
    } else {
        (
            in_front(&horizontal, &grid.y, &layout.x, layout.width),
            behind(&vertical, &layout.x, &grid.y, &horizontal),
        )
    };
    // Where strokes in front overlap past a crossing, the heavier is drawn
    // over the lighter, as page 5 draws its 3 pt magenta edges last.
    front.sort_by(|a, b| a.edge.weight.total_cmp(&b.edge.weight));
    let stroke = |segment: &Segment, along_row: bool| {
        let length = segment.to - segment.from;
        let (bounds, path) = if along_row {
            (
                Rect::new(ox + segment.from, oy + segment.across, length, 0.0),
                crate::authoring::path_for(crate::authoring::ShapeKind::Line, length, 0.0),
            )
        } else {
            (
                Rect::new(ox + segment.across, oy + segment.from, 0.0, length),
                crate::authoring::path_for(crate::authoring::ShapeKind::Line, 0.0, length),
            )
        };
        let paint = segment
            .edge
            .paint
            .as_ref()
            .expect("drawn edges are painted");
        shape(path, bounds, None, Some((paint, segment.edge.weight)), page)
    };
    let strokes: Vec<PlacedObject> = back
        .iter()
        .map(|segment| stroke(segment, columns_in_front))
        .chain(
            front
                .iter()
                .map(|segment| stroke(segment, !columns_in_front)),
        )
        .collect();
    let mut fills = Vec::new();
    let mut texts = Vec::new();
    for (index, cell) in table.cells.iter().enumerate() {
        let Some((first, end)) = spans[index] else {
            continue;
        };
        let rect = rect_of(cell, (first, end));
        if let Some(fill) = table.fill(cell) {
            fills.push(shape(
                crate::authoring::path_for(
                    crate::authoring::ShapeKind::Rectangle,
                    rect.width,
                    rect.height,
                ),
                rect,
                Some(fill),
                None,
                page,
            ));
        }
        // Text below its top inset, moved down for center or bottom.
        let needed = cell.insets.top + layout.content[index] + cell.insets.bottom;
        let spare = (rect.height - needed).max(0.0);
        let shift = match cell.justification {
            CellJustification::Top => 0.0,
            CellJustification::Center => spare / 2.0,
            CellJustification::Bottom => spare,
        };
        // Room below the bottom inset for the last line's descent, which the
        // row does not count, so a line whose baseline fits is drawn.
        let mut bounds = rect;
        bounds.y += shift;
        bounds.height += layout.overhang[index] + 0.01 - shift;
        texts.push(cell_frame(cell, bounds, page));
    }
    fills.into_iter().chain(strokes).chain(texts).collect()
}
