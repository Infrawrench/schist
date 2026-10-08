//! Initial counts and character style stay behind a closed Paragraph disclosure.
use super::*;
use crate::design::controls::{initials, Target};

pub(super) fn rows(
    ws: &Workspace,
    name: &str,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let Some(style) = ws.design.document.styles.paragraph(name) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for (id, label, value) in [
        (
            "design-prop-initial-lines",
            "design.initial_lines",
            style.drop_caps_lines,
        ),
        (
            "design-prop-initial-characters",
            "design.initial_characters",
            style.drop_caps_characters,
        ),
    ] {
        rows.push(super::design_controls::field(
            ws,
            id,
            label,
            value.map(|v| v.to_string()).unwrap_or_default(),
            Target::Paragraph(name.to_owned()),
            cx,
        ));
    }
    let target = Target::Paragraph(name.to_owned());
    rows.push(
        div()
            .flex()
            .justify_end()
            .child(
                IconButton::new("design-initial-inherit", "undo")
                    .tooltip(t("design.initial_inherit"), None)
                    .disabled(
                        style.drop_caps_lines.is_none()
                            && style.drop_caps_characters.is_none()
                            && style.drop_caps_detail.is_none(),
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.commit_focused_field();
                        initials::inherit(&mut ws.design, &target);
                        cx.notify();
                    })),
            )
            .into_any_element(),
    );
    use initials::CharacterChoice;
    use schist_layout::nested_styles::CharacterStyle;
    let current = initials::character_choice(style);
    let mut options = vec![
        (
            t("design.initial_style_inherit").to_string().into(),
            CharacterChoice::Inherit,
        ),
        (
            t("common.none").to_string().into(),
            CharacterChoice::Style(CharacterStyle::None),
        ),
    ];
    options.extend(ws.design.document.styles.characters.iter().map(|style| {
        (
            style.name.clone().into(),
            CharacterChoice::Style(CharacterStyle::Named(style.name.clone())),
        )
    }));
    let label = match &current {
        CharacterChoice::Inherit => t("design.inherited").to_string(),
        CharacterChoice::Style(CharacterStyle::None) => t("common.none").to_string(),
        CharacterChoice::Style(CharacterStyle::Named(name))
            if ws.design.document.styles.character(name).is_some() =>
        {
            name.clone()
        }
        CharacterChoice::Style(CharacterStyle::Named(name) | CharacterStyle::Unresolved(name)) => {
            schist_i18n::tf!("design.initial_style_missing", name = name)
        }
    };
    // Retained missing references remain visible and can be replaced. Choosing
    // the same unavailable entry is a no-op, never a new unresolved edit.
    if !options.iter().any(|(_, value)| *value == current) {
        options.push((label.clone().into(), current.clone()));
    }
    let popup = Popup::Field("design-initial-style");
    let target = Target::Paragraph(name.to_owned());
    rows.push(
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .child(
                div()
                    .text_xs()
                    .flex_1()
                    .min_w_0()
                    .child(t("design.initial_style")),
            )
            .child(ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup,
                    is_open: ws.open_popup == Some(popup),
                    current,
                    label: label.into(),
                    width: 146.0,
                    options,
                },
                move |ws, choice, cx| {
                    ws.commit_focused_field();
                    initials::set_character_choice(&mut ws.design, &target, choice);
                    cx.notify();
                },
                cx,
            ))
            .into_any_element(),
    );
    rows
}
