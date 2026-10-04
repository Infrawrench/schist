//! Compact text-wrap controls for the selected items. Each click or committed
//! field is one undoable edit of every selected item.
use super::*;
use crate::design::controls::Target;
use schist_layout::text_wrap::{self, TextWrap, WrapMode, WrapSide};
use schist_layout::LayoutObject;

const MODES: [(WrapMode, &str, &str); 5] = [
    (WrapMode::None, "wrap-none", "design.wrap_none"),
    (
        WrapMode::BoundingBox,
        "wrap-box",
        "design.wrap_bounding_box",
    ),
    (WrapMode::Contour, "wrap-contour", "design.wrap_contour"),
    (WrapMode::JumpObject, "wrap-jump", "design.wrap_jump"),
    (
        WrapMode::NextColumn,
        "wrap-next-column",
        "design.wrap_next_column",
    ),
];

const SIDES: [(WrapSide, &str); 6] = [
    (WrapSide::BothSides, "design.wrap_side_both"),
    (WrapSide::LeftSide, "design.wrap_side_left"),
    (WrapSide::RightSide, "design.wrap_side_right"),
    (WrapSide::SideTowardsSpine, "design.wrap_side_towards_spine"),
    (WrapSide::SideAwayFromSpine, "design.wrap_side_away_spine"),
    (WrapSide::LargestArea, "design.wrap_side_largest"),
];

/// The selection's common wrap value, or None when it is mixed.
fn shared<T: PartialEq>(ws: &Workspace, value: impl Fn(&TextWrap) -> T) -> Option<T> {
    let default = TextWrap::default();
    let mut values = ws
        .design
        .selection
        .iter()
        .filter_map(|id| ws.design.document.object(*id))
        .map(|o| value(o.appearance.text_wrap.as_ref().unwrap_or(&default)));
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn edit(ws: &mut Workspace, cx: &mut Context<Workspace>, change: impl Fn(&mut TextWrap)) {
    ws.commit_focused_field();
    let ids = ws.design.selection.clone();
    text_wrap::edit_wrap(
        &mut ws.design.document,
        &mut ws.design.history,
        &ids,
        change,
    );
    cx.notify();
}

pub(super) fn rows(ws: &Workspace, cx: &mut Context<Workspace>) -> Vec<gpui::AnyElement> {
    let document = &ws.design.document;
    let selection = &ws.design.selection;
    let objects: Vec<_> = selection
        .iter()
        .filter_map(|id| document.object(*id))
        .collect();
    if objects.is_empty()
        || objects.len() != selection.len()
        || objects
            .iter()
            .any(|o| matches!(o.object, LayoutObject::Note { .. }))
    {
        return Vec::new();
    }
    let mode = shared(ws, |w| w.mode);
    let mut header = div().flex().items_center().gap_1().child(
        div()
            .flex_1()
            .min_w_0()
            .text_xs()
            .truncate()
            .child(t("design.text_wrap")),
    );
    for (index, (value, icon, label)) in MODES.into_iter().enumerate() {
        header = header.child(
            IconButton::new(("design-wrap-mode", index), icon)
                .size(22.0)
                .icon_size(14.0)
                .tooltip(t(label), None)
                .active(mode == Some(value))
                .on_click(cx.listener(move |ws, _, _, cx| edit(ws, cx, |w| w.mode = value))),
        );
    }
    // A frame's own text can ignore every other item's wrap.
    let frames = objects.iter().all(|o| {
        matches!(
            o.object,
            LayoutObject::TextFrame {
                text_path: None,
                ..
            }
        )
    });
    if frames {
        let ignoring = objects.iter().all(|o| o.appearance.ignore_wrap);
        header = header
            .child(
                div()
                    .w(px(1.0))
                    .h(px(14.0))
                    .mx_1()
                    .bg(gpui::rgb(palette().panel_edge)),
            )
            .child(
                IconButton::new("design-wrap-ignore", "wrap-ignore")
                    .size(22.0)
                    .icon_size(14.0)
                    .tooltip(t("design.wrap_ignore"), None)
                    .active(ignoring)
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.commit_focused_field();
                        let ids = ws.design.selection.clone();
                        text_wrap::set_ignore_wrap(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            &ids,
                            !ignoring,
                        );
                        cx.notify();
                    })),
            );
    }
    let mut rows = vec![header.into_any_element()];
    if matches!(mode, None | Some(WrapMode::None)) {
        return rows;
    }
    let offset = shared(ws, |w| w.offsets)
        .filter(|o| o.top == o.left && o.top == o.bottom && o.top == o.right)
        .map(|o| format!("{:.2}", o.top))
        .unwrap_or_default();
    let shaped = matches!(mode, Some(WrapMode::BoundingBox | WrapMode::Contour));
    let mut line = div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(super::design_controls::field(
                    ws,
                    "design-prop-wrap-offset",
                    "design.wrap_offset",
                    offset,
                    Target::Objects(selection.clone()),
                    cx,
                )),
        );
    if shaped {
        let inverse = shared(ws, |w| w.inverse) == Some(true);
        line = line.child(
            IconButton::new("design-wrap-invert", "wrap-invert")
                .size(22.0)
                .icon_size(14.0)
                .tooltip(t("design.wrap_invert"), None)
                .active(inverse)
                .on_click(cx.listener(move |ws, _, _, cx| edit(ws, cx, |w| w.inverse = !inverse))),
        );
    }
    rows.push(line.into_any_element());
    if shaped {
        let side = shared(ws, |w| w.side);
        let current = SIDES
            .iter()
            .position(|(value, _)| Some(*value) == side)
            .unwrap_or(0);
        let popup = Popup::Field("design-wrap-side");
        rows.push(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .child(t("design.wrap_side")),
                )
                .child(ui::dropdown(
                    &ws.dropdown,
                    ui::Dropdown {
                        popup,
                        is_open: ws.open_popup == Some(popup),
                        current,
                        label: if side.is_some() {
                            t(SIDES[current].1).to_string().into()
                        } else {
                            SharedString::default()
                        },
                        width: 130.0,
                        options: SIDES
                            .iter()
                            .enumerate()
                            .map(|(i, (_, key))| (t(key).to_string().into(), i))
                            .collect(),
                    },
                    move |ws, index, cx| {
                        if let Some((value, _)) = SIDES.get(index).copied() {
                            edit(ws, cx, |w| w.side = value);
                        }
                    },
                    cx,
                ))
                .into_any_element(),
        );
    }
    rows
}
