//! The embedded backer roll and a link to support Schist.

use super::*;
use gpui::StyledImage as _;

pub(super) fn dialog(cx: &mut Context<Workspace>) -> impl IntoElement {
    let catalog = crate::backers::catalog();
    let mut body = div()
        .flex()
        .flex_col()
        .gap_4()
        .text_size(px(ui::metrics().text))
        .child(t("dialog.support.thanks"));

    for (tier_index, tier) in catalog.tiers.iter().enumerate() {
        if tier.backers.is_empty() {
            continue;
        }
        // Tier and backer names are supplied by the backer service, like
        // document names; the surrounding interface uses the UI locale.
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(schist_ui::Heading::new(tier.name.clone()));
        let mut entries = div().flex().flex_wrap().gap_3();
        for (backer_index, backer) in tier.backers.iter().enumerate() {
            let mut entry = div()
                .id(SharedString::from(format!(
                    "backer-{tier_index}-{backer_index}"
                )))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .p_3()
                .max_w_full()
                .rounded_md()
                .border_1()
                .border_color(gpui::rgb(ui::palette().divider));
            if let Some(logo) = &backer.logo {
                let url = if schist_ui::is_light() {
                    &logo.light
                } else {
                    logo.dark.as_ref().unwrap_or(&logo.light)
                };
                if let Some(image) = crate::backers::logo(url) {
                    entry = entry.child(
                        gpui::img(image)
                            .w(px(logo.width.clamp(32, 320) as f32))
                            .max_w_full()
                            .max_h(px(160.0))
                            .object_fit(gpui::ObjectFit::Contain)
                            .with_fallback(|| div().into_any_element()),
                    );
                }
            }
            entry = entry.child(backer.name.clone());
            if let Some(url) = &backer.url {
                let url = url.clone();
                entry = entry
                    .cursor_pointer()
                    .text_color(gpui::rgb(ui::palette().accent))
                    .hover(|style| style.bg(gpui::rgb(ui::palette().hover)))
                    .on_click(move |_, _, cx| cx.open_url(&url));
            }
            entries = entries.child(entry);
        }
        section = section.child(entries);
        body = body.child(section);
    }

    let actions = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(ui::button(
            t("dialog.support.title"),
            false,
            move |_, _, cx| cx.open_url(&catalog.support_url),
            cx,
        ))
        .child(ui::button(
            t("common.close"),
            true,
            |ws, _, cx| ws.close_modal(cx),
            cx,
        ));
    ui::modal_frame(t("dialog.support.title"), 520.0, body, actions)
}
