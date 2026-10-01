//! The Swatches panel: the document's inks, as things to fill with.
//!
//! These are the document's own inks, not a user's saved palette. A layout
//! document carries the inks it separates to, and a spot colour that is in
//! the swatch list but not in the document is a plate the prepress stage
//! will not have. So the list is the document's, and a spot ink says so —
//! it needs its own plate, which is a fact with a cost attached.
//!
//! Clicking a swatch fills whatever is selected, which is the whole point:
//! a swatch panel you can only look at is a legend.

use gpui::{div, px, rgb, Context, IntoElement};

use super::*;

/// The size of a swatch chip, in pixels.
///
/// Square, because an ink is a colour and not a quantity. Large enough to
/// tell two similar spot inks apart by eye, which is the one judgement a
/// name cannot make for you.
const CHIP: f32 = 16.0;

/// The swatches panel.
///
/// `None` when there is no document, or it has no inks at all.
pub(super) fn swatches_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() {
        return None;
    }
    let inks = ws.design.document.inks.clone();
    if inks.is_empty() {
        return None;
    }
    let chips = swatch_chips(&inks, &ws.design.document, &ws.design.selection, cx);
    let spots = inks
        .iter()
        .filter(|ink| ink.spot && ink.tint.is_none())
        .count();
    let selected_index = ws
        .design
        .controls
        .swatch
        .filter(|i| *i < inks.len())
        .unwrap_or(0);
    let selected = inks[selected_index].clone();
    let mut fields = Vec::new();
    if let Some(tint) = &selected.tint {
        fields.push(super::design_controls::field(
            ws,
            "design-prop-swatch-tint",
            "design.tint_value",
            format!("{:.2}", tint.value * 100.0),
            crate::design::controls::Target::Swatch(selected.clone()),
            cx,
        ));
    } else if let Some(cmyk) = selected.source_cmyk {
        for (index, id, label) in [
            (0, "design-prop-swatch-cmyk-c", "common.cyan"),
            (1, "design-prop-swatch-cmyk-m", "common.magenta"),
            (2, "design-prop-swatch-cmyk-y", "common.yellow"),
            (3, "design-prop-swatch-cmyk-k", "common.black"),
        ] {
            fields.push(super::design_controls::field(
                ws,
                id,
                label,
                format!("{:.2}", cmyk[index] * 100.0),
                crate::design::controls::Target::Swatch(selected.clone()),
                cx,
            ));
        }
    } else {
        for (index, id, label) in [
            (0, "design-prop-swatch-red", "common.red"),
            (1, "design-prop-swatch-green", "common.green"),
            (2, "design-prop-swatch-blue", "common.blue"),
        ] {
            fields.push(super::design_controls::field(
                ws,
                id,
                label,
                format!("{:.2}", selected.preview_rgb[index] * 255.0),
                crate::design::controls::Target::Swatch(selected.clone()),
                cx,
            ));
        }
    }
    Some(
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_h(px(0.0))
            .p_2()
            .gap_2()
            .border_t_1()
            .border_color(rgb(palette().panel_edge))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .text_xs()
                    .text_color(rgb(palette().text_dim))
                    .child(schist_i18n::tf!("design.ink_count", count = inks.len()))
                    .child(if spots > 0 {
                        schist_i18n::tf!("design.spot_ink_count", count = spots)
                    } else {
                        String::new()
                    })
                    .child(
                        IconButton::new("design-new-tint", "plus")
                            .tooltip(t("design.new_tint"), None)
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                ws.commit_focused_field();
                                let Some(base) =
                                    ws.design.document.inks.get(selected_index).cloned()
                                else {
                                    return;
                                };
                                if let Some(index) = schist_layout::swatches::add_tint(
                                    &mut ws.design.document,
                                    &mut ws.design.history,
                                    &base,
                                    0.5,
                                ) {
                                    ws.design.controls.swatch = Some(index);
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .child(super::design_dock::section(
                ws,
                "swatch-options",
                "design.appearance",
                vec![div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xs().truncate().child(selected.name))
                    .children(fields)
                    .into_any_element()],
                cx,
            ))
            .child(div().flex().flex_col().children(chips))
            .into_any_element(),
    )
}

/// One chip per ink.
fn swatch_chips(
    inks: &[schist_layout::Ink],
    document: &schist_layout::LayoutDocument,
    selection: &[schist_layout::ObjectId],
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    inks.iter()
        .enumerate()
        .map(|(index, ink)| {
            // A swatch is brighter if the selection is already filled with
            // this ink, so the panel answers "what is this shape?" without
            // the user having to look back at the pasteboard.
            let in_use = filled_with(document, selection, ink);
            let colour = rgb(hex_of(ink));
            let mut chip = div()
                // Keyed by position, not by name: two inks can share a
                // name in a document that has not been cleaned up, and two
                // rows with one id is a row that stops responding to
                // clicks.
                .id(("swatch", index))
                .flex()
                .items_center()
                .min_w_0()
                .h(px(26.0))
                .px_1()
                .gap_2()
                .tooltip(ui::tip(ink.name.clone(), None))
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(CHIP))
                        .h(px(CHIP))
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(palette().edge))
                        .bg(colour),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .child(ink.name.clone()),
                );
            if ink.spot {
                // A spot ink is premixed and needs its own plate. Marked
                // on the chip rather than only in the count above, because
                // a user picking an ink is looking at the chip.
                chip = chip.child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette().text_dim))
                        .child(t("design.spot")),
                );
            }
            if in_use {
                chip = chip.bg(rgb(palette().selection_bg));
            }
            chip.hover(|s| s.bg(rgb(palette().hover)))
                .on_click(cx.listener(move |ws, _ev, _window, cx| {
                    ws.commit_focused_field();
                    ws.design.controls.swatch = Some(index);
                    if let Some(chosen) = ws.design.document.inks.get(index).cloned() {
                        schist_layout::authoring::set_fill_ink(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            &ws.design.selection,
                            &chosen,
                        );
                    }
                    cx.notify();
                }))
                .into_any_element()
        })
        .collect()
}

/// An ink's preview colour as the 0xRRGGBB the shell takes.
///
/// Clamped rather than cast, because an ink defined by Lab can have a
/// preview component outside 0..=1, and a cast past the end of a byte wraps
/// round: a black ink showing as white is a bug that looks like a feature.
fn hex_of(ink: &schist_layout::Ink) -> u32 {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    let rgb = ink.preview_at_tint(1.0);
    (channel(rgb[0]) << 16) | (channel(rgb[1]) << 8) | channel(rgb[2])
}

/// Whether an object is filled with an ink.
///
/// Compare complete definitions, since distinct tints or colours can share names.
fn filled_with(
    document: &schist_layout::LayoutDocument,
    selection: &[schist_layout::ObjectId],
    ink: &schist_layout::Ink,
) -> bool {
    selection.iter().any(|id| {
        matches!(
            document.object(*id).map(|placed| &placed.object),
            Some(schist_layout::LayoutObject::Shape { fill: Some(fill), .. }) if fill == ink
        )
    })
}

impl Workspace {
    /// Fill everything selected with a named ink.
    ///
    /// A frame that cannot take a fill — a line, or a frame that is not
    /// selected — is skipped rather than refused, so a mixed selection
    /// still does the half of the work that makes sense.
    pub fn fill_selection_with(&mut self, ink: &str, cx: &mut Context<Self>) {
        let selection = self.design.selection.clone();
        if selection.is_empty() {
            return;
        }
        if schist_layout::authoring::set_fill(
            &mut self.design.document,
            &mut self.design.history,
            &selection,
            ink,
        ) {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(name: &str) -> schist_layout::Ink {
        schist_layout::Ink::process(name, [0.0, 0.0, 0.0])
    }

    #[test]
    fn a_preview_colour_is_clamped_into_a_byte() {
        let out_of_range = schist_layout::Ink {
            name: "Odd".into(),
            lab: [0.0, 0.0, 0.0],
            // Beyond 0..=1 on both ends, which a Lab-specified spot colour
            // can produce.
            preview_rgb: [-1.0, 2.0, 0.5],
            source_cmyk: None,
            spot: true,
            tint: None,
        };
        // Not 0xFFFF7F, which is what an unclamped shift would give.
        assert_eq!(hex_of(&out_of_range), 0x00FF80);
        assert_eq!(hex_of(&ink("Black")), 0x000000);
    }

    #[test]
    fn only_a_swatch_the_selection_is_filled_with_lights_up() {
        // Every swatch lighting up at once reads as "the selection is
        // filled with every ink", which is not a thing.
        let mut document = schist_layout::blank_a4();
        let mut history = schist_layout::History::default();
        let cyan = document.inks[2].name.clone();
        let object = schist_layout::authoring::rectangle(
            &mut document,
            &mut history,
            0,
            schist_layout::Rect::new(0.0, 0.0, 50.0, 50.0),
            schist_layout::authoring::Paint::none(),
        )
        .expect("a shape");
        schist_layout::authoring::set_fill(&mut document, &mut history, &[object], &cyan);
        let selection = vec![object];
        assert!(filled_with(
            &document,
            &selection,
            document.ink(&cyan).unwrap()
        ));
        assert!(!filled_with(&document, &selection, &ink("Magenta")));
        // And an empty selection lights up nothing.
        assert!(!filled_with(&document, &[], document.ink(&cyan).unwrap()));
    }

    #[test]
    fn a_text_frame_lights_up_no_swatch() {
        // A frame has no fill at all, so no swatch is claiming it.
        let document = schist_layout::blank_a4();
        let (mut document, _) = (document, ());
        let mut history = schist_layout::History::default();
        let frame = schist_layout::authoring::text_frame(
            &mut document,
            &mut history,
            0,
            schist_layout::Rect::new(0.0, 0.0, 100.0, 40.0),
        )
        .expect("a frame");
        assert!(!filled_with(&document, &[frame.object], &ink("Cyan")));
    }
}
