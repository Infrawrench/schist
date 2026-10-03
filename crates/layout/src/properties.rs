//! Undoable control-panel edits. A committed value is one gesture, even
//! when it changes several frames. Live field typing does not edit a file.
use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageProperty {
    Width,
    Height,
    Bleed,
    Slug,
    BleedTop,
    BleedBottom,
    BleedInside,
    BleedOutside,
    SlugTop,
    SlugBottom,
    SlugInside,
    SlugOutside,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
}
impl PageProperty {
    /// Inside/outside follow the spine on facing pages, left/right otherwise.
    fn physical(self, doc: &LayoutDocument, page: usize) -> Self {
        if doc.facing_pages && doc.page_is_left(page) {
            match self {
                Self::BleedInside => Self::BleedOutside,
                Self::BleedOutside => Self::BleedInside,
                Self::SlugInside => Self::SlugOutside,
                Self::SlugOutside => Self::SlugInside,
                property => property,
            }
        } else {
            self
        }
    }

    pub fn value_for(self, doc: &LayoutDocument, page: usize) -> f32 {
        self.physical(doc, page).value(&doc.pages[page])
    }

    pub fn value(self, page: &Page) -> f32 {
        match self {
            Self::Width => page.width,
            Self::Height => page.height,
            Self::Bleed | Self::BleedTop => page.bleed.top,
            Self::Slug | Self::SlugTop => page.slug.top,
            Self::BleedBottom => page.bleed.bottom,
            Self::BleedInside => page.bleed.left,
            Self::BleedOutside => page.bleed.right,
            Self::SlugBottom => page.slug.bottom,
            Self::SlugInside => page.slug.left,
            Self::SlugOutside => page.slug.right,
            Self::MarginTop => page.margins.top,
            Self::MarginRight => page.margins.right,
            Self::MarginBottom => page.margins.bottom,
            Self::MarginLeft => page.margins.left,
        }
    }
}

/// One field commit changes all addressed pages in one undo step. Object
/// coordinates remain page-local; changing trim does not resize artwork.
pub fn set_page_property(
    doc: &mut LayoutDocument,
    history: &mut History,
    pages: &[usize],
    property: PageProperty,
    value: f32,
) -> bool {
    if !value.is_finite()
        || value < 0.0
        || (matches!(property, PageProperty::Width | PageProperty::Height) && value < 1.0)
    {
        return false;
    }
    let mut edits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for index in pages {
        if !seen.insert(*index) {
            continue;
        }
        let Some(page) = doc.pages.get(*index) else {
            return false;
        };
        let before = snapshot_page(page);
        let mut after = before.clone();
        match property.physical(doc, *index) {
            PageProperty::Width => after.width = value,
            PageProperty::Height => after.height = value,
            PageProperty::Bleed => after.bleed = value.into(),
            PageProperty::Slug => after.slug = value.into(),
            PageProperty::BleedTop => after.bleed.top = value,
            PageProperty::BleedBottom => after.bleed.bottom = value,
            PageProperty::BleedInside => after.bleed.left = value,
            PageProperty::BleedOutside => after.bleed.right = value,
            PageProperty::SlugTop => after.slug.top = value,
            PageProperty::SlugBottom => after.slug.bottom = value,
            PageProperty::SlugInside => after.slug.left = value,
            PageProperty::SlugOutside => after.slug.right = value,
            PageProperty::MarginTop => after.margins[0] = value,
            PageProperty::MarginRight => after.margins[1] = value,
            PageProperty::MarginBottom => after.margins[2] = value,
            PageProperty::MarginLeft => after.margins[3] = value,
        }
        after.landscape = after.width > after.height;
        if before != after {
            edits.push(LayoutEdit::PageChanged {
                index: *index,
                before,
                after,
            });
        }
    }
    !edits.is_empty() && history.apply(doc, LayoutEdit::Batch { edits })
}

/// Document settings also participate in content-based save tracking.
pub fn edit_settings(
    doc: &mut LayoutDocument,
    history: &mut History,
    edit: impl FnOnce(&mut history::SettingsSnapshot),
) -> bool {
    let before = snapshot_settings(doc);
    let mut after = before.clone();
    edit(&mut after);
    before != after
        && history.apply(
            doc,
            LayoutEdit::DocumentChanged {
                before: Box::new(before),
                after: Box::new(after),
            },
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectProperty {
    X,
    Y,
    Width,
    Height,
    Columns,
    Gutter,
    Inset,
    FillTint,
    StrokeTint,
    StrokeWidth,
}

impl ObjectProperty {
    pub fn value(self, object: &PlacedObject) -> Option<f32> {
        match self {
            Self::StrokeWidth => object.supports_paint().then(|| {
                object
                    .appearance
                    .paint
                    .over(&object.legacy_paint())
                    .stroke_width
                    .unwrap_or(0.0)
            }),
            Self::FillTint | Self::StrokeTint => {
                if !object.supports_paint() {
                    return None;
                }
                let paint = object.appearance.paint.over(&object.legacy_paint());
                let (ink, tint) = if self == Self::FillTint {
                    (paint.fill_ink(), paint.fill_tint)
                } else {
                    (paint.stroke_ink(), paint.stroke_tint)
                };
                Some(
                    100.0
                        * ink
                            .filter(|ink| ink.tint.is_some())
                            .map_or(tint.unwrap_or(1.0), |ink| ink.tint_amount()),
                )
            }
            Self::X => Some(object.bounds.x),
            Self::Y => Some(object.bounds.y),
            Self::Width => Some(object.bounds.width),
            Self::Height => Some(object.bounds.height),
            Self::Columns | Self::Gutter | Self::Inset => match &object.object {
                LayoutObject::TextFrame {
                    columns,
                    gutter,
                    insets,
                    text_path: None,
                    ..
                } => match self {
                    Self::Columns => Some(*columns as f32),
                    Self::Gutter => Some(*gutter),
                    _ => Some(insets.top),
                },
                _ => None,
            },
        }
    }
}

pub fn set_object_property(
    doc: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    property: ObjectProperty,
    value: f32,
) -> bool {
    if !value.is_finite() || ids.is_empty() || ids.iter().any(|id| doc.object_locked(*id)) {
        return false;
    }
    if matches!(property, ObjectProperty::Width | ObjectProperty::Height) && value <= 0.0 {
        return false;
    }
    if matches!(
        property,
        ObjectProperty::Gutter | ObjectProperty::Inset | ObjectProperty::StrokeWidth
    ) && value < 0.0
    {
        return false;
    }
    if matches!(
        property,
        ObjectProperty::FillTint | ObjectProperty::StrokeTint
    ) && !(0.0..=100.0).contains(&value)
    {
        return false;
    }
    if property == ObjectProperty::Columns
        && (!(1.0..=100.0).contains(&value) || value.fract() != 0.0)
    {
        return false;
    }
    let mut edits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !seen.insert(*id) {
            continue;
        }
        let Some(object) = doc.object(*id) else {
            return false;
        };
        if property.value(object).is_none() {
            return false;
        }
        if property.value(&object.resolved_appearance(&doc.styles)) == Some(value) {
            continue;
        }
        let mut changed = object.clone();
        match property {
            ObjectProperty::StrokeWidth => changed.set_local_paint(&crate::ObjectPaint {
                stroke_width: Some(value),
                ..Default::default()
            }),
            ObjectProperty::FillTint | ObjectProperty::StrokeTint => {
                let current = doc.styles.object_paint(object);
                let mut paint = crate::ObjectPaint::default();
                if property == ObjectProperty::FillTint {
                    paint.fill_tint = Some(value / 100.0);
                    paint.fill = current
                        .fill_ink()
                        .map(|ink| crate::Paint::Ink(ink.base_color().into_owned()));
                } else {
                    paint.stroke_tint = Some(value / 100.0);
                    paint.stroke = current
                        .stroke_ink()
                        .map(|ink| crate::Paint::Ink(ink.base_color().into_owned()));
                }
                changed.set_local_paint(&paint);
            }
            ObjectProperty::X => changed.bounds.x = value,
            ObjectProperty::Y => changed.bounds.y = value,
            ObjectProperty::Width | ObjectProperty::Height => {
                let width = property == ObjectProperty::Width;
                let old = if width {
                    object.bounds.width
                } else {
                    object.bounds.height
                };
                let path = match &mut changed.object {
                    LayoutObject::Shape { path, .. } => Some(path),
                    LayoutObject::TextFrame {
                        text_path: Some(path),
                        ..
                    } => Some(&mut path.path),
                    _ => None,
                };
                if let Some(path) = path {
                    if old <= 0.0 {
                        return false;
                    }
                    path.map_points(|mut point| {
                        if width {
                            point.x *= value / old;
                        } else {
                            point.y *= value / old;
                        }
                        point
                    });
                }
                if width {
                    changed.bounds.width = value;
                } else {
                    changed.bounds.height = value;
                }
            }
            ObjectProperty::Columns | ObjectProperty::Gutter | ObjectProperty::Inset => {
                let LayoutObject::TextFrame {
                    columns,
                    gutter,
                    insets,
                    ..
                } = &mut changed.object
                else {
                    return false;
                };
                match property {
                    ObjectProperty::Columns => *columns = value as u16,
                    ObjectProperty::Gutter => *gutter = value,
                    _ => *insets = Insets::uniform(value),
                }
            }
        }
        if changed != *object {
            edits.push(LayoutEdit::ObjectChanged {
                id: id.0,
                before: snapshot_object(object),
                after: snapshot_object(&changed),
            });
        }
    }
    !edits.is_empty() && history.apply(doc, LayoutEdit::Batch { edits })
}

pub fn edit_styles(
    doc: &mut LayoutDocument,
    history: &mut History,
    edit: impl FnOnce(&mut StyleSet),
) -> bool {
    let before = doc.styles.clone();
    let mut after = before.clone();
    edit(&mut after);
    before != after
        && history.apply(
            doc,
            LayoutEdit::StylesChanged {
                before: Box::new(before),
                after: Box::new(after),
            },
        )
}

/// Rename a definition and its typed model references as a single operation.
/// Retained opaque native XML is not rewritten.
pub fn rename_style(
    doc: &mut LayoutDocument,
    history: &mut History,
    paragraph: bool,
    old: &str,
    new: &str,
) -> bool {
    let new = new.trim();
    if new.is_empty() || new == old || new.chars().any(char::is_control) {
        return false;
    }
    if if paragraph {
        doc.styles.paragraph(new).is_some() || doc.styles.paragraph(old).is_none()
    } else {
        doc.styles.character(new).is_some() || doc.styles.character(old).is_none()
    } {
        return false;
    }
    let before = doc.styles.clone();
    let mut after = before.clone();
    let rename = |name: &mut String| {
        if name == old {
            *name = new.into();
        }
    };
    if paragraph {
        for style in &mut after.paragraphs {
            rename(&mut style.name);
            if let Some(base) = &mut style.based_on {
                rename(base);
            }
            if let Some(next) = &mut style.next {
                rename(next);
            }
        }
    } else {
        for style in &mut after.paragraphs {
            for rule in style.nested_styles.iter_mut().flatten() {
                if let crate::nested_styles::CharacterStyle::Named(name) = &mut rule.character_style
                {
                    rename(name);
                }
            }
            for marker in [
                &mut style.list.bullet_character_style,
                &mut style.list.numbering_character_style,
            ]
            .into_iter()
            .flatten()
            {
                rename(marker);
            }
        }
        for style in &mut after.characters {
            rename(&mut style.name);
            if let Some(base) = &mut style.based_on {
                rename(base);
            }
        }
    }
    let mut edits = vec![LayoutEdit::StylesChanged {
        before: Box::new(before),
        after: Box::new(after),
    }];
    for (id, story) in doc.stories.iter().enumerate() {
        let before = snapshot_story(story);
        let mut after = before.clone();
        if paragraph {
            for point in &mut after.points {
                if let StoryPointSnapshot::Paragraph { style, .. } = point {
                    rename(style);
                }
            }
        } else {
            for (_, _, style) in &mut after.ranges {
                rename(style);
            }
        }
        for structure in &mut after.structures {
            if let Some(note) = &mut structure.footnote {
                note.rename_style(paragraph, old, new);
            }
        }
        if before != after {
            edits.push(LayoutEdit::StoryChanged {
                id: id as u32,
                before,
                after,
            });
        }
    }
    let before = snapshot_settings(doc);
    let mut after = before.clone();
    if paragraph {
        rename(&mut after.default_paragraph_style);
    } else {
        rename(&mut after.default_character_style);
    }
    let reference = if paragraph {
        &mut after.footnotes.text_style
    } else {
        &mut after.footnotes.marker_style
    };
    if let Some(crate::footnotes::FootnoteReference::Resolved(name)) = reference {
        rename(name);
    }
    if before != after {
        edits.push(LayoutEdit::DocumentChanged {
            before: Box::new(before),
            after: Box::new(after),
        });
    }
    history.apply(doc, LayoutEdit::Batch { edits })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_settings_are_atomic_for_every_property_and_page_count() {
        for count in 1..9 {
            for property in [
                PageProperty::Width,
                PageProperty::Height,
                PageProperty::Bleed,
                PageProperty::Slug,
                PageProperty::MarginTop,
                PageProperty::MarginRight,
                PageProperty::MarginBottom,
                PageProperty::MarginLeft,
            ] {
                let mut doc = blank_a4();
                doc.pages = vec![Page::letter(); count];
                let before = doc.clone();
                let mut history = History::default();
                let indices: Vec<_> = (0..count).collect();
                for value in [-1.0, f32::NAN, f32::INFINITY] {
                    assert!(!set_page_property(
                        &mut doc,
                        &mut history,
                        &indices,
                        property,
                        value
                    ));
                    assert_eq!(doc, before);
                }
                let mut invalid = indices.clone();
                invalid.push(count);
                assert!(!set_page_property(
                    &mut doc,
                    &mut history,
                    &invalid,
                    property,
                    33.5
                ));
                assert_eq!(doc, before);
                assert!(set_page_property(
                    &mut doc,
                    &mut history,
                    &indices,
                    property,
                    33.5
                ));
                assert_eq!(history.undo_depth(), 1);
                assert!(doc.pages.iter().all(|p| property.value(p) == 33.5));
                let after = doc.clone();
                assert!(history.undo(&mut doc));
                assert_eq!(doc, before);
                assert!(history.redo(&mut doc));
                assert_eq!(doc, after);
            }
        }
    }
    #[test]
    fn every_selection_size_changes_in_one_step_and_invalid_values_change_nothing() {
        for count in 1..15 {
            let mut doc = blank_a4();
            let mut history = History::default();
            let ids: Vec<_> = (0..count)
                .map(|_| {
                    authoring::rectangle(
                        &mut doc,
                        &mut history,
                        0,
                        Rect::new(10.0, 20.0, 30.0, 40.0),
                        authoring::Paint::none(),
                    )
                    .unwrap()
                })
                .collect();
            history.clear();
            let before = doc.clone();
            for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                assert!(!set_object_property(
                    &mut doc,
                    &mut history,
                    &ids,
                    ObjectProperty::Width,
                    value
                ));
                assert_eq!(doc, before);
            }
            assert!(set_object_property(
                &mut doc,
                &mut history,
                &ids,
                ObjectProperty::Width,
                90.0
            ));
            assert_eq!(history.undo_depth(), 1);
            for object in &doc.objects {
                let LayoutObject::Shape { path, .. } = &object.object else {
                    panic!()
                };
                assert_eq!(path.bounds().width, 90.0);
            }
            assert!(history.undo(&mut doc));
            assert_eq!(doc, before);
        }
    }
}
