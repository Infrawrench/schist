//! Grids: a document grid for text, and a baseline grid for lines.
//!
//! Page layout uses both, and they answer different questions.
//!
//! A **document grid** divides the page into columns and rows, and the
//! margin settings place text in it. It is where a text frame gets its
//! position, and it is what makes a two-page spread line up.
//!
//! A **baseline grid** is a rhythm of horizontal lines at a fixed
//! interval. Body text set *on* the grid has every baseline on a line,
//! so text flowing between frames across pages keeps its rhythm -- which
//! is the entire reason multi-page body copy looks like one document
//! rather than a pile of pages. Text set *to* the grid is stretched or
//! the leading is adjusted to fit the interval.
//!
//! Nothing here rasterises anything. A grid is geometry and a set of
//! guide positions, and the editor draws guides from it.

use serde::{Deserialize, Serialize};

use crate::geometry::{Insets, Page, Pt, Rect};
use crate::model::LayoutDocument;

/// How text is aligned to a baseline grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GridMode {
    /// Off. The grid is guides only, and leading is whatever the style
    /// says.
    #[default]
    None,
    /// Move each baseline forward onto a guide, retaining the style's
    /// minimum line advance. A large font may span several grid intervals.
    SnapToGrid,
    /// Round each line's advance up to whole grid intervals, then align
    /// its baseline. The page's guide positions remain fixed.
    LinesPerGrid,
}

/// Where a frame's text sits relative to the page's grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridSettings {
    pub mode: GridMode,
    /// Baselines per inch. The usual value is 12 for leading in points,
    /// because 72/12 is 6pt.
    pub baseline_count: f32,
    /// Divide the page's measure into this many text columns.
    pub columns: u16,
    /// Space between the grid's columns.
    pub column_gutter: Pt,
    /// Rows per column, for a grid with a row rhythm as well.
    pub rows: u16,
    /// Snap objects to the grid when they are dragged.
    pub snap_objects: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            mode: GridMode::None,
            // 12 per inch is a 6pt rhythm, the most common body leading.
            baseline_count: 12.0,
            columns: 1,
            column_gutter: 0.0,
            rows: 0,
            snap_objects: false,
        }
    }
}

impl GridSettings {
    /// The distance between baselines, in points.
    ///
    /// `None` for a grid that cannot produce one -- a zero or negative
    /// count is a corrupted document, and dividing by it would produce
    /// infinities that propagate into every line's position.
    pub fn baseline_interval(&self) -> Option<Pt> {
        if self.baseline_count <= 0.0 || !self.baseline_count.is_finite() {
            return None;
        }
        let interval = 72.0 / self.baseline_count;
        (interval > 0.0 && interval.is_finite()).then_some(interval)
    }

    /// The first baseline's y, measured from the page's top edge.
    ///
    /// The grid starts at the top margin, not at the paper edge, so the
    /// margin is the first thing text sits in.
    pub fn first_baseline(&self, page: &Page) -> Option<Pt> {
        self.baseline_interval()?;
        Some(page.margins.top)
    }

    /// The text area divided into the grid's columns.
    pub fn content_columns(&self, page: &Page) -> Vec<Rect> {
        crate::compose::columns(page.content_rect(), self.columns.max(1), self.column_gutter)
    }

    /// The y of every baseline on a page, in page space.
    ///
    /// These are the guides a canvas draws. The count is bounded so a
    /// corrupted `baseline_count` cannot make this allocate without end.
    pub fn baselines(&self, page: &Page) -> Vec<Pt> {
        let Some(interval) = self.baseline_interval() else {
            return Vec::new();
        };
        let Some(first) = self.first_baseline(page) else {
            return Vec::new();
        };
        let bottom = page.height + page.bleed.bottom;
        let mut out = Vec::new();
        let mut y = first;
        // 4000 lines is far past any real page and stops a divide-by-a-
        // rounding-error from looping for ever.
        while y <= bottom && out.len() < 4000 {
            out.push(y);
            y += interval;
        }
        out
    }

    /// The leading to use so that baselines land on the grid.
    ///
    /// `wanted` is the style's leading. `None` when the grid is off or
    /// has no interval, so a caller keeps the style's own value.
    pub fn leading_for(&self, page: &Page, wanted: Pt) -> Option<Pt> {
        if !wanted.is_finite() || wanted <= 0.0 {
            return None;
        }
        self.first_baseline(page)?;
        match self.mode {
            GridMode::SnapToGrid => Some(wanted),
            GridMode::LinesPerGrid => {
                let interval = self.baseline_interval()?;
                Some((wanted / interval - 0.00001).ceil().max(1.0) * interval)
            }
            GridMode::None => None,
        }
    }

    /// Snap a value to the nearest grid line, or return it unchanged
    /// when the grid cannot produce lines.
    pub fn snap_y(&self, page: &Page, y: Pt) -> Pt {
        if self.mode == GridMode::None {
            return y;
        }
        let Some(interval) = self.baseline_interval() else {
            return y;
        };
        let Some(first) = self.first_baseline(page) else {
            return y;
        };
        if interval <= 0.0 {
            return y;
        }
        let steps = ((y - first) / interval).round();
        if !steps.is_finite() {
            return y;
        }
        first + steps * interval
    }

    /// Snap a rectangle to the grid.
    pub fn snap_rect(&self, page: &Page, rect: Rect) -> Rect {
        if !self.snap_objects {
            return rect;
        }
        let top = self.snap_y(page, rect.y);
        let bottom = self.snap_y(page, rect.bottom());
        Rect::new(rect.x, top, rect.width, (bottom - top).max(0.0))
    }
}

/// The document's grid.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GridSet {
    /// Grid settings for the document as a whole.
    pub document: GridSettings,
    /// Named grids applied to particular frames, by frame name. A
    /// catalogue-style document often has a body grid and a separate
    /// one for captions or folios.
    pub named: Vec<(String, GridSettings)>,
}

impl GridSet {
    pub fn with_defaults() -> GridSet {
        GridSet {
            document: GridSettings::default(),
            named: Vec::new(),
        }
    }

    /// The grid in force for a frame: a named one if the frame has it,
    /// otherwise the document's.
    pub fn for_frame(&self, name: &str) -> &GridSettings {
        self.named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, g)| g)
            .unwrap_or(&self.document)
    }

    pub fn set_named(&mut self, name: impl Into<String>, settings: GridSettings) {
        let name = name.into();
        match self.named.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = settings,
            None => self.named.push((name, settings)),
        }
    }
}

/// The document's grid, if it has one.
pub fn grid_of(doc: &LayoutDocument) -> GridSettings {
    doc.grids.document.clone()
}

/// Insets a document grid implies for a page, for a UI that offers a
/// "grid" button rather than free margin entry.
pub fn margins_for_grid(page: &Page, settings: &GridSettings) -> Option<Insets> {
    settings.baseline_interval()?;
    if settings.columns < 2 {
        return None;
    }
    // A text column at least six characters wide is the usual rule of
    // thumb; 45pt is about that at a 10pt size. The width is chosen
    // first and the margins derived from it, so a narrow page gets the
    // minimum column and equal margins rather than margins computed from
    // a division that lands a rounding error short of it.
    const MINIMUM_COLUMN: Pt = 45.0;
    let columns = settings.columns as Pt;
    let gutters = settings.column_gutter * (columns - 1.0);
    let available = (page.width - gutters).max(0.0);
    let column = (available / columns).max(MINIMUM_COLUMN);
    let margin = ((page.width - column * columns - gutters) / 2.0).max(0.0);
    Some(Insets::new(
        page.margins.top,
        margin,
        page.margins.bottom,
        margin,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::mm;

    fn grid() -> GridSettings {
        GridSettings {
            mode: GridMode::SnapToGrid,
            baseline_count: 12.0,
            ..GridSettings::default()
        }
    }

    #[test]
    fn twelve_baselines_an_inch_is_a_six_point_rhythm() {
        assert!((grid().baseline_interval().unwrap() - 6.0).abs() < 1e-4);
    }

    #[test]
    fn a_baseline_count_of_zero_produces_no_grid() {
        // A corrupted document, and dividing by it would put an infinity
        // into every line's position.
        let settings = GridSettings {
            baseline_count: 0.0,
            ..grid()
        };
        assert_eq!(settings.baseline_interval(), None);
        assert!(settings.baselines(&Page::a4()).is_empty());
    }

    #[test]
    fn a_negative_or_nonsense_count_produces_no_grid() {
        for count in [-1.0, f32::NAN, f32::INFINITY] {
            let settings = GridSettings {
                baseline_count: count,
                ..grid()
            };
            assert_eq!(settings.baseline_interval(), None, "count {count}");
        }
    }

    #[test]
    fn baselines_start_at_the_top_margin() {
        let mut page = Page::a4();
        page.margins.top = mm(20.0);
        let lines = grid().baselines(&page);
        assert!(lines.len() > 10);
        assert!((lines[0] - page.margins.top).abs() < 1e-4);
        // And they are evenly spaced.
        let step = lines[1] - lines[0];
        assert!((step - 6.0).abs() < 1e-4);
    }

    #[test]
    fn baselines_cover_the_bleed_as_well_as_the_trim() {
        let mut page = Page::a4();
        page.bleed = (mm(10.0)).into();
        let lines = grid().baselines(&page);
        let last = *lines.last().unwrap();
        assert!(
            last > page.height,
            "baselines stop at the trim, not the bleed"
        );
    }

    #[test]
    fn a_grid_off_snaps_nothing() {
        let settings = GridSettings {
            mode: GridMode::None,
            ..grid()
        };
        let page = Page::a4();
        let y = 137.0;
        assert_eq!(settings.snap_y(&page, y), y);
    }

    #[test]
    fn snapping_moves_a_value_to_the_nearest_baseline() {
        let page = Page::a4();
        let y = 6.4;
        let snapped = grid().snap_y(&page, y);
        assert!((snapped - y).abs() <= 3.0, "{snapped} from {y}");
        assert!(
            (snapped / 6.0 - (snapped / 6.0).round()).abs() < 1e-4,
            "off the grid"
        );
    }

    #[test]
    fn snapping_is_idempotent() {
        let page = Page::a4();
        let once = grid().snap_y(&page, 137.0);
        assert_eq!(grid().snap_y(&page, once), once);
    }

    #[test]
    fn snapping_does_nothing_when_the_grid_is_off() {
        let settings = GridSettings {
            mode: GridMode::None,
            ..grid()
        };
        assert_eq!(settings.snap_y(&Page::a4(), 137.0), 137.0);
    }

    #[test]
    fn snapping_preserves_the_minimum_requested_leading() {
        let page = Page::a4();
        let leading = grid().leading_for(&page, 14.0);
        assert_eq!(leading, Some(14.0));
    }

    #[test]
    fn lines_per_grid_rounds_leading_up_to_whole_intervals() {
        let settings = GridSettings {
            mode: GridMode::LinesPerGrid,
            ..grid()
        };
        let page = Page::a4();
        assert_eq!(settings.leading_for(&page, 14.0), Some(18.0));
    }

    #[test]
    fn a_grid_off_leaves_the_leading_to_the_style() {
        let settings = GridSettings {
            mode: GridMode::None,
            ..grid()
        };
        assert_eq!(settings.leading_for(&Page::a4(), 14.0), None);
    }

    #[test]
    fn object_snapping_is_opt_in() {
        let page = Page::a4();
        let rect = Rect::new(0.0, 137.0, 100.0, 20.0);
        let off = grid();
        assert_eq!(off.snap_rect(&page, rect), rect);
        let mut on = grid();
        on.snap_objects = true;
        let snapped = on.snap_rect(&page, rect);
        assert_ne!(snapped.y, rect.y);
        // The height follows the snap, so the object stays on grid lines
        // at both ends.
        assert!((snapped.bottom() / 6.0 - (snapped.bottom() / 6.0).round()).abs() < 1e-3);
    }

    #[test]
    fn a_named_grid_overrides_the_document_grid() {
        let mut grids = GridSet::with_defaults();
        grids.set_named(
            "Caption",
            GridSettings {
                mode: GridMode::SnapToGrid,
                baseline_count: 24.0,
                ..GridSettings::default()
            },
        );
        assert_eq!(grids.for_frame("Caption").baseline_interval(), Some(3.0));
        assert_eq!(
            grids.for_frame("Body").baseline_interval(),
            Some(72.0 / 12.0)
        );
    }

    #[test]
    fn setting_a_named_grid_replaces_it_rather_than_duplicating() {
        let mut grids = GridSet::with_defaults();
        grids.set_named(
            "Caption",
            GridSettings {
                baseline_count: 24.0,
                ..GridSettings::default()
            },
        );
        grids.set_named(
            "Caption",
            GridSettings {
                baseline_count: 18.0,
                ..GridSettings::default()
            },
        );
        assert_eq!(grids.named.len(), 1);
        assert_eq!(grids.for_frame("Caption").baseline_count, 18.0);
    }

    #[test]
    fn content_columns_are_the_grid_columns_inside_the_margins() {
        let mut page = Page::a4();
        page.margins = Insets::new(mm(20.0), mm(20.0), mm(20.0), mm(20.0));
        let settings = GridSettings {
            columns: 3,
            column_gutter: mm(4.0),
            ..GridSettings::default()
        };
        let cols = settings.content_columns(&page);
        assert_eq!(cols.len(), 3);
        // They span the content width plus the gutters.
        let spanned = cols[2].right() - cols[0].x;
        assert!((spanned - page.content_rect().width).abs() < 0.01);
    }

    #[test]
    fn grid_margins_give_two_columns_equal_widths() {
        // A page too narrow for two readable columns gets equal margins
        // rather than a negative one, and the columns come out equal.
        let page = Page::new("Narrow", mm(60.0), mm(200.0));
        let settings = GridSettings {
            columns: 2,
            column_gutter: mm(4.0),
            ..GridSettings::default()
        };
        let margins = margins_for_grid(&page, &settings).expect("margins for a two column grid");
        let cols = crate::compose::columns(
            Rect::new(
                margins.left,
                page.margins.top,
                page.width - margins.left - margins.right,
                page.height,
            ),
            2,
            settings.column_gutter,
        );
        assert!((cols[0].width - cols[1].width).abs() < 1e-3, "{:?}", cols);
    }

    #[test]
    fn a_page_too_narrow_for_the_minimum_column_still_answers() {
        // Two 45pt columns plus a gutter need more than this page has.
        // The margins clamp at zero rather than going negative and
        // producing a text area outside the paper.
        let page = Page::new("Tiny", mm(20.0), mm(200.0));
        let settings = GridSettings {
            columns: 2,
            column_gutter: mm(4.0),
            ..GridSettings::default()
        };
        let margins = margins_for_grid(&page, &settings).expect("margins");
        assert_eq!(margins.left, 0.0);
        assert_eq!(margins.right, 0.0);
    }

    #[test]
    fn a_single_column_grid_needs_no_computed_margins() {
        assert!(margins_for_grid(&Page::a4(), &GridSettings::default()).is_none());
    }
}
