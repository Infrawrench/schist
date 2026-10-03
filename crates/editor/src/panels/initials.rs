//! Initial counts stay behind a closed Paragraph disclosure.
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
    rows
}
