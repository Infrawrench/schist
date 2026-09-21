//! Photo decisions, filters and badges shared by local and cloud galleries.
//! The local presentation is the source of truth; hosts supply state and actions.
use crate::pal;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use schist_gallery::culling::{ColourLabel, CullEdit, CullFilter, CullFlag, PhotoCulling};
use schist_i18n::t;
use schist_ui::{Button, ButtonColors, Chip, Popover};

pub fn shortcut(key: &str) -> Option<CullEdit> {
    Some(match key {
        "0" | "1" | "2" | "3" | "4" | "5" => CullEdit::Rating(key.as_bytes()[0] - b'0'),
        "p" => CullEdit::Flag(CullFlag::Pick),
        "x" => CullEdit::Flag(CullFlag::Reject),
        "u" => CullEdit::Flag(CullFlag::None),
        "6" => CullEdit::Label(ColourLabel::Red),
        "7" => CullEdit::Label(ColourLabel::Yellow),
        "8" => CullEdit::Label(ColourLabel::Green),
        "9" => CullEdit::Label(ColourLabel::Blue),
        "m" => CullEdit::Label(ColourLabel::Magenta),
        "l" => CullEdit::Label(ColourLabel::None),
        _ => return None,
    })
}

fn flag_name(flag: CullFlag) -> &'static str {
    match flag {
        CullFlag::None => t("common.none"),
        CullFlag::Pick => t("culling.pick"),
        CullFlag::Reject => t("culling.reject"),
    }
}
const LABELS: [ColourLabel; 6] = [
    ColourLabel::None,
    ColourLabel::Red,
    ColourLabel::Yellow,
    ColourLabel::Green,
    ColourLabel::Blue,
    ColourLabel::Magenta,
];
fn colour(label: ColourLabel) -> u32 {
    match label {
        ColourLabel::None => pal().text_dim,
        ColourLabel::Red => 0xD84B4B,
        ColourLabel::Yellow => 0xD0AD25,
        ColourLabel::Green => 0x39AB60,
        ColourLabel::Blue => 0x428FD9,
        ColourLabel::Magenta => 0xCB50BD,
    }
}
fn colour_name(label: ColourLabel) -> &'static str {
    match label {
        ColourLabel::None => t("common.none"),
        ColourLabel::Red => t("common.red"),
        ColourLabel::Yellow => t("common.yellow"),
        ColourLabel::Green => t("common.green"),
        ColourLabel::Blue => t("common.blue"),
        ColourLabel::Magenta => t("common.magenta"),
    }
}

pub fn badge(value: PhotoCulling) -> Option<gpui::AnyElement> {
    if value == PhotoCulling::default() {
        return None;
    }
    let flag = match value.flag {
        CullFlag::None => "",
        flag => flag_name(flag),
    };
    Some(
        div()
            .absolute()
            .bottom_1()
            .left_1()
            .rounded_sm()
            .px_2()
            .py_0p5()
            .bg(gpui::rgba(0x000000CC))
            .text_color(gpui::rgb(0xFFFFFF))
            .text_size(px(10.0))
            .border_l_2()
            .border_color(gpui::rgb(colour(value.label)))
            .child(if value.rating == 0 && flag.is_empty() {
                colour_name(value.label).to_string()
            } else {
                format!("{} {flag}", "★".repeat(value.rating as usize))
                    .trim()
                    .to_string()
            })
            .into_any_element(),
    )
}

fn control_group() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .p_0p5()
        .rounded_sm()
        .bg(gpui::rgb(pal().tray_bg))
}

/// The local gallery's filter trigger and anchored popover, shared by every backend.
pub struct CullingPopover {
    pub open: bool,
    pub filtered: bool,
    pub compact: bool,
}

pub fn toolbar<T: 'static>(
    state: CullingPopover,
    content: Option<gpui::AnyElement>,
    toggle: fn(&mut T, &mut Context<T>),
    dismiss: fn(&mut T, &mut Context<T>),
    cx: &mut Context<T>,
) -> gpui::AnyElement {
    let active = state.open || state.filtered;
    div()
        .relative()
        .flex_none()
        .child(
            Button::new("cull-controls-toggle", t("menu.filter"))
                .colors(ButtonColors {
                    bg: Some(pal().button_bg),
                    hover: pal().button_hover,
                    text: pal().text,
                    border: Some(pal().chrome_edge),
                })
                .rounded_md()
                .active(active)
                .child(schist_ui::icon(
                    "chevron-down",
                    11.0,
                    if active {
                        schist_ui::palette().accent_text
                    } else {
                        pal().text
                    },
                ))
                .on_click(cx.listener(move |host, _, _, cx| toggle(host, cx))),
        )
        .children(content.map(|content| {
            gpui::deferred(
                div().absolute().left_0().top(px(30.0)).size_0().child(
                    gpui::anchored().snap_to_window_with_margin(px(8.0)).child(
                        Popover::new("cull-controls-popover")
                            .in_flow()
                            .w(px(if state.compact { 300.0 } else { 680.0 }))
                            .p_2()
                            .on_dismiss(cx.listener(move |host, _, _, cx| {
                                dismiss(host, cx);
                                // Consume the trigger press too, so it cannot reopen.
                                cx.stop_propagation();
                            }))
                            .child(content),
                    ),
                ),
            )
        }))
        .into_any_element()
}

/// A snapshot of the selected photos; persistence stays with the caller.
pub struct CullingControls {
    pub values: Vec<PhotoCulling>,
    pub busy: bool,
    pub comparing: bool,
    pub can_compare: bool,
    pub filter: CullFilter,
    pub max_height: f32,
}

/// Apply to the host's current filter, so clicks queued before the next render
/// compose without replacing another control's change with an older snapshot.
#[derive(Clone, Copy)]
pub enum CullingFilterAction {
    Reset,
    Rating(u8),
    Flag(CullFlag),
    Label(ColourLabel),
}

impl CullingFilterAction {
    pub fn apply(self, mut filter: CullFilter) -> CullFilter {
        match self {
            Self::Reset => filter = CullFilter::default(),
            Self::Rating(rating) => {
                filter.minimum_rating = if filter.minimum_rating == rating {
                    0
                } else {
                    rating
                }
            }
            Self::Flag(flag) => {
                filter.flag = if filter.flag == Some(flag) {
                    None
                } else {
                    Some(flag)
                }
            }
            Self::Label(label) => {
                filter.label = if filter.label == Some(label) {
                    None
                } else {
                    Some(label)
                }
            }
        }
        filter
    }
}

pub struct CullingActions<T: 'static> {
    pub edit: fn(&mut T, CullEdit, &mut Context<T>),
    pub filter: fn(&mut T, CullingFilterAction, &mut Context<T>),
    pub compare: fn(&mut T, &mut Context<T>),
}

pub fn controls<T: 'static>(
    state: CullingControls,
    actions: CullingActions<T>,
    cx: &mut Context<T>,
) -> gpui::AnyElement {
    let values = &state.values;
    let disabled = values.is_empty() || state.busy;
    let comparing = state.comparing;
    let edit = actions.edit;
    let set_filter = actions.filter;
    let compare = actions.compare;
    let mut ratings = control_group().child(
        Button::new(("cull-rating", 0usize), "×")
            .ghost()
            .w(px(24.0))
            .px_0()
            .tooltip(t("common.reset"), Some("0".into()))
            .disabled(disabled)
            .on_click(cx.listener(move |host, _, _, cx| edit(host, CullEdit::Rating(0), cx))),
    );
    for rating in 1..=5 {
        let filled = !values.is_empty() && values.iter().all(|v| v.rating >= rating);
        ratings = ratings.child(
            Button::new(
                ("cull-rating", rating as usize),
                if filled { "★" } else { "☆" },
            )
            .ghost()
            .w(px(24.0))
            .px_0()
            .text_size(px(15.0))
            .text_color(gpui::rgb(if filled { pal().text } else { pal().text_dim }))
            .tooltip(format!("{rating} ★"), Some(rating.to_string().into()))
            .disabled(disabled)
            .on_click(cx.listener(move |host, _, _, cx| edit(host, CullEdit::Rating(rating), cx))),
        );
    }
    let mut flags = control_group();
    for (index, (flag, key)) in [
        (CullFlag::Pick, "P"),
        (CullFlag::Reject, "X"),
        (CullFlag::None, "U"),
    ]
    .into_iter()
    .enumerate()
    {
        flags = flags.child(
            Button::new(("cull-flag", index), flag_name(flag))
                .ghost()
                .px_2()
                .tooltip(flag_name(flag), Some(key.into()))
                .active(!values.is_empty() && values.iter().all(|v| v.flag == flag))
                .disabled(disabled)
                .on_click(cx.listener(move |host, _, _, cx| edit(host, CullEdit::Flag(flag), cx))),
        );
    }
    let mut labels = control_group();
    for (index, label) in LABELS.into_iter().enumerate() {
        let key = ["L", "6", "7", "8", "9", "M"][index];
        labels = labels.child(
            Button::bare(("cull-label", index))
                .ghost()
                .w(px(24.0))
                .px_0()
                .child(label_dot(label))
                .tooltip(colour_name(label), Some(key.into()))
                .active(!values.is_empty() && values.iter().all(|v| v.label == label))
                .disabled(disabled)
                .on_click(
                    cx.listener(move |host, _, _, cx| edit(host, CullEdit::Label(label), cx)),
                ),
        );
    }
    let mut edits = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .min_w(px(60.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(t(if comparing {
                    "common.photo"
                } else {
                    "common.selection"
                })),
        )
        .child(ratings)
        .child(flags)
        .child(labels);
    if !comparing {
        edits = edits.child(div().flex_grow()).child(
            Button::new("cull-compare", t("culling.compare"))
                .px_2()
                .tooltip(t("culling.compare"), Some("C".into()))
                .disabled(!state.can_compare || state.busy)
                .on_click(cx.listener(move |host, _, _, cx| compare(host, cx))),
        );
    }
    let filter = state.filter;
    let mut minimums = control_group();
    for rating in 1..=5 {
        minimums = minimums.child(
            Chip::new(("cull-min", rating as usize), format!("{rating}★+"))
                .rounded_sm()
                .px_2()
                .selected(filter.minimum_rating == rating)
                .on_click(cx.listener(move |host, _, _, cx| {
                    set_filter(host, CullingFilterAction::Rating(rating), cx);
                })),
        );
    }
    let mut filter_flags = control_group();
    for (index, flag) in [CullFlag::Pick, CullFlag::Reject, CullFlag::None]
        .into_iter()
        .enumerate()
    {
        filter_flags = filter_flags.child(
            Chip::new(("cull-filter-flag", index), flag_name(flag))
                .rounded_sm()
                .px_2()
                .selected(filter.flag == Some(flag))
                .on_click(cx.listener(move |host, _, _, cx| {
                    set_filter(host, CullingFilterAction::Flag(flag), cx);
                })),
        );
    }
    let mut filter_labels = control_group();
    for (index, label) in LABELS.into_iter().enumerate() {
        filter_labels = filter_labels.child(
            Chip::new(("cull-filter-label", index), "")
                .w(px(24.0))
                .px_0()
                .rounded_sm()
                .child(label_dot(label))
                .tooltip(colour_name(label), None)
                .selected(filter.label == Some(label))
                .on_click(cx.listener(move |host, _, _, cx| {
                    set_filter(host, CullingFilterAction::Label(label), cx);
                })),
        );
    }
    let filters = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .min_w(px(60.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(t("menu.filter")),
        )
        .child(
            Chip::new("cull-all", t("common.all"))
                .rounded_sm()
                .selected(filter == CullFilter::default())
                .on_click(cx.listener(move |host, _, _, cx| {
                    set_filter(host, CullingFilterAction::Reset, cx);
                })),
        )
        .child(minimums)
        .child(filter_flags)
        .child(filter_labels);
    div()
        .id("cull-controls")
        .flex()
        .flex_col()
        .max_h(px(state.max_height))
        .overflow_y_scroll()
        .gap_2()
        .text_size(px(11.0))
        .child(edits)
        .when(!comparing, |bar| bar.child(filters))
        .into_any_element()
}

fn label_dot(label: ColourLabel) -> gpui::AnyElement {
    if label == ColourLabel::None {
        div().text_size(px(13.0)).child("×").into_any_element()
    } else {
        div()
            .size(px(9.0))
            .rounded_full()
            .bg(gpui::rgb(colour(label)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn culling_filter_actions_compose_against_current_state() {
        let filter = CullingFilterAction::Rating(4).apply(CullFilter::default());
        let filter = CullingFilterAction::Flag(CullFlag::None).apply(filter);
        let filter = CullingFilterAction::Label(ColourLabel::Blue).apply(filter);
        assert_eq!(
            filter,
            CullFilter {
                minimum_rating: 4,
                flag: Some(CullFlag::None),
                label: Some(ColourLabel::Blue),
            }
        );
        let filter = CullingFilterAction::Flag(CullFlag::None).apply(filter);
        assert_eq!(filter.flag, None);
        assert_eq!(filter.minimum_rating, 4);
        assert_eq!(filter.label, Some(ColourLabel::Blue));
        let filter = CullingFilterAction::Rating(4).apply(filter);
        assert_eq!(filter.minimum_rating, 0);
        assert_eq!(
            CullingFilterAction::Reset.apply(filter),
            CullFilter::default()
        );
    }
}
