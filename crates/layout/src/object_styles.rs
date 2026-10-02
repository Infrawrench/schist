//! Object paint inheritance. Unset properties inherit; an explicit no-ink
//! paint clears a base colour. Disabled style categories contribute nothing.
use serde::{Deserialize, Serialize};

use crate::{Ink, LayoutObject, PaintTints, PlacedObject, ShapePath, StyleSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    None,
    Ink(Ink),
}

impl Paint {
    pub fn ink(&self) -> Option<&Ink> {
        match self {
            Self::None => None,
            Self::Ink(ink) => Some(ink),
        }
    }
}

/// Local values or style properties. Tint is a fraction; absent means inherit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectPaint {
    pub fill: Option<Paint>,
    pub stroke: Option<Paint>,
    pub stroke_width: Option<f32>,
    pub fill_tint: Option<f32>,
    pub stroke_tint: Option<f32>,
    pub overprint_fill: Option<bool>,
    pub overprint_stroke: Option<bool>,
}

impl ObjectPaint {
    pub fn concrete(mut self) -> Self {
        self.fill.get_or_insert(Paint::None);
        self.stroke.get_or_insert(Paint::None);
        self.stroke_width.get_or_insert(0.0);
        self.fill_tint.get_or_insert(1.0);
        self.stroke_tint.get_or_insert(1.0);
        self.overprint_fill.get_or_insert(false);
        self.overprint_stroke.get_or_insert(false);
        self
    }
    /// Overlay one level, retaining named-tint versus direct-tint semantics.
    pub fn over(&self, base: &Self) -> Self {
        let paint = |local: &Option<Paint>, tint: Option<f32>, fallback: &Option<Paint>| {
            local.clone().or_else(|| {
                fallback.as_ref().map(|paint| match paint {
                    Paint::Ink(ink) if tint.is_some() => Paint::Ink(ink.base_color().into_owned()),
                    _ => paint.clone(),
                })
            })
        };
        Self {
            fill: paint(&self.fill, self.fill_tint, &base.fill),
            stroke: paint(&self.stroke, self.stroke_tint, &base.stroke),
            stroke_width: self.stroke_width.or(base.stroke_width),
            fill_tint: self.fill_tint.or(base.fill_tint),
            stroke_tint: self.stroke_tint.or(base.stroke_tint),
            overprint_fill: self.overprint_fill.or(base.overprint_fill),
            overprint_stroke: self.overprint_stroke.or(base.overprint_stroke),
        }
    }

    pub fn fill_ink(&self) -> Option<&Ink> {
        self.fill.as_ref().and_then(Paint::ink)
    }
    pub fn stroke_ink(&self) -> Option<&Ink> {
        self.stroke.as_ref().and_then(Paint::ink)
    }

    pub fn shape(&self, path: ShapePath) -> LayoutObject {
        LayoutObject::Shape {
            path,
            fill: self.fill_ink().cloned(),
            stroke: self.stroke_ink().cloned(),
            stroke_width: self.stroke_width.unwrap_or(0.0),
            fill_overprint: self.overprint_fill.unwrap_or(false),
            stroke_overprint: self.overprint_stroke.unwrap_or(false),
            tints: PaintTints {
                fill: self.fill_tint.unwrap_or(1.0),
                stroke: self.stroke_tint.unwrap_or(1.0),
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectStyle {
    pub name: String,
    pub based_on: Option<String>,
    pub enable_fill: Option<bool>,
    pub enable_stroke: Option<bool>,
    pub enable_stroke_options: Option<bool>,
    pub enable_footnotes: Option<bool>,
    pub enable_text_frame_general: Option<bool>,
    pub balance_columns: Option<bool>,
    pub footnotes: crate::footnotes::FrameFootnotes,
    pub paint: ObjectPaint,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectAppearance {
    pub style: Option<String>,
    /// Inline paint overrides. Legacy unstyled shapes keep their original
    /// fields. Styled shapes use this paint exclusively; authoring/import
    /// neutralize the legacy fields when attaching a style.
    pub paint: ObjectPaint,
    /// Text-frame outline in normalized coordinates, independent of text flow.
    /// Graphic frames use their existing clip_path instead.
    pub outline: Option<ShapePath>,
}

impl StyleSet {
    pub fn object_style(&self, name: &str) -> Option<&ObjectStyle> {
        self.objects.iter().find(|s| s.name == name)
    }

    pub fn resolve_object(&self, name: &str) -> ObjectStyle {
        let mut chain = Vec::new();
        let mut next = self.object_style(name);
        while let Some(style) = next {
            if chain.iter().any(|s: &&ObjectStyle| s.name == style.name) {
                break;
            }
            chain.push(style);
            next = style
                .based_on
                .as_deref()
                .and_then(|name| self.object_style(name));
        }
        let mut resolved = ObjectStyle::default();
        for style in chain.into_iter().rev() {
            resolved.paint = style.paint.over(&resolved.paint);
            resolved.enable_fill = style.enable_fill.or(resolved.enable_fill);
            resolved.enable_stroke = style.enable_stroke.or(resolved.enable_stroke);
            resolved.enable_stroke_options = style
                .enable_stroke_options
                .or(resolved.enable_stroke_options);
            resolved.enable_text_frame_general = style
                .enable_text_frame_general
                .or(resolved.enable_text_frame_general);
            resolved.balance_columns = style.balance_columns.or(resolved.balance_columns);
            resolved.enable_footnotes = style.enable_footnotes.or(resolved.enable_footnotes);
            resolved.footnotes = style.footnotes.over(&resolved.footnotes);
        }
        resolved
    }

    pub fn frame_balance(&self, object: &PlacedObject) -> bool {
        let LayoutObject::TextFrame {
            balance_columns, ..
        } = &object.object
        else {
            return false;
        };
        balance_columns
            .or_else(|| {
                object
                    .appearance
                    .style
                    .as_deref()
                    .map(|name| self.resolve_object(name))
                    .filter(|style| style.enable_text_frame_general == Some(true))
                    .and_then(|style| style.balance_columns)
            })
            .unwrap_or(false)
    }

    pub fn frame_footnotes(&self, object: &PlacedObject) -> crate::footnotes::FrameFootnotes {
        let LayoutObject::TextFrame { footnotes, .. } = &object.object else {
            return Default::default();
        };
        let inherited = object
            .appearance
            .style
            .as_deref()
            .map(|name| self.resolve_object(name))
            .filter(|style| style.enable_footnotes == Some(true))
            .map(|style| style.footnotes)
            .unwrap_or_default();
        footnotes.over(&inherited)
    }

    pub fn object_paint(&self, object: &PlacedObject) -> ObjectPaint {
        let mut inherited = object
            .appearance
            .style
            .as_deref()
            .map(|name| self.resolve_object(name))
            .unwrap_or_default();
        if inherited.enable_fill != Some(true) {
            inherited.paint.fill = None;
            inherited.paint.fill_tint = None;
            inherited.paint.overprint_fill = None;
        }
        if inherited.enable_stroke != Some(true) {
            inherited.paint.stroke = None;
            inherited.paint.stroke_tint = None;
            inherited.paint.overprint_stroke = None;
            // Weight belongs to the stroke category; stroke/corner options
            // govern joins, caps and corner effects (not yet represented).
            inherited.paint.stroke_width = None;
        }
        let fallback = if object.appearance.style.is_some() {
            ObjectPaint::default()
        } else {
            object.legacy_paint()
        };
        object
            .appearance
            .paint
            .over(&inherited.paint.over(&fallback))
            .concrete()
    }
}

impl PlacedObject {
    pub fn supports_paint(&self) -> bool {
        matches!(
            self.object,
            LayoutObject::Shape { .. }
                | LayoutObject::TextFrame { .. }
                | LayoutObject::GraphicFrame { .. }
        )
    }

    /// Set local properties without flattening any other inherited property.
    pub fn set_local_paint(&mut self, paint: &ObjectPaint) {
        if self.appearance.style.is_none() && self.appearance.paint == ObjectPaint::default() {
            if let LayoutObject::Shape { path, .. } = &self.object {
                self.object = paint.over(&self.legacy_paint()).shape(path.clone());
                return;
            }
        }
        self.appearance.paint = paint.over(&self.appearance.paint);
    }

    pub fn legacy_paint(&self) -> ObjectPaint {
        match &self.object {
            LayoutObject::Shape {
                fill,
                stroke,
                stroke_width,
                fill_overprint,
                stroke_overprint,
                tints,
                ..
            } => ObjectPaint {
                fill: Some(fill.clone().map(Paint::Ink).unwrap_or(Paint::None)),
                stroke: Some(stroke.clone().map(Paint::Ink).unwrap_or(Paint::None)),
                stroke_width: Some(*stroke_width),
                fill_tint: Some(tints.fill),
                stroke_tint: Some(tints.stroke),
                overprint_fill: Some(*fill_overprint),
                overprint_stroke: Some(*stroke_overprint),
            },
            _ => ObjectPaint::default(),
        }
    }

    /// Paint-ready copy; the document retains inheritance and local overrides.
    pub fn resolved_appearance(&self, styles: &StyleSet) -> Self {
        let mut out = self.clone();
        let paint = styles.object_paint(self);
        if let LayoutObject::Shape { path, .. } = &self.object {
            out.object = paint.shape(path.clone());
        }
        out.appearance.paint = paint;
        out.appearance.style = None;
        out
    }

    /// Frame fill is painted before content; its stroke is painted afterwards.
    /// Call on a paint-ready object returned by page_objects/page_artwork.
    pub fn frame_paint(&self, stroke: bool) -> Option<Self> {
        let outline = match &self.object {
            LayoutObject::TextFrame { .. } => self.appearance.outline.as_ref(),
            LayoutObject::GraphicFrame { clip_path, .. } => clip_path.as_ref(),
            _ => return None,
        };
        let mut paint = self.appearance.paint.clone();
        if stroke {
            paint.stroke_ink()?;
            if paint.stroke_width.unwrap_or(0.0) <= 0.0 {
                return None;
            }
            paint.fill = None;
        } else {
            paint.fill_ink()?;
            paint.stroke = None;
        }
        let baseline = match &self.object {
            LayoutObject::TextFrame {
                text_path: Some(path),
                ..
            } => Some(&path.path),
            _ => None,
        };
        let mut path = baseline.or(outline).cloned().unwrap_or_else(|| {
            crate::authoring::path_for(crate::authoring::ShapeKind::Rectangle, 1.0, 1.0)
        });
        if baseline.is_none() {
            path.map_points(|p| {
                crate::Point::new(p.x * self.bounds.width, p.y * self.bounds.height)
            });
        }
        let mut out = self.clone();
        out.object = paint.shape(path);
        out.appearance = ObjectAppearance::default();
        Some(out)
    }
}

/// A selection-wide style change is one history entry, independent of its size.
/// Applying clears overrides in enabled categories; disabled categories retain
/// their current appearance. Detaching bakes paint and frame footnote settings.
pub fn apply_style(
    doc: &mut crate::LayoutDocument,
    history: &mut crate::History,
    ids: &[crate::ObjectId],
    name: Option<&str>,
) -> bool {
    if name.is_some_and(|name| doc.styles.object_style(name).is_none()) {
        return false;
    }
    let resolved = name.map(|name| doc.styles.resolve_object(name));
    let mut edits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !seen.insert(*id) || doc.object_locked(*id) {
            continue;
        }
        let Some(object) = doc.object(*id).filter(|o| o.supports_paint()) else {
            continue;
        };
        let mut after = object.clone();
        let mut paint = doc.styles.object_paint(object);
        if let LayoutObject::TextFrame {
            footnotes,
            balance_columns,
            ..
        } = &mut after.object
        {
            *balance_columns = if resolved
                .as_ref()
                .is_some_and(|s| s.enable_text_frame_general == Some(true))
            {
                None
            } else {
                Some(doc.styles.frame_balance(object))
            };
            *footnotes = if resolved
                .as_ref()
                .is_some_and(|s| s.enable_footnotes == Some(true))
            {
                Default::default()
            } else {
                doc.styles.frame_footnotes(object)
            };
        }
        if let Some(style) = &resolved {
            if style.enable_fill == Some(true) {
                paint.fill = None;
                paint.fill_tint = None;
                paint.overprint_fill = None;
            }
            if style.enable_stroke == Some(true) {
                paint.stroke = None;
                paint.stroke_tint = None;
                paint.overprint_stroke = None;
                paint.stroke_width = None;
            }
        }
        after.appearance.style = name.map(str::to_owned);
        after.appearance.paint = paint.clone();
        if let LayoutObject::Shape { path, .. } = &object.object {
            if name.is_none() {
                after.object = paint.shape(path.clone());
                after.appearance.paint = ObjectPaint::default();
            } else {
                after.object = ObjectPaint::default().shape(path.clone());
            }
        }
        if after != *object {
            edits.push(crate::LayoutEdit::ObjectChanged {
                id: id.0,
                before: crate::snapshot_object(object),
                after: crate::snapshot_object(&after),
            });
        }
    }
    !edits.is_empty() && history.apply(doc, crate::LayoutEdit::Batch { edits })
}

/// Explicit local fill/stroke edits share a single selection-wide undo step.
pub fn edit_paint(
    doc: &mut crate::LayoutDocument,
    history: &mut crate::History,
    ids: &[crate::ObjectId],
    paint: &ObjectPaint,
) -> bool {
    let mut edits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !seen.insert(*id) || doc.object_locked(*id) {
            continue;
        }
        let Some(object) = doc.object(*id).filter(|o| o.supports_paint()) else {
            continue;
        };
        let mut after = object.clone();
        after.set_local_paint(paint);
        if after != *object {
            edits.push(crate::LayoutEdit::ObjectChanged {
                id: id.0,
                before: crate::snapshot_object(object),
                after: crate::snapshot_object(&after),
            });
        }
    }
    !edits.is_empty() && history.apply(doc, crate::LayoutEdit::Batch { edits })
}

/// Rename the definition and every ordinary/parent reference in one edit.
pub fn rename_style(
    doc: &mut crate::LayoutDocument,
    history: &mut crate::History,
    old: &str,
    new: &str,
) -> bool {
    let new = new.trim();
    if new.is_empty()
        || new == old
        || new.chars().any(char::is_control)
        || doc.styles.object_style(old).is_none()
        || doc.styles.object_style(new).is_some()
    {
        return false;
    }
    let mut styles = doc.styles.clone();
    for style in &mut styles.objects {
        if style.name == old {
            style.name = new.into();
        }
        if style.based_on.as_deref() == Some(old) {
            style.based_on = Some(new.into());
        }
    }
    let mut edits = vec![crate::LayoutEdit::StylesChanged {
        before: Box::new(doc.styles.clone()),
        after: Box::new(styles),
    }];
    let update = |object: &mut PlacedObject| {
        if object.appearance.style.as_deref() == Some(old) {
            object.appearance.style = Some(new.into());
        }
    };
    for object in &doc.objects {
        let mut after = object.clone();
        update(&mut after);
        if after != *object {
            edits.push(crate::LayoutEdit::ObjectChanged {
                id: object.id.0,
                before: crate::snapshot_object(object),
                after: crate::snapshot_object(&after),
            });
        }
    }
    let before = crate::structure::Topology::of(doc);
    let mut after = before.clone();
    for parent in &mut after.parents {
        for object in &mut parent.objects {
            update(&mut object.object);
        }
    }
    if after != before {
        edits.push(crate::LayoutEdit::TopologyChanged {
            before: Box::new(before),
            after: Box::new(after),
        });
    }
    history.apply(doc, crate::LayoutEdit::Batch { edits })
}
