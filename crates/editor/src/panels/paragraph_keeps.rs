//! Keep policies stay behind a disclosure; only active line counts need fields.
use super::*;
use crate::design::controls::{keeps, Target};

pub(super) fn rows(
    ws: &Workspace,
    name: &str,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let Some(style) = ws.design.document.styles.paragraph(name) else {
        return Vec::new();
    };
    let local = keeps::local(style);
    let start = style.start_paragraph;
    let resolved = ws.design.document.styles.resolve_paragraph(name).keeps;
    let enabled = resolved.enabled == Some(true);
    let mut actions = div().flex().items_center().gap_1();
    for (index, (flag, glyph, label, own, active, disabled)) in [
        (
            keeps::Flag::Enabled,
            "type-keep-lines",
            "design.keep_lines",
            local.enabled,
            enabled,
            false,
        ),
        (
            keeps::Flag::All,
            "type-keep-all",
            "design.keep_all",
            local.all,
            resolved.all == Some(true),
            !enabled,
        ),
        (
            keeps::Flag::Previous,
            "type-keep-previous",
            "design.keep_previous",
            local.previous,
            resolved.previous == Some(true),
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let target = Target::Paragraph(name.to_owned());
        actions = actions.child(
            IconButton::new(("design-keep-flag", index), glyph)
                .tooltip(
                    t(label),
                    own.is_none().then(|| t("design.inherited").into()),
                )
                .active(active)
                .disabled(disabled)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    keeps::set_flag(&mut ws.design, &target, flag, Some(!active));
                    cx.notify();
                })),
        );
    }
    let target = Target::Paragraph(name.to_owned());
    actions = actions.child(div().flex_1()).child(
        IconButton::new("design-keep-inherit", "undo")
            .tooltip(t("design.keep_inherit"), None)
            .disabled(local == Default::default() && start.is_none())
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.commit_focused_field();
                keeps::inherit(&mut ws.design, &target);
                cx.notify();
            })),
    );
    let mut rows = vec![actions.into_any_element()];
    if enabled && resolved.all != Some(true) {
        for (id, label, value) in [
            ("design-prop-keep-first", "design.keep_first", local.first),
            ("design-prop-keep-last", "design.keep_last", local.last),
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
    }
    rows.push(super::design_controls::field(
        ws,
        "design-prop-keep-next",
        "design.keep_next",
        local.next.map(|v| v.to_string()).unwrap_or_default(),
        Target::Paragraph(name.to_owned()),
        cx,
    ));
    use schist_layout::styles::ParagraphStart;
    let choices = [
        (None, "design.inherited"),
        (Some(ParagraphStart::Anywhere), "design.start_anywhere"),
        (Some(ParagraphStart::NextColumn), "design.start_column"),
        (Some(ParagraphStart::NextFrame), "design.start_frame"),
        (Some(ParagraphStart::NextPage), "design.start_page"),
        (Some(ParagraphStart::NextOddPage), "design.start_odd"),
        (Some(ParagraphStart::NextEvenPage), "design.start_even"),
    ];
    let current = choices
        .iter()
        .position(|(value, _)| *value == start)
        .unwrap_or(0);
    let target = Target::Paragraph(name.to_owned());
    rows.push(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().child(t("design.keep_start")))
            .child(super::object_styles::picker(
                ws,
                "design-keep-start",
                choices.iter().map(|(_, label)| t(label).into()).collect(),
                current,
                move |ws, index, _| {
                    if let Some((value, _)) = choices.get(index) {
                        keeps::set_start(&mut ws.design, &target, *value);
                    }
                },
                cx,
            ))
            .into_any_element(),
    );
    rows
}
