//! Tables set in text. IDML keeps a table inside its story at a character
//! position; its XML is retained for saving, and it is typed here for
//! composition and drawing. Each cell's text is a story of its own.
//!
//! A table is set on lines of its own, one [`Part`] per line: its outer
//! stroke edge at the line's top left. Rows that grow take the height their
//! cells' text needs: the top inset, the last line's baseline below the cell's
//! content top, and the bottom inset. A table that does not fit breaks
//! between whole rows into parts, each repeating the header and footer rows
//! as the table asks (see `table_flow`). Table and cell styles are not
//! composed yet. The rules and their evidence are in `docs/idml-format.md`.
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

    /// Finite, positive sizes and cells that tile part of the grid without
    /// overlapping or leaving it.
    pub fn valid(&self) -> bool {
        let finite = |v: Pt| v.is_finite() && v >= 0.0;
        if self.rows.is_empty()
            || self.columns.is_empty()
            || self.header_rows + self.footer_rows > self.rows.len()
            || !self.columns.iter().all(|w| finite(*w) && *w > 0.0)
            || !self
                .rows
                .iter()
                .all(|r| finite(r.height) && finite(r.minimum) && r.maximum.is_none_or(finite))
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

/// What the rows of `grid` draw with its outer top left at `origin`, in the
/// frame's space: cell fills, then cell edges, then cell text. Horizontal
/// edges run the whole cell and reach the outer stroke edge at the table's
/// sides; vertical edges stop at the horizontal strokes, as InDesign's PDF of
/// the public paged-media `tables` sample draws them. A part's last row
/// draws its bottom edges as the table's last row does.
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
    let drawn = |edge: &CellEdge| edge.weight > 0.0 && edge.paint.is_some();
    let mut fills = Vec::new();
    let mut verticals = Vec::new();
    let mut horizontals = Vec::new();
    let mut texts = Vec::new();
    let last_row = grid.rows.len();
    let last_column = table.columns.len();
    // The horizontal stroke weight along a grid line at a column.
    let horizontal_at = |line: usize, column: usize| -> Pt {
        table
            .cells
            .iter()
            .zip(&spans)
            .filter(|(c, _)| column >= c.column && column < c.column + c.columns)
            .filter_map(|(c, span)| {
                let (first, end) = (*span)?;
                if first == line {
                    Some(c.edges[TOP].weight)
                } else if end == line {
                    Some(c.edges[BOTTOM].weight)
                } else {
                    None
                }
            })
            .fold(0.0, Pt::max)
    };
    for (index, cell) in table.cells.iter().enumerate() {
        let Some((first, end)) = spans[index] else {
            continue;
        };
        let rect = rect_of(cell, (first, end));
        if let Some(fill) = &cell.fill {
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
        let mut horizontal = |y: Pt, edge: &CellEdge| {
            if !drawn(edge) {
                return;
            }
            let left = if cell.column == 0 {
                rect.x - layout.x[0]
            } else {
                rect.x
            };
            let right = if cell.column + cell.columns == last_column {
                rect.right() + (layout.width - layout.x[last_column])
            } else {
                rect.right()
            };
            horizontals.push(shape(
                crate::authoring::path_for(crate::authoring::ShapeKind::Line, right - left, 0.0),
                Rect::new(left, y, right - left, 0.0),
                None,
                Some((edge.paint.as_ref().unwrap(), edge.weight)),
                page,
            ));
        };
        horizontal(rect.y, &cell.edges[TOP]);
        if end == last_row {
            horizontal(rect.bottom(), &cell.edges[BOTTOM]);
        }
        let mut vertical = |x: Pt, edge: &CellEdge| {
            if !drawn(edge) {
                return;
            }
            let column = cell.column;
            let top = rect.y + horizontal_at(first, column) / 2.0;
            let bottom = rect.bottom() - horizontal_at(end, column) / 2.0;
            if bottom > top {
                verticals.push(shape(
                    crate::authoring::path_for(
                        crate::authoring::ShapeKind::Line,
                        0.0,
                        bottom - top,
                    ),
                    Rect::new(x, top, 0.0, bottom - top),
                    None,
                    Some((edge.paint.as_ref().unwrap(), edge.weight)),
                    page,
                ));
            }
        };
        vertical(rect.x, &cell.edges[LEFT]);
        if cell.column + cell.columns == last_column {
            vertical(rect.right(), &cell.edges[RIGHT]);
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
    fills
        .into_iter()
        .chain(verticals)
        .chain(horizontals)
        .chain(texts)
        .collect()
}
