//! Gradient swatches and the gradients items and text are filled and
//! stroked with. A gradient runs its stops along a line (linear) or out from
//! a centre (radial), in the item's own path coordinates: from `start`,
//! `length` points long, at `angle` degrees counter-clockwise from the x axis.
//!
//! An item that states no start begins at its path's left and bottom edges,
//! `length` its width: InDesign's own exports write exactly that for a
//! gradient applied in its user interface, and its PDF of the public
//! paged-media `gradients` sample, whose items state none, draws it so. The
//! evidence is in `docs/idml-format.md`. A stroke runs the same way over the
//! same path: no public sample strokes with a gradient, so this is the
//! specification's GradientStrokeStart, Length and Angle read as their fill
//! counterparts are.
//!
//! Text has no path of its own: a gradient on text runs over the frame it is
//! set in, as an item's runs over its path, so each glyph shows the part of
//! the gradient it sits on. No public sample sets text in a gradient either;
//! this is a Schist reading, consistent with fills. InDesign's defaults for
//! text, GradientFillStart "0 0" with GradientFillLength -1, state no vector.
use crate::{Ink, Point, Pt, Rect};
use serde::{Deserialize, Serialize};

/// A gradient swatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub name: String,
    /// Radial rather than linear.
    pub radial: bool,
    /// At least two, in order of location.
    pub stops: Vec<GradientStop>,
}

/// One colour of a gradient.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub ink: Ink,
    /// Where along the gradient, 0 to 1.
    pub location: f32,
    /// Where between the previous stop and this one the two colours mix
    /// equally, 0 to 1 of that span; a half mixes them evenly.
    pub midpoint: f32,
}

/// A gradient applied to an item's fill or stroke, or to text, and where it
/// runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientFill {
    pub gradient: Gradient,
    /// In the item's path coordinates; None begins at the path's left and
    /// bottom edges. Text keeps the start it states for saving and does not
    /// draw it: see [`GradientFill::text_vector`].
    pub start: Option<Point>,
    /// The line's length or the radius; None is the path's width.
    pub length: Option<Pt>,
    /// Degrees counter-clockwise from the x axis.
    pub angle: Pt,
}

impl Gradient {
    /// Stops in range, in order, at least two of them.
    pub fn valid(&self) -> bool {
        self.stops.len() >= 2
            && self
                .stops
                .iter()
                .all(|s| (0.0..=1.0).contains(&s.location) && (0.0..=1.0).contains(&s.midpoint))
            && self
                .stops
                .windows(2)
                .all(|pair| pair[0].location <= pair[1].location)
    }

    /// At `t` along the gradient: the stops either side and how far toward
    /// the second the colour is, the midpoint applied as an exponent so the
    /// mix is even there (a Schist reading: the public sample's midpoints
    /// are all even).
    pub fn mix(&self, t: f32) -> (usize, usize, f32) {
        let last = self.stops.len() - 1;
        if t <= self.stops[0].location {
            return (0, 0, 0.0);
        }
        if t >= self.stops[last].location {
            return (last, last, 0.0);
        }
        let next = self
            .stops
            .iter()
            .position(|s| s.location > t)
            .unwrap_or(last);
        let (a, b) = (&self.stops[next - 1], &self.stops[next]);
        let span = b.location - a.location;
        let mut f = if span > 0.0 {
            (t - a.location) / span
        } else {
            1.0
        };
        let m = b.midpoint.clamp(0.01, 0.99);
        if (m - 0.5).abs() > 1e-4 {
            f = f.powf(0.5f32.ln() / m.ln());
        }
        (next - 1, next, f.clamp(0.0, 1.0))
    }

    /// A screen approximation of the colour at `t`.
    pub fn preview(&self, t: f32) -> [f32; 3] {
        let (a, b, f) = self.mix(t);
        let (a, b) = (
            self.stops[a].ink.preview_at_tint(1.0),
            self.stops[b].ink.preview_at_tint(1.0),
        );
        [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * f)
    }
}

impl GradientFill {
    /// The start, the unit direction and the length for a path whose
    /// bounding box is `bounds`, in path coordinates.
    pub fn vector(&self, bounds: Rect) -> (Point, Point, Pt) {
        let start = self
            .start
            .unwrap_or_else(|| Point::new(bounds.x, bounds.bottom()));
        let length = self.length.unwrap_or(bounds.width).max(1e-3);
        let radians = self.angle.to_radians();
        // Path coordinates run down the page.
        (start, Point::new(radians.cos(), -radians.sin()), length)
    }

    /// The vector over text set in a frame `size` wide and high, in the
    /// frame's own coordinates from its top-left corner: from its left and
    /// bottom edges, as for an item stating no start, `length` (or the
    /// frame's width) long at `angle`. A start the text states is not drawn:
    /// it is in the coordinates of an InDesign frame Schist does not keep,
    /// and import reports it.
    pub fn text_vector(&self, size: (Pt, Pt)) -> (Point, Point, Pt) {
        let (width, height) = size;
        let length = self.length.unwrap_or(width).max(1e-3);
        let radians = self.angle.to_radians();
        (
            Point::new(0.0, height),
            Point::new(radians.cos(), -radians.sin()),
            length,
        )
    }

    /// Where `p`, in path coordinates, falls along the gradient, 0 to 1;
    /// beyond either end the end colour continues.
    pub fn position(&self, p: Point, vector: (Point, Point, Pt)) -> f32 {
        let (start, direction, length) = vector;
        let (dx, dy) = (p.x - start.x, p.y - start.y);
        let t = if self.gradient.radial {
            (dx * dx + dy * dy).sqrt() / length
        } else {
            (dx * direction.x + dy * direction.y) / length
        };
        t.clamp(0.0, 1.0)
    }
}
