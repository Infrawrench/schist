//! Page geometry: units, points, rectangles, pages and spreads.
//!
//! Everything here is in **points** (1/72 inch), the unit page layout
//! actually works in. Millimetres and inches are presentation and input
//! conveniences that convert at the edges. IDML uses decimal XML geometry;
//! any fixed-point conversion needed by a binary codec belongs there.
//!
//! A `Page` is a sheet of a fixed size. A `Spread` is a group of pages
//! presented side by side, the way a bound document opens to two facing
//! pages. Spreads carry presentation geometry only -- moving one around
//! the pasteboard must not change where a page sits in the document.

use serde::{Deserialize, Serialize};

/// A length in points, as a plain scalar.
///
/// This is `f32` to match the rest of the compositor and the printing
/// layout, which is ample for pages measured in points.
pub type Pt = f32;

pub const POINTS_PER_INCH: Pt = 72.0;
/// 1 inch is exactly 25.4 mm by definition, so this is a constant rather
/// than an approximation.
pub const MM_PER_INCH: Pt = 25.4;

/// Points per millimetre.
pub const PT_PER_MM: Pt = POINTS_PER_INCH / MM_PER_INCH;

pub fn mm(value: Pt) -> Pt {
    value * PT_PER_MM
}

pub fn pt_from_mm(value: Pt) -> Pt {
    value / PT_PER_MM
}

pub fn inch(value: Pt) -> Pt {
    value * POINTS_PER_INCH
}

pub fn to_mm(value: Pt) -> Pt {
    value / PT_PER_MM
}

pub fn to_inch(value: Pt) -> Pt {
    value / POINTS_PER_INCH
}

/// A position or size on the page plane, in points.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: Pt,
    pub y: Pt,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: Pt, y: Pt) -> Self {
        Self { x, y }
    }

    /// Componentwise sum.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Point) -> Point {
        Point::new(self.x + other.x, self.y + other.y)
    }

    /// Componentwise difference.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, other: Point) -> Point {
        Point::new(self.x - other.x, self.y - other.y)
    }

    pub fn scale(self, factor: Pt) -> Point {
        Point::new(self.x * factor, self.y * factor)
    }

    /// Rotation about the origin, in degrees clockwise.
    ///
    /// The page plane puts y downwards and x to the right, so a positive
    /// angle turns the same way it does on screen.
    pub fn rotate(self, degrees: Pt) -> Point {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Point::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }

    /// The signed area of the triangle spanned with `other`, doubled.
    ///
    /// This is the cross product test every polygon and point-in-polygon
    /// routine is built from, so it lives with the point type.
    pub fn cross(self, other: Point) -> Pt {
        self.x * other.y - self.y * other.x
    }
}

impl std::ops::Add for Point {
    type Output = Point;
    fn add(self, rhs: Point) -> Point {
        Point::add(self, rhs)
    }
}

impl std::ops::Sub for Point {
    type Output = Point;
    fn sub(self, rhs: Point) -> Point {
        Point::sub(self, rhs)
    }
}

/// An axis-aligned rectangle, in points.
///
/// `width`/`height` are always non-negative. A zero extent is legal and
/// means an empty region, which is how a freshly created frame with no
/// content behaves.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: Pt,
    pub y: Pt,
    pub width: Pt,
    pub height: Pt,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(x: Pt, y: Pt, width: Pt, height: Pt) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// A rectangle spanning two corners in any order.
    pub fn from_corners(a: Point, b: Point) -> Rect {
        let (left, right) = if a.x <= b.x { (a.x, b.x) } else { (b.x, a.x) };
        let (top, bottom) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
        Rect::new(left, top, right - left, bottom - top)
    }

    pub fn from_size(origin: Point, size: Point) -> Rect {
        Rect::new(origin.x, origin.y, size.x, size.y)
    }

    pub fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn size(self) -> Point {
        Point::new(self.width, self.height)
    }

    pub fn left(self) -> Pt {
        self.x
    }

    pub fn top(self) -> Pt {
        self.y
    }

    pub fn right(self) -> Pt {
        self.x + self.width
    }

    pub fn bottom(self) -> Pt {
        self.y + self.height
    }

    pub fn center(self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn is_empty(self) -> bool {
        !(self.width > 0.0 && self.height > 0.0)
    }

    pub fn contains(self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    pub fn intersects(self, other: Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The shared region, or a zero rect when the two are apart.
    pub fn intersection(self, other: Rect) -> Rect {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= left || bottom <= top {
            return Rect::ZERO;
        }
        Rect::from_corners(Point::new(left, top), Point::new(right, bottom))
    }

    pub fn inset(self, insets: Insets) -> Rect {
        let x = self.x + insets.left;
        let y = self.y + insets.top;
        let width = self.width - insets.left - insets.right;
        let height = self.height - insets.top - insets.bottom;
        if width <= 0.0 || height <= 0.0 {
            return Rect::ZERO;
        }
        Rect::new(x, y, width, height)
    }

    /// The smallest rectangle containing this one and `other`.
    ///
    /// A zero-size rectangle is *not* special-cased as absent. It used to
    /// be, on the assumption that an empty rect meant "no value yet" —
    /// which is true of an accumulator and false of a point, and the two
    /// are the same type. The cost was that `ShapePath::bounds`, which
    /// unions one zero-size rect per path point, returned the last point
    /// rather than the whole outline: a shape was placed at a point
    /// instead of where it was drawn.
    pub fn union(self, other: Rect) -> Rect {
        Rect::from_corners(
            Point::new(self.x.min(other.x), self.y.min(other.y)),
            Point::new(
                self.right().max(other.right()),
                self.bottom().max(other.bottom()),
            ),
        )
    }

    /// Move the rectangle by a delta.
    /// Whether this rectangle wholly contains another.
    pub fn contains_rect(&self, other: Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    pub fn translated(self, delta: Point) -> Rect {
        Rect::new(self.x + delta.x, self.y + delta.y, self.width, self.height)
    }

    /// Move and resize from two opposing corners, as a drag does.
    pub fn dragged(self, origin: Point, corner: Point) -> Rect {
        Rect::from_corners(origin, corner)
    }
}

/// Per-edge insets, in points.
///
/// Positive values shrink the content area. This is what a text frame's
/// margins are.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Insets {
    pub top: Pt,
    pub right: Pt,
    pub bottom: Pt,
    pub left: Pt,
}

impl Insets {
    pub const ZERO: Insets = Insets {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    pub const fn uniform(value: Pt) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub const fn new(top: Pt, right: Pt, bottom: Pt, left: Pt) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.top.max(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
            self.left.max(other.left),
        )
    }

    pub fn expanded(self, amount: Pt) -> Self {
        Self::new(
            self.top + amount,
            self.right + amount,
            self.bottom + amount,
            self.left + amount,
        )
    }

    pub fn mirrored(self) -> Self {
        Self::new(self.top, self.left, self.bottom, self.right)
    }

    pub fn is_uniform(self) -> bool {
        self.top == self.right && self.top == self.bottom && self.top == self.left
    }

    pub fn is_zero(self) -> bool {
        self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0 && self.left == 0.0
    }
}

impl From<Pt> for Insets {
    fn from(value: Pt) -> Self {
        Self::uniform(value)
    }
}

/// Cubic Bézier handles in the same coordinates as their anchor.
/// An absent handle coincides with the anchor (a corner).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BezierHandles {
    pub incoming: Option<Point>,
    pub outgoing: Option<Point>,
}

/// An open or closed contour. Shape coordinates are relative to the
/// placed object's bounds origin, independent of raster pixels.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubPath {
    pub points: Vec<Point>,
    pub closed: bool,
    /// Indexed by anchor. Missing entries mean corner points, preserving
    /// existing polygon documents. Entries past `points.len()` are ignored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<BezierHandles>,
}

/// A vector shape: one or more subpaths and a fill rule.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShapePath {
    pub subpaths: Vec<SubPath>,
    /// How overlapping subpaths combine. Nonzero counts winding
    /// direction; even-odd counts crossings.
    pub even_odd: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    pub fn as_bool(self) -> bool {
        self == Orientation::Landscape
    }
}

/// How a page's number is derived for display.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NumberStyle {
    /// Plain western arabic: 1, 2, 3.
    Arabic,
    /// Lowercase roman: i, ii, iii.
    RomanLower,
    /// Uppercase roman: I, II, III.
    RomanUpper,
    /// Lowercase letters: a, b, c.
    AlphaLower,
    /// Uppercase letters: A, B, C.
    AlphaUpper,
}

impl NumberStyle {
    /// Render `number` (1-based) in this style.
    ///
    /// Zero becomes one. Roman values above 3999 add M symbols; letter
    /// styles continue with repeated letters (aa, bb, ...).
    pub fn format(self, number: u32) -> String {
        let n = number.max(1);
        match self {
            NumberStyle::Arabic => n.to_string(),
            NumberStyle::RomanLower => to_roman(n).to_lowercase(),
            NumberStyle::RomanUpper => to_roman(n),
            NumberStyle::AlphaLower => to_alpha(n, false),
            NumberStyle::AlphaUpper => to_alpha(n, true),
        }
    }
}

/// The classic roman numerals. Values above 3999 keep adding M rather
/// than growing overlines, which is what a page label wants: still
/// readable, still unambiguous, and no typographic rule to disagree about.
fn to_roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, symbol) in TABLE {
        while n >= value {
            out.push_str(symbol);
            n -= value;
        }
    }
    out
}

/// Repeated page-number letters: 1 -> a, 26 -> z, 27 -> aa, 28 -> bb.
///
/// The repeated-letter form (aa, bb) is the convention for page prefixes
/// past the first 26.
fn to_alpha(n: u32, upper: bool) -> String {
    let letter = char::from(b'a' + ((n - 1) % 26) as u8);
    let repeats = (n - 1) / 26 + 1;
    let s: String = letter.to_string().repeat(repeats as usize);
    if upper {
        s.to_uppercase()
    } else {
        s
    }
}

/// A single sheet.
///
/// `width`/`height` are the **trim** size -- the finished page after
/// cutting. Bleed and slug sit outside it; the printing area is trim plus
/// bleed. Margins live in the page too, because in a page-layout document
/// they are a property of the page rather than of any one frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub name: String,
    /// A numbering section begins here and follows this page when reordered.
    /// Pages without a boundary inherit the preceding section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<crate::Section>,
    /// Trim width, in points.
    pub width: Pt,
    /// Trim height, in points.
    pub height: Pt,
    /// Physical top/right/bottom/left offsets beyond trim, in points.
    pub bleed: Insets,
    /// Slug offsets measured from trim, independently of bleed. The media
    /// contains both areas, even when one slug edge is inside the bleed.
    pub slug: Insets,
    pub margins: Insets,
    pub orientation: Orientation,
    /// Hidden pages retain their numbering and appear muted on the pasteboard.
    /// Output includes them only when the user requests hidden pages.
    pub hidden: bool,
    /// The page this one's settings were copied from, if any.
    pub master: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<RulerGuide>,
}

/// A page-local, nonprinting ruler guide. Coordinates are measured from trim.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RulerGuide {
    pub horizontal: bool,
    pub position: Pt,
    #[serde(default)]
    pub locked: bool,
}

impl Page {
    /// A page of the given trim size with no bleed, slug or margins.
    pub fn new(name: impl Into<String>, width: Pt, height: Pt) -> Page {
        Page {
            name: name.into(),
            section: None,
            width: width.max(1.0),
            height: height.max(1.0),
            bleed: Insets::ZERO,
            slug: Insets::ZERO,
            margins: Insets::ZERO,
            orientation: if width > height {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            },
            hidden: false,
            master: None,
            guides: Vec::new(),
        }
    }

    /// US Letter, the most common default in print workflows.
    pub fn letter() -> Page {
        Page::new("Letter", inch(8.5), inch(11.0))
    }

    /// ISO A4.
    pub fn a4() -> Page {
        Page::new("A4", mm(210.0), mm(297.0))
    }

    /// Trim plus bleed: the paper that has to be printed for a page to
    /// come off the press without a white sliver at the cut.
    pub fn bleed_rect(&self) -> Rect {
        self.expanded_rect(self.bleed)
    }

    /// Paper enclosing both bleed and slug, whose offsets are measured from
    /// trim independently. Printer marks may require additional output media.
    pub fn media_rect(&self) -> Rect {
        self.expanded_rect(self.bleed.max(self.slug))
    }

    fn expanded_rect(&self, offsets: Insets) -> Rect {
        Rect::new(
            -offsets.left,
            -offsets.top,
            self.width + offsets.left + offsets.right,
            self.height + offsets.top + offsets.bottom,
        )
    }

    /// The printable content area: trim shrunk by the page margins.
    pub fn content_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height).inset(self.margins)
    }

    /// Swap width and height, preserving the name.
    pub fn rotated(&self) -> Page {
        let mut page = self.clone();
        std::mem::swap(&mut page.width, &mut page.height);
        page.orientation = if page.width > page.height {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        };
        std::mem::swap(&mut page.margins.left, &mut page.margins.right);
        std::mem::swap(&mut page.margins.top, &mut page.margins.bottom);
        page
    }
}

/// Pages presented together, the way a bound document opens.
///
/// The spread is where a reader sees facing pages, so it owns the gap
/// between them and where the group sits on the pasteboard. It does not
/// own the pages: they carry their own geometry, and reordering a spread
/// must not resize anything.
/// The gap between two adjacent spreads on the pasteboard, in points.
///
/// A reader needs to see where one spread ends and the next begins, and a
/// flush join does not show that.
pub const SPREAD_GAP: Pt = 24.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    /// Physical order, from left to right, independent of reading direction.
    pub pages: Vec<usize>,
    /// Number of pages left of the binding spine. None uses the document's
    /// facing-page convention; an imported explicit spine stays fixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_location: Option<usize>,
    /// Gap between adjacent pages, in points.
    pub gutter: Pt,
    /// Where this spread sits on the pasteboard.
    ///
    /// Kept for a document that stores its own layout, but **not**
    /// consulted when drawing: [`crate::model::LayoutDocument::spread_origins`]
    /// lays spreads out, because this field is zero in every document
    /// built through the API and trusting it stacks them.
    pub origin: Point,
}

impl Spread {
    pub fn single(page: usize) -> Spread {
        Spread {
            pages: vec![page],
            binding_location: None,
            gutter: 0.0,
            origin: Point::ZERO,
        }
    }

    /// The combined trim area, laid out left to right.
    pub fn bounds(&self, pages: &[Page]) -> Rect {
        let mut out = Rect::ZERO;
        for (i, index) in self.pages.iter().enumerate() {
            let Some(page) = pages.get(*index) else {
                continue;
            };
            let x = if i == 0 { 0.0 } else { out.width + self.gutter };
            out = out.union(Rect::new(x, 0.0, page.width, page.height));
        }
        out
    }

    /// Where page `index` sits within the spread, relative to the spread
    /// origin. Spreads are laid out left to right regardless of reading
    /// order, so this is positional, not logical.
    pub fn page_origin(&self, pages: &[Page], index: usize) -> Point {
        let mut x = 0.0;
        for (i, page_index) in self.pages.iter().enumerate() {
            let Some(page) = pages.get(*page_index) else {
                continue;
            };
            if i == index {
                break;
            }
            x += page.width + self.gutter;
        }
        Point::new(x, 0.0)
    }
}

/// Reading progression between pages, independent of text/story direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageBinding {
    #[default]
    LeftToRight,
    RightToLeft,
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_union_of_points_is_the_box_around_them() {
        // `ShapePath::bounds` unions one zero-size rect per point, so a
        // union that treated a zero-size rect as "no value yet" returned
        // the last point instead of the whole outline -- which places a
        // shape at a point rather than where it was drawn.
        let a = Rect::new(-104.0, -114.0, 0.0, 0.0);
        let b = Rect::new(696.0, 486.0, 0.0, 0.0);
        let bounds = a.union(b);
        assert_eq!(bounds, Rect::new(-104.0, -114.0, 800.0, 600.0));
        // And the reverse order gives the same answer, which an
        // accumulator-style union would not.
        assert_eq!(b.union(a), bounds);
    }

    #[test]
    fn a_union_with_a_rect_containing_the_origin_keeps_the_origin() {
        // The pasteboard's bounds start at zero, and a page at the origin
        // must not pull the whole document's bounds to the origin corner.
        let placed = Rect::new(0.0, 0.0, 595.0, 842.0);
        let page = Rect::new(595.0, 0.0, 595.0, 842.0);
        assert_eq!(placed.union(page).width, 1190.0);
    }

    use super::*;

    #[test]
    fn millimetres_and_inches_agree_at_a4() {
        // A4 is 210 x 297 mm; the inch definition makes these exact.
        let a4 = Page::a4();
        assert!((to_mm(a4.width) - 210.0).abs() < 0.001);
        assert!((to_mm(a4.height) - 297.0).abs() < 0.001);
        let letter = Page::letter();
        assert!((to_inch(letter.width) - 8.5).abs() < 0.001);
        assert!((to_inch(letter.height) - 11.0).abs() < 0.001);
    }

    #[test]
    fn letter_is_portrait_and_its_rotation_is_not() {
        let letter = Page::letter();
        assert_eq!(letter.orientation, Orientation::Portrait);
        let landscape = letter.rotated();
        assert_eq!(landscape.orientation, Orientation::Landscape);
        assert_eq!(landscape.width, letter.height);
        assert_eq!(landscape.height, letter.width);
    }

    #[test]
    fn bleed_and_slug_offsets_are_measured_from_trim() {
        let mut page = Page::a4();
        page.bleed = (mm(3.0)).into();
        page.slug = (mm(5.0)).into();
        let bleed = page.bleed_rect();
        let media = page.media_rect();
        assert_eq!(
            bleed.origin(),
            Point::new(-page.bleed.left, -page.bleed.top)
        );
        assert_eq!(media.origin(), Point::new(-page.slug.left, -page.slug.top));
        assert!((bleed.center().x - page.width / 2.0).abs() < 0.001);
        assert!((media.center().y - page.height / 2.0).abs() < 0.001);
        // Bleed is trim grown by the bleed on every side...
        assert!(bleed.width > page.width);
        let bleed_expected = page.width + mm(3.0) * 2.0;
        assert!((bleed.width - bleed_expected).abs() < 0.001);
        // ...and slug grows it further.
        assert!(media.width > bleed.width);
        let expected = page.width + mm(5.0) * 2.0;
        assert!((media.width - expected).abs() < 0.001);
    }

    #[test]
    fn insets_shrink_the_content_area() {
        let page = Page {
            margins: Insets::new(mm(20.0), mm(15.0), mm(20.0), mm(15.0)),
            ..Page::a4()
        };
        let content = page.content_rect();
        assert!((content.x - mm(15.0)).abs() < 0.001);
        assert!((content.width - (page.width - mm(30.0))).abs() < 0.001);
    }

    #[test]
    fn over_insetting_collapses_to_empty_rather_than_inverting() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let collapsed = rect.inset(Insets::uniform(20.0));
        assert!(collapsed.is_empty());
        assert!(collapsed.width >= 0.0);
    }

    #[test]
    fn rect_from_corners_normalises_reversed_input() {
        let a = Rect::from_corners(Point::new(50.0, 60.0), Point::new(10.0, 20.0));
        assert_eq!(a, Rect::new(10.0, 20.0, 40.0, 40.0));
    }

    #[test]
    fn intersection_of_disjoint_rects_is_empty() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);
        assert!(!a.intersects(b));
        assert!(a.intersection(b).is_empty());
        // A zero-area rect covers no ground and so intersects nothing.
        assert!(!a.intersects(Rect::new(0.0, 0.0, 0.0, 10.0)));
    }

    #[test]
    fn spread_bounds_account_for_the_gutter() {
        let pages = vec![Page::a4(), Page::a4()];
        let spread = Spread {
            pages: vec![0, 1],
            binding_location: None,
            gutter: mm(20.0),
            origin: Point::ZERO,
        };
        let bounds = spread.bounds(&pages);
        assert!((bounds.width - (pages[0].width * 2.0 + mm(20.0))).abs() < 0.001);
        let second = spread.page_origin(&pages, 1);
        assert!((second.x - (pages[0].width + mm(20.0))).abs() < 0.001);
        assert_eq!(spread.page_origin(&pages, 0), Point::ZERO);
    }

    #[test]
    fn spread_ignores_page_indices_that_do_not_exist() {
        let pages = vec![Page::a4()];
        let spread = Spread {
            pages: vec![0, 99],
            binding_location: None,
            gutter: 0.0,
            origin: Point::ZERO,
        };
        // The bad index contributes nothing rather than panicking.
        assert!((spread.bounds(&pages).width - pages[0].width).abs() < 0.001);
    }

    #[test]
    fn number_styles_render_the_expected_labels() {
        assert_eq!(NumberStyle::Arabic.format(7), "7");
        assert_eq!(NumberStyle::RomanLower.format(4), "iv");
        assert_eq!(NumberStyle::RomanUpper.format(1987), "MCMLXXXVII");
        assert_eq!(NumberStyle::AlphaLower.format(1), "a");
        assert_eq!(NumberStyle::AlphaUpper.format(26), "Z");
        // Past 26 the repeated-letter form takes over.
        assert_eq!(NumberStyle::AlphaLower.format(27), "aa");
        // 3999 is the largest number roman numerals are conventionally
        // used for; beyond it we keep adding M rather than growing an
        // overline.
        assert_eq!(NumberStyle::RomanUpper.format(3999), "MMMCMXCIX");
        assert_eq!(NumberStyle::RomanUpper.format(4000), "MMMM");
    }

    #[test]
    fn zero_is_treated_as_page_one() {
        // A document can be configured to start numbering at 0, but a
        // label must never render as an empty string.
        assert_eq!(NumberStyle::Arabic.format(0), "1");
        assert_eq!(NumberStyle::RomanLower.format(0), "i");
    }

    #[test]
    fn rotation_turns_clockwise_on_the_page_plane() {
        let p = Point::new(10.0, 0.0).rotate(90.0);
        assert!((p.x).abs() < 1e-4);
        assert!((p.y - 10.0).abs() < 1e-4);
    }
}
