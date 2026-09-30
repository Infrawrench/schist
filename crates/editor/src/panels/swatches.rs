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
const CHIP: f32 = 22.0;

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
    let spots = inks.iter().filter(|ink| ink.spot).count();
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
                    .text_xs()
                    .text_color(rgb(palette().text_dim))
                    .child(schist_i18n::tn!("design.ink_count", inks.len() as u64))
                    .child(if spots > 0 {
                        schist_i18n::tn!("design.spot_ink_count", spots as u64)
                    } else {
                        String::new()
                    }),
            )
            .child(div().flex().flex_wrap().gap_2().children(chips))
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
    inks
        .iter()
        .enumerate()
        .map(|(index, ink)| {
            // A swatch is brighter if the selection is already filled with
            // this ink, so the panel answers "what is this shape?" without
            // the user having to look back at the pasteboard.
            let in_use = filled_with(document, selection, ink);
            let name = ink.name.clone();
            let colour = rgb(hex_of(ink));
            let mut chip = div()
                // Keyed by position, not by name: two inks can share a
                // name in a document that has not been cleaned up, and two
                // rows with one id is a row that stops responding to
                // clicks.
                .id(("swatch", index))
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .w(px(CHIP + 16.0))
                .child(
                    div()
                        .w(px(CHIP))
                        .h(px(CHIP))
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(palette().edge))
                        .bg(colour),
                )
                .child(
                    div()
                        .w(px(CHIP + 16.0))
                        .overflow_hidden()
                        .text_ellipsis()
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
            chip.on_click(cx.listener(move |ws, _ev, _window, cx| {
                ws.fill_selection_with(&name, cx);
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
    (channel(ink.preview_rgb[0]) << 16)
        | (channel(ink.preview_rgb[1]) << 8)
        | channel(ink.preview_rgb[2])
}

/// Whether an object is filled with an ink.
///
/// A name comparison, not a pointer one: an ink loaded by two different
/// routes is one ink, and a swatch that did not light up for a fill it set
/// would be a swatch lying.
fn filled_with(
    document: &schist_layout::LayoutDocument,
    selection: &[schist_layout::ObjectId],
    ink: &schist_layout::Ink,
) -> bool {
    // By name, not by identity: an ink reached by two routes is one ink,
    // and a swatch that failed to light up for a fill it set would be a
    // swatch lying about the selection.
    selection.iter().any(|id| {
        matches!(
            document.object(*id).map(|placed| &placed.object),
            Some(schist_layout::LayoutObject::Shape { fill: Some(fill), .. }) if fill.name == ink.name
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
        assert!(filled_with(&document, &selection, &ink(&cyan)));
        assert!(!filled_with(&document, &selection, &ink("Magenta")));
        // And an empty selection lights up nothing.
        assert!(!filled_with(&document, &[], &ink(&cyan)));
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
