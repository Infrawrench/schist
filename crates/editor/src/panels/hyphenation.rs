//! Automatic line breaking is a closed inspector section, not persistent form rows.
use super::*;
use crate::design::controls::{hyphenation, Target};

pub(super) fn rows(
    ws: &Workspace,
    name: &str,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let Some(style) = ws.design.document.styles.paragraph(name) else {
        return Vec::new();
    };
    let resolved = ws.design.document.styles.resolve_paragraph(name);
    let enabled = resolved.hyphenate.unwrap_or(true);
    let mut actions = div().flex().items_center().gap_1();
    for (index, (flag, glyph, label, own, active)) in [
        (
            hyphenation::Flag::Enabled,
            "type-hyphenate",
            "design.hyphenation",
            style.hyphenate,
            enabled,
        ),
        (
            hyphenation::Flag::Capitals,
            "type",
            "design.hyphen_capitals",
            style.hyphenation.capitalized_words,
            resolved.hyphenation.capitalized_words.unwrap_or(true),
        ),
        (
            hyphenation::Flag::LastWord,
            "type-align-right",
            "design.hyphen_last",
            style.hyphenation.last_word,
            resolved.hyphenation.last_word.unwrap_or(true),
        ),
        (
            hyphenation::Flag::AcrossColumns,
            "type-balance-columns",
            "design.hyphen_columns",
            style.hyphenation.across_columns,
            resolved.hyphenation.across_columns.unwrap_or(true),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let target = Target::Paragraph(name.to_owned());
        actions = actions.child(
            IconButton::new(("design-hyphen-flag", index), glyph)
                .tooltip(
                    t(label),
                    own.is_none().then(|| t("design.inherited").into()),
                )
                .active(active)
                .disabled(index != 0 && !enabled)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    hyphenation::set_flag(&mut ws.design, &target, flag, Some(!active));
                    cx.notify();
                })),
        );
    }
    let target = Target::Paragraph(name.to_owned());
    actions = actions.child(div().flex_1()).child(
        IconButton::new("design-hyphen-inherit", "undo")
            .tooltip(t("design.hyphen_inherit"), None)
            .disabled(style.hyphenate.is_none() && style.hyphenation == Default::default())
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.commit_focused_field();
                hyphenation::inherit(&mut ws.design, &target);
                cx.notify();
            })),
    );
    let mut rows = vec![actions.into_any_element()];
    if enabled {
        rows.push(
            div()
                .text_xs()
                .child(t("design.hyphen_languages"))
                .into_any_element(),
        );
        for (id, label, value) in [
            (
                "design-prop-hyphen-word",
                "design.hyphen_word",
                style.hyphenation.words_longer_than,
            ),
            (
                "design-prop-hyphen-after",
                "design.hyphen_after",
                style.hyphenation.after_first,
            ),
            (
                "design-prop-hyphen-before",
                "design.hyphen_before",
                style.hyphenation.before_last,
            ),
            (
                "design-prop-hyphen-limit",
                "design.hyphen_limit",
                style.hyphenation.ladder_limit,
            ),
            (
                "design-prop-hyphen-weight",
                "design.hyphen_weight",
                style.hyphenation.weight,
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
        rows.push(super::design_controls::field(
            ws,
            "design-prop-hyphen-zone",
            "design.hyphen_zone",
            style
                .hyphenation
                .zone
                .map(|v| format!("{v:.3}"))
                .unwrap_or_default(),
            Target::Paragraph(name.to_owned()),
            cx,
        ));
    }
    rows
}
