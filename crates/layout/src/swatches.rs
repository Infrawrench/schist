//! Swatch edits update every use, including parent artwork, in one undo step.
use crate::{History, Ink, LayoutDocument, LayoutEdit, LayoutObject};

pub fn add_tint(
    doc: &mut LayoutDocument,
    history: &mut History,
    base: &Ink,
    value: f32,
) -> Option<usize> {
    if !doc.inks.contains(base) {
        return None;
    }
    let title = format!("{} {}%", base.base_color().name, value * 100.0);
    let mut name = title.clone();
    let mut suffix = 2;
    while doc.inks.iter().any(|ink| ink.name == name) {
        name = format!("{title} ({suffix})");
        suffix += 1;
    }
    let tint = base.named_tint(name, value)?;
    let before = doc.inks.clone();
    let index = before.len();
    let mut after = before.clone();
    after.push(tint);
    history
        .apply(doc, LayoutEdit::SwatchesChanged { before, after })
        .then_some(index)
}

/// Replace a captured swatch definition. Unrelated same-named inline colours
/// remain distinct. A base edit also updates every named tint of that definition.
/// Resource order, style inheritance and all affected uses undo together.
pub fn replace(doc: &mut LayoutDocument, history: &mut History, before: &Ink, after: Ink) -> bool {
    if !doc.inks.contains(before)
        || before == &after
        || (before.name != after.name && before.tint.is_none())
        || before.tint.is_some() != after.tint.is_some()
        || !after
            .lab
            .iter()
            .chain(&after.preview_rgb)
            .chain(after.source_cmyk.iter().flatten())
            .all(|v| v.is_finite())
        || after
            .tint
            .as_ref()
            .is_some_and(|t| !t.value.is_finite() || !(0.0..=1.0).contains(&t.value))
    {
        return false;
    }
    let update = |ink: &mut Ink| {
        if ink == before {
            *ink = after.clone();
        } else if before.tint.is_none() && ink.tint.is_some() && ink.base_color().as_ref() == before
        {
            *ink = after
                .named_tint(ink.name.clone(), ink.tint_amount())
                .expect("bounded tint");
        }
    };
    let paint = |paint: &mut Option<Ink>| {
        if let Some(ink) = paint {
            update(ink);
        }
    };
    let object_paint = |p: &mut crate::ObjectPaint| {
        for paint in [&mut p.fill, &mut p.stroke] {
            if let Some(crate::Paint::Ink(ink)) = paint {
                update(ink);
            }
        }
    };
    let object = |object: &mut crate::PlacedObject| {
        object_paint(&mut object.appearance.paint);
        let object = &mut object.object;
        if let LayoutObject::Shape { fill, stroke, .. } = object {
            paint(fill);
            paint(stroke);
        }
    };
    let mut inks = doc.inks.clone();
    for ink in &mut inks {
        update(ink);
    }
    let mut edits = vec![LayoutEdit::SwatchesChanged {
        before: doc.inks.clone(),
        after: inks,
    }];
    let before_settings = crate::snapshot_settings(doc);
    let mut after_settings = before_settings.clone();
    for rule in [
        &mut after_settings.footnotes.rule,
        &mut after_settings.footnotes.continuing_rule,
    ] {
        for paint in [&mut rule.paint, &mut rule.gap_paint] {
            if let Some(crate::footnotes::FootnoteReference::Resolved(ink)) = paint {
                update(ink);
            }
        }
    }
    if before_settings != after_settings {
        edits.push(LayoutEdit::DocumentChanged {
            before: Box::new(before_settings),
            after: Box::new(after_settings),
        });
    }
    let mut styles = doc.styles.clone();
    for style in &mut styles.paragraphs {
        paint(&mut style.fill);
        paint(&mut style.stroke);
        for decoration in [&mut style.underline_style, &mut style.strike_style] {
            for paint in [&mut decoration.paint, &mut decoration.gap_paint] {
                if let Some(crate::decorations::DecorationPaint::Ink(ink)) = paint {
                    update(ink);
                }
            }
        }
    }
    for style in &mut styles.characters {
        paint(&mut style.fill);
        paint(&mut style.stroke);
        for decoration in [&mut style.underline_style, &mut style.strike_style] {
            for paint in [&mut decoration.paint, &mut decoration.gap_paint] {
                if let Some(crate::decorations::DecorationPaint::Ink(ink)) = paint {
                    update(ink);
                }
            }
        }
    }
    for style in &mut styles.objects {
        object_paint(&mut style.paint);
    }
    if styles != doc.styles {
        edits.push(LayoutEdit::StylesChanged {
            before: Box::new(doc.styles.clone()),
            after: Box::new(styles),
        });
    }
    for placed in &doc.objects {
        let mut changed = placed.clone();
        object(&mut changed);
        if changed != *placed {
            edits.push(LayoutEdit::ObjectChanged {
                id: placed.id.0,
                before: crate::snapshot_object(placed),
                after: crate::snapshot_object(&changed),
            });
        }
    }
    let before = crate::structure::Topology::of(doc);
    let mut after = before.clone();
    for parent in &mut after.parents {
        for placed in &mut parent.objects {
            object(&mut placed.object);
        }
    }
    if before != after {
        edits.push(LayoutEdit::TopologyChanged {
            before: Box::new(before),
            after: Box::new(after),
        });
    }
    history.apply(doc, LayoutEdit::Batch { edits })
}

/// Edit a tint and refresh names generated by add_tint. Imported custom names
/// remain intact. Choosing an already-used name adds a disambiguating suffix.
pub fn set_tint(doc: &mut LayoutDocument, history: &mut History, ink: &Ink, value: f32) -> bool {
    let Some(tint) = &ink.tint else {
        return false;
    };
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return false;
    }
    let mut after = ink.clone();
    after.tint.as_mut().unwrap().value = value;
    let generated = format!("{} {}%", tint.base_name, tint.value * 100.0);
    if ink.name == generated
        || ink
            .name
            .strip_prefix(&generated)
            .is_some_and(|suffix| suffix.starts_with(" (") && suffix.ends_with(')'))
    {
        let title = format!("{} {}%", tint.base_name, value * 100.0);
        after.name.clone_from(&title);
        let mut suffix = 2;
        while doc
            .inks
            .iter()
            .any(|other| other != ink && other.name == after.name)
        {
            after.name = format!("{title} ({suffix})");
            suffix += 1;
        }
    }
    replace(doc, history, ink, after)
}
