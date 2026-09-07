//! The gallery's chrome, shared by the local library and Schist Cloud:
//! the palette, the top strip and bottom tray, the sidebar rows, the
//! grid frame with its scrollbar, the thumbnail cell, the drag ghost
//! and the right-click menu frame. The local gallery and the cloud
//! gallery are the same room — one shows watched folders, the other a
//! remote library — so everything here is what both of them draw
//! with, and the browser build, which has no local gallery, draws the
//! cloud one from the very same parts.
//!
//! It keeps its own palette rather than `ui::palette()` — a photo grid
//! wants quieter, flatter chrome than a panel set — but it follows the
//! theme choice: the light theme gets Picasa's warm white lightbox, the
//! dark theme a Lightroom-grey version of the same room, so opening the
//! gallery from a dark editor is not a flashbang.

use super::*;
use crate::ui::LineEdit;
use gpui::{img, Animation, AnimationExt as _, StatefulInteractiveElement as _, Transformation};
use schist_ui::{Button, ButtonColors, ListItem};

/// The gallery's chrome colours for one theme.
pub struct GalleryPalette {
    /// Behind the thumbnails.
    pub grid_bg: u32,
    /// The top strip and sidebar.
    pub chrome_bg: u32,
    pub chrome_edge: u32,
    pub tray_bg: u32,
    pub sidebar_selected: u32,
    /// Folder headers and the add-folder link — Picasa's blue.
    pub header: u32,
    pub text: u32,
    pub text_dim: u32,
    pub cell_edge: u32,
    /// Cell border under the pointer.
    pub cell_hover: u32,
    pub select_border: u32,
    pub select_fill: u32,
    pub button_bg: u32,
    pub button_hover: u32,
    /// The green action buttons and the "edited" badge.
    pub green: u32,
    pub green_hover: u32,
}

/// Picasa: white grid, warm grey chrome.
const GALLERY_LIGHT: GalleryPalette = GalleryPalette {
    grid_bg: 0xFFFFFF,
    chrome_bg: 0xEDEDE6,
    chrome_edge: 0xC9C9C0,
    tray_bg: 0xE3E3DC,
    sidebar_selected: 0xCFE0F2,
    header: 0x2A5DB0,
    text: 0x2B2B2B,
    text_dim: 0x7A7A72,
    cell_edge: 0xDDDDDD,
    cell_hover: 0xB9CBE0,
    select_border: 0x4A90D9,
    select_fill: 0xE8F0FB,
    button_bg: 0xF7F7F2,
    button_hover: 0xFFFFFF,
    green: 0x5C9E31,
    green_hover: 0x6DB33F,
};

/// The same room with the lights down — Lightroom's greys.
const GALLERY_DARK: GalleryPalette = GalleryPalette {
    grid_bg: 0x232323,
    chrome_bg: 0x2B2B2B,
    chrome_edge: 0x1C1C1C,
    tray_bg: 0x282828,
    sidebar_selected: 0x3A4A5C,
    header: 0x7FB0E8,
    text: 0xD8D8D8,
    text_dim: 0x8F8F8A,
    cell_edge: 0x3A3A3A,
    cell_hover: 0x55708C,
    select_border: 0x4A90D9,
    select_fill: 0x2C3A4A,
    button_bg: 0x383838,
    button_hover: 0x444444,
    green: 0x5C9E31,
    green_hover: 0x6DB33F,
};

pub fn pal() -> &'static GalleryPalette {
    if crate::ui::is_light() {
        &GALLERY_LIGHT
    } else {
        &GALLERY_DARK
    }
}

/// How the grid is grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    /// By capture month, newest first — the diary reading.
    Date,
    /// By the directory scanning found them in, or the cloud folder.
    Folder,
    /// By the nearest city their EXIF position names.
    Place,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
impl GroupBy {
    pub const ALL: [GroupBy; 3] = [GroupBy::Date, GroupBy::Folder, GroupBy::Place];

    pub fn label(self) -> &'static str {
        match self {
            GroupBy::Date => "Date",
            GroupBy::Folder => "Folder",
            GroupBy::Place => "Place",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            GroupBy::Date => "date",
            GroupBy::Folder => "folder",
            GroupBy::Place => "place",
        }
    }

    pub fn from_key(key: &str) -> Option<GroupBy> {
        GroupBy::ALL.into_iter().find(|g| g.key() == key)
    }
}

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A "YYYY-MM" month key as a header: "March 2024", or "Undated" for
/// anything that is not a month.
pub fn month_title(key: &str) -> String {
    match (
        key.get(..4),
        key.get(5..7).and_then(|m| m.parse::<usize>().ok()),
    ) {
        (Some(year), Some(month)) if (1..=12).contains(&month) => {
            format!("{} {year}", MONTHS[month - 1])
        }
        _ => "Undated".to_string(),
    }
}

/// The "YYYY-MM" month of a Unix time, in UTC — the cloud reports
/// capture times as seconds, and a month header does not care about
/// the hour.
pub fn month_key(unix: i64) -> String {
    // Civil-from-days (Howard Hinnant), enough for a month header.
    let days = unix.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}")
}

const SCROLLBAR_INSET: f32 = 4.0;

/// A grid's scroll handle, plus the viewport and selected-cell
/// rectangles recorded each paint — what keyboard navigation needs to
/// keep the selection on screen in a wrap layout that has no notion of
/// rows to ask about — and the state of a scrollbar-thumb drag.
pub struct GridScroll {
    pub handle: gpui::ScrollHandle,
    pub bounds: Bounds<Pixels>,
    /// A scrollbar-thumb drag in progress: the pointer's offset from
    /// the thumb's top when it was grabbed, in pixels.
    pub grab: Option<f32>,
    pub selected_bounds: Option<Bounds<Pixels>>,
    /// The keyboard moved the selection; scroll until it is visible.
    pub reveal: bool,
}

impl Default for GridScroll {
    fn default() -> Self {
        Self {
            handle: gpui::ScrollHandle::new(),
            bounds: Bounds::default(),
            grab: None,
            selected_bounds: None,
            reveal: false,
        }
    }
}

impl GridScroll {
    /// Cells per row at the recorded width: p_2 padding both sides,
    /// gap_2 between cells — the keyboard navigation's formula, so
    /// rows agree with up/down arrows.
    pub fn columns(&self, cell: f32) -> usize {
        let width = f32::from(self.bounds.size.width);
        (((width - 16.0 + 8.0) / (cell + 8.0)).floor() as usize).max(1)
    }

    /// The scrollbar's (inset, thumb height, thumb travel, max scroll)
    /// when the content overflows; `None` when it fits.
    pub fn scrollbar_geometry(&self) -> Option<(f32, f32, f32, f32)> {
        let view_h = f32::from(self.bounds.size.height);
        let max_y = f32::from(self.handle.max_offset().height);
        if view_h <= 0.0 || max_y <= 1.0 {
            return None;
        }
        let track_h = view_h - 2.0 * SCROLLBAR_INSET;
        let thumb_h = (track_h * view_h / (view_h + max_y)).clamp(30.0, track_h);
        let travel = (track_h - thumb_h).max(1.0);
        Some((SCROLLBAR_INSET, thumb_h, travel, max_y))
    }

    /// Scroll so the thumb follows the pointer of an active grab;
    /// `pointer_y` is in window coordinates.
    pub fn drag_to(&mut self, pointer_y: f32) {
        let Some(grab) = self.grab else {
            return;
        };
        let Some((inset, _, travel, max_y)) = self.scrollbar_geometry() else {
            return;
        };
        let top = f32::from(self.bounds.origin.y);
        let thumb_top = (pointer_y - top - inset - grab).clamp(0.0, travel);
        let mut offset = self.handle.offset();
        offset.y = px(-(thumb_top / travel * max_y));
        self.handle.set_offset(offset);
    }

    /// Nudge the grid until the keyboard-moved selection is on screen.
    /// Runs per render off the bounds the previous paint recorded, so
    /// it converges a frame after the selection moves. Returns whether
    /// it scrolled.
    pub fn reveal_tick(&mut self) -> bool {
        if !self.reveal {
            return false;
        }
        let (Some(cell), view) = (self.selected_bounds, self.bounds) else {
            return false;
        };
        if view.size.height <= px(0.0) {
            return false;
        }
        let top = f32::from(cell.origin.y);
        let bottom = top + f32::from(cell.size.height);
        let view_top = f32::from(view.origin.y);
        let view_bottom = view_top + f32::from(view.size.height);
        let mut offset = self.handle.offset();
        if bottom > view_bottom {
            // Scrolling down means a more negative offset in gpui.
            offset.y -= px(bottom - view_bottom + 8.0);
        } else if top < view_top {
            offset.y += px(view_top - top + 8.0);
        } else {
            self.reveal = false;
            return false;
        }
        self.handle.set_offset(offset);
        true
    }
}

/// Which grid a shared element is working on. A plain function
/// pointer, so the local grid's and the cloud grid's frames can share
/// one implementation without either capturing anything.
pub type GridAccess = fn(&mut Workspace) -> &mut GridScroll;

pub fn gallery_button(
    label: impl Into<SharedString>,
    green: bool,
    on_click: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let label = label.into();
    Button::new(label.clone(), label)
        .colors(ButtonColors {
            bg: Some(if green { pal().green } else { pal().button_bg }),
            hover: if green {
                pal().green_hover
            } else {
                pal().button_hover
            },
            text: if green { 0xFFFFFF } else { pal().text },
            border: Some(if green {
                pal().green
            } else {
                pal().chrome_edge
            }),
        })
        .rounded_md()
        .on_click(cx.listener(move |ws, _e, window, cx| on_click(ws, window, cx)))
}

/// A chip in the top strip that announces an active filter — the
/// least ignorable thing in the strip, since a filter you forgot is a
/// gallery that looks mysteriously empty. Clicking it opens the
/// filter; the ✕ clears it.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn filter_chip(
    label: String,
    open: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    clear: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .h(px(24.0))
        .px_2()
        .rounded_md()
        .bg(gpui::rgb(pal().select_border))
        .text_color(gpui::rgb(0xFFFFFF))
        .text_size(px(12.0))
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| open(ws, cx)),
        )
        .child(label)
        .child(
            div()
                .px_1()
                .hover(|s| s.bg(gpui::rgb(0xFFFFFF30)).rounded_sm())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| {
                        cx.stop_propagation();
                        clear(ws, cx);
                    }),
                )
                .child("\u{2715}"),
        )
}

/// The search box: a [`LineEdit`] drawn the gallery's way. Takes the
/// keyboard while active (the key context flips to text entry, so
/// letters stop being tool shortcuts). `focus` runs on click, `clear`
/// on the ✕ that appears once there is text.
pub fn search_field(
    edit: &LineEdit,
    placeholder: SharedString,
    caret_on: bool,
    focus: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    clear: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let active = edit.active;
    let text = edit.text.clone();
    let cursor = edit.cursor.min(text.len());
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .w(px(260.0))
        .h(px(24.0))
        .px_2()
        .rounded_md()
        .bg(gpui::rgb(pal().grid_bg))
        .border_1()
        .border_color(gpui::rgb(if active {
            pal().select_border
        } else {
            pal().chrome_edge
        }))
        .text_size(px(12.0))
        .text_color(gpui::rgb(if text.is_empty() {
            pal().text_dim
        } else {
            pal().text
        }))
        .cursor(gpui::CursorStyle::IBeam)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| {
                focus(ws, cx);
                ws.reset_caret_phase();
                cx.notify();
            }),
        )
        .child(div().flex_grow().truncate().child(
            // ⌘A's selection, drawn the way every field draws one; a
            // focused box otherwise shows a blinking caret the arrows
            // move, with the placeholder ghosted while it is empty.
            if edit.selected && !text.is_empty() {
                div()
                    .rounded_sm()
                    .px(px(1.0))
                    .bg(gpui::rgb(pal().select_border))
                    .text_color(gpui::rgb(0xFFFFFF))
                    .child(SharedString::from(text.clone()))
                    .into_any_element()
            } else if active {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(crate::ui::caret_run(
                        text[..cursor].to_string(),
                        text[cursor..].to_string(),
                        caret_on,
                        pal().text,
                    ))
                    .children(text.is_empty().then(|| {
                        div()
                            .text_color(gpui::rgb(pal().text_dim))
                            .child(placeholder.clone())
                    }))
                    .into_any_element()
            } else if text.is_empty() {
                div().child(placeholder.clone()).into_any_element()
            } else {
                div()
                    .child(SharedString::from(text.clone()))
                    .into_any_element()
            },
        ))
        .children((!text.is_empty()).then(|| {
            div()
                .px_1()
                .text_color(gpui::rgb(pal().text_dim))
                .hover(|s| s.text_color(gpui::rgb(pal().text)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| {
                        cx.stop_propagation();
                        clear(ws, cx);
                    }),
                )
                .child("\u{2715}")
        }))
}

/// The thumbnail-size slider, drawn on the gallery's own palette so it
/// does not import the editor theme's near-black track onto the tray.
pub fn size_slider(ratio: f32, cx: &mut Context<Workspace>) -> impl IntoElement {
    const WIDTH: f32 = 110.0;
    let entity = cx.entity();
    let set = move |ws: &mut Workspace, r: f32| {
        ws.set_gallery_thumb_px(80.0 + r * 160.0);
    };
    let down = set;
    let moved = set;
    div()
        .relative()
        .w(px(WIDTH))
        .h(px(12.0))
        .flex_none()
        .rounded_sm()
        .bg(gpui::rgb(pal().chrome_edge))
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(WIDTH * ratio.clamp(0.0, 1.0)))
                .rounded_sm()
                .bg(gpui::rgb(pal().select_border)),
        )
        .child(
            gpui::canvas(
                move |bounds, _window, cx| {
                    entity.update(cx, |ws, _| {
                        ws.record_slider_bounds("gallery-thumb-size", bounds)
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, ev: &gpui::MouseDownEvent, _w, cx| {
                ws.begin_slider("gallery-thumb-size", ratio);
                if let Some(r) = ws.slider_ratio("gallery-thumb-size", ev.position) {
                    down(ws, r);
                }
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |ws, ev: &gpui::MouseMoveEvent, _w, cx| {
            if ev.pressed_button == Some(MouseButton::Left)
                && ws.dragging_slider("gallery-thumb-size")
            {
                if let Some(r) = ws.slider_ratio("gallery-thumb-size", ev.position) {
                    moved(ws, r);
                    cx.notify();
                }
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|ws, _ev: &gpui::MouseUpEvent, _w, _cx| {
                ws.end_slider("gallery-thumb-size");
            }),
        )
}

/// A section caption in the sidebar: "FOLDERS", "BUCKETS".
pub fn sidebar_caption(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px_2()
        .pt_2()
        .pb_1()
        .text_size(px(11.0))
        .text_color(gpui::rgb(pal().text_dim))
        .child(text.into())
}

/// A blue link row in the sidebar: "+ Add folder…", "+ New bucket".
pub fn sidebar_link(
    label: impl Into<SharedString>,
    on_click: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    div()
        .px_2()
        .h(px(24.0))
        .flex()
        .items_center()
        .text_size(px(12.0))
        .text_color(gpui::rgb(pal().header))
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgb(pal().sidebar_selected)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _e: &MouseDownEvent, window, cx| on_click(ws, window, cx)),
        )
        .child(label.into())
}

/// A sidebar link whose click also reports the pointer, for one that
/// opens a small menu there.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn sidebar_menu_link(
    label: impl Into<SharedString>,
    on_click: impl Fn(&mut Workspace, Point<Pixels>, &mut Window, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    div()
        .px_2()
        .h(px(24.0))
        .flex()
        .items_center()
        .text_size(px(12.0))
        .text_color(gpui::rgb(pal().header))
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgb(pal().sidebar_selected)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                on_click(ws, e.position, window, cx)
            }),
        )
        .child(label.into())
}

/// One row of the sidebar, before its behaviour: the label, an
/// optional count on the right, the selected tint. Callers add the
/// click, drag and drop.
pub fn sidebar_row_frame(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    count: Option<usize>,
    selected: bool,
    indent: usize,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px_2()
        .pl(px(8.0 + 12.0 * indent as f32))
        .h(px(24.0))
        .text_size(px(12.0))
        .cursor_pointer()
        .bg(gpui::rgb(if selected {
            pal().sidebar_selected
        } else {
            pal().chrome_bg
        }))
        .hover(|s| s.bg(gpui::rgb(pal().sidebar_selected)))
        .child(div().flex_grow().truncate().child(label.into()))
        .children(count.map(|count| {
            div()
                .text_size(px(10.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(format!("{count}"))
        }))
}

/// The GROUP BY chips at the top of the sidebar.
pub fn group_chips(
    current: GroupBy,
    options: &[GroupBy],
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let mut row = div().flex().flex_row().gap_1().px_2().pb_2();
    for &group in options {
        let active = group == current;
        row = row.child(
            div()
                .px_2()
                .h(px(20.0))
                .flex()
                .items_center()
                .rounded_md()
                .text_size(px(11.0))
                .cursor_pointer()
                .bg(gpui::rgb(if active {
                    pal().sidebar_selected
                } else {
                    pal().button_bg
                }))
                .hover(move |s| {
                    if active {
                        s
                    } else {
                        s.bg(gpui::rgb(pal().button_hover))
                    }
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| {
                        ws.set_gallery_group(group, cx);
                    }),
                )
                .child(group.label()),
        );
    }
    div().child(sidebar_caption("GROUP BY")).child(row)
}

/// The sidebar's column: fixed width, its own scroll, the chrome tint.
pub fn sidebar_column(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .w(px(210.0))
        .flex_none()
        .overflow_y_scroll()
        .bg(gpui::rgb(pal().chrome_bg))
        .border_r_1()
        .border_color(gpui::rgb(pal().chrome_edge))
}

/// A group header in the grid: the blue title, a dim detail, a rule.
pub fn section_header(title: String, detail: String) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .pt_2()
                .pb_1()
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(gpui::rgb(pal().header))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(gpui::rgb(pal().text_dim))
                        .child(detail),
                ),
        )
        .child(div().h(px(1.0)).mb_2().bg(gpui::rgb(pal().cell_edge)))
}

/// The same small activity indicator for the grid, sidebar, and tray.
pub fn loading_spinner(id: &'static str) -> impl IntoElement {
    gpui::svg()
        .path("icons/loading.svg")
        .size(px(14.0))
        .flex_none()
        .text_color(gpui::rgb(pal().header))
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(900)).repeat(),
            |icon, delta| icon.with_transformation(Transformation::rotate(gpui::percentage(delta))),
        )
}

pub fn loading_note() -> impl IntoElement {
    div()
        .p_4()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(12.0))
        .text_color(gpui::rgb(pal().text_dim))
        .child(loading_spinner("cloud-grid-loading"))
        .child("Loading photos…")
}

/// Why the grid is bare, in the grid's own quiet voice.
pub fn empty_note(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .p_4()
        .text_size(px(12.0))
        .text_color(gpui::rgb(pal().text_dim))
        .child(text.into())
}

/// The scrolling column the grid's rows go in. It records the viewport
/// rectangle, so keyboard navigation can work out columns per row, the
/// reveal logic can keep the selection on screen, and the cells'
/// visibility probes know what "on screen" means; and it takes ⌘-wheel
/// (Ctrl elsewhere) to resize the thumbnails, as ⌘-wheel zooms a
/// canvas.
pub fn grid_column(
    id: &'static str,
    scroll: &GridScroll,
    access: GridAccess,
    cx: &mut Context<Workspace>,
) -> gpui::Stateful<gpui::Div> {
    let grid_entity = cx.entity();
    div()
        .id(id)
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .track_scroll(&scroll.handle)
        .bg(gpui::rgb(pal().grid_bg))
        .p_2()
        // The canvas sits inside the scrolled content, so its bounds
        // scroll along with it — subtract the scroll offset to get
        // back to window coordinates, the space every cell's own
        // bounds are reported in.
        .child(
            canvas(
                {
                    let grid_entity = grid_entity.clone();
                    move |bounds, _window, cx| {
                        grid_entity.update(cx, |ws, _| {
                            let grid = access(ws);
                            let offset = grid.handle.offset();
                            grid.bounds = gpui::Bounds {
                                origin: bounds.origin - offset,
                                size: bounds.size,
                            };
                        });
                    }
                },
                move |_, _, window, _| {
                    // It has to win over the container's own scrolling,
                    // which runs in the bubble phase — so take it in
                    // capture and stop it there.
                    let grid_entity = grid_entity.clone();
                    window.on_mouse_event(move |ev: &gpui::ScrollWheelEvent, phase, _w, cx| {
                        if phase != gpui::DispatchPhase::Capture
                            || !(ev.modifiers.platform || ev.modifiers.control)
                        {
                            return;
                        }
                        let dy = wheel_pixels(ev);
                        let took = grid_entity.update(cx, |ws, cx| {
                            if !access(ws).bounds.contains(&ev.position) {
                                return false;
                            }
                            ws.nudge_gallery_thumb_px(dy);
                            cx.notify();
                            true
                        });
                        if took {
                            cx.stop_propagation();
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        )
}

/// A wheel event's vertical travel in pixels: lines become pixels at
/// the platform's usual line height.
pub fn wheel_pixels(ev: &gpui::ScrollWheelEvent) -> f32 {
    match ev.delta {
        gpui::ScrollDelta::Pixels(p) => f32::from(p.y),
        gpui::ScrollDelta::Lines(l) => l.y * 20.0,
    }
}

/// The grid's frame around its column: the scrollbar gpui doesn't
/// paint — a track along the viewport's right edge, exact because the
/// thumb reads the scroll handle's own extents. Clicking the track
/// jumps there; dragging is handled by the wrapper, so the pointer may
/// wander off the twelve-pixel strip mid-drag without dropping the
/// thumb.
pub fn grid_frame(
    column: impl IntoElement,
    scroll: &GridScroll,
    access: GridAccess,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let scrollbar = scroll
        .scrollbar_geometry()
        .map(|(inset, thumb_h, travel, max_y)| {
            let scroll_y = (-f32::from(scroll.handle.offset().y)).clamp(0.0, max_y);
            let thumb_top = inset + scroll_y / max_y * travel;
            div()
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .w(px(12.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                        let grid = access(ws);
                        let y = f32::from(ev.position.y) - f32::from(grid.bounds.origin.y);
                        grid.grab = Some(if (thumb_top..thumb_top + thumb_h).contains(&y) {
                            // Grabbed the thumb: keep the grip point.
                            y - thumb_top
                        } else {
                            // Clicked the track: the thumb jumps
                            // there, held by its middle.
                            thumb_h / 2.0
                        });
                        grid.drag_to(f32::from(ev.position.y));
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(thumb_top))
                        .right(px(2.0))
                        .w(px(8.0))
                        .h(px(thumb_h))
                        .rounded_md()
                        .bg(gpui::rgb(pal().cell_edge))
                        .hover(|s| s.bg(gpui::rgb(pal().cell_hover))),
                )
        });
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(px(0.0))
        // Shrinkable: a flex item's minimum width is its content's, and
        // a row of cells is a fixed width, so without this the grid
        // refuses to give up room to the AI panel beside it and pushes
        // it off the right edge. The column count follows the width
        // the next frame.
        .min_w(px(0.0))
        .overflow_hidden()
        .on_mouse_move(cx.listener(move |ws, ev: &gpui::MouseMoveEvent, _w, cx| {
            let grid = access(ws);
            if grid.grab.is_none() {
                return;
            }
            if ev.pressed_button == Some(MouseButton::Left) {
                grid.drag_to(f32::from(ev.position.y));
                cx.notify();
            } else {
                // The button went up somewhere we never heard about.
                grid.grab = None;
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |ws, _ev: &gpui::MouseUpEvent, _w, _cx| {
                access(ws).grab = None;
            }),
        )
        .child(column)
        .children(scrollbar)
}

/// One thumbnail's square, before its behaviour: the picture (or the
/// "no preview" note), the selection tint and border, the "edited"
/// badge. Callers add the listeners, the drag and their probes.
pub fn cell_frame(
    id: impl Into<gpui::ElementId>,
    cell: f32,
    selected: bool,
    thumb: Option<Arc<RenderImage>>,
    failed: bool,
    edited: bool,
) -> gpui::Stateful<gpui::Div> {
    let inner = cell - 10.0;
    div()
        .id(id)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .w(px(cell))
        .h(px(cell))
        .flex_none()
        .relative()
        .rounded_sm()
        .bg(gpui::rgb(if selected {
            pal().select_fill
        } else {
            pal().grid_bg
        }))
        .border_2()
        .border_color(gpui::rgb(if selected {
            pal().select_border
        } else {
            pal().cell_edge
        }))
        .cursor_pointer()
        .hover(move |s| {
            if selected {
                s
            } else {
                s.border_color(gpui::rgb(pal().cell_hover))
            }
        })
        .children(thumb.map(|t| img(t).max_w(px(inner)).max_h(px(inner))))
        .children(failed.then(|| {
            div()
                .text_size(px(10.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child("no preview")
        }))
        .children(edited.then(|| {
            // Picasa's little brush: a corner badge saying this photo
            // carries an edit.
            div()
                .absolute()
                .bottom(px(3.0))
                .left(px(3.0))
                .px_1()
                .rounded_sm()
                .bg(gpui::rgb(pal().green))
                .text_size(px(9.0))
                .text_color(gpui::rgb(0xFFFFFF))
                .child("edited")
        }))
}

/// The lead cell reports where it landed, for the keyboard's
/// scroll-into-view.
pub fn lead_probe(access: GridAccess, cx: &mut Context<Workspace>) -> impl IntoElement {
    let cell_entity = cx.entity();
    canvas(
        move |bounds, _window, cx| {
            cell_entity.update(cx, |ws, _| access(ws).selected_bounds = Some(bounds));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// The ghost that rides the pointer during a drag: the picked-up
/// photo's whole square when its thumbnail is in memory (with a count
/// badge for a multi-drag), the old name pill only when it is not.
pub struct DragGhost {
    pub label: String,
    pub thumb: Option<Arc<gpui::RenderImage>>,
    pub count: usize,
    pub size: f32,
}

impl gpui::Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let Some(thumb) = self.thumb.clone() else {
            return div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(gpui::rgb(pal().select_border))
                .text_color(gpui::rgb(0xFFFFFF))
                .text_size(px(11.0))
                .child(SharedString::from(self.label.clone()))
                .into_any_element();
        };
        let inner = self.size - 10.0;
        div()
            .w(px(self.size))
            .h(px(self.size))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .bg(gpui::rgb(pal().grid_bg))
            .border_2()
            .border_color(gpui::rgb(pal().select_border))
            .opacity(0.85)
            .child(img(thumb).max_w(px(inner)).max_h(px(inner)))
            .children((self.count > 1).then(|| {
                div()
                    .absolute()
                    .top(px(-6.0))
                    .right(px(-6.0))
                    .min_w(px(18.0))
                    .h(px(18.0))
                    .px_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(gpui::rgb(pal().select_border))
                    .text_color(gpui::rgb(0xFFFFFF))
                    .text_size(px(10.0))
                    .child(format!("{}", self.count))
            }))
            .into_any_element()
    }
}

/// What a menu row does when clicked.
pub type MenuAction = std::rc::Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// One row of a right-click menu. `dismiss` closes the menu before the
/// action runs.
pub fn menu_row(
    label: String,
    dismiss: fn(&mut Workspace),
    act: MenuAction,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    ListItem::new(SharedString::from(format!("menu-row-{label}")))
        .accent_hover()
        .on_click(cx.listener(move |ws, _e, window, cx| {
            dismiss(ws);
            act(ws, window, cx);
            cx.notify();
        }))
        .child(SharedString::from(label))
        .into_any_element()
}

pub fn menu_sep() -> gpui::AnyElement {
    div()
        .h(px(1.0))
        .my_1()
        .bg(gpui::rgb(crate::ui::palette().edge))
        .into_any_element()
}

/// The menu's popup at the pointer, over everything; a click anywhere
/// else dismisses it.
pub fn menu_frame(
    position: Point<Pixels>,
    rows: Vec<gpui::AnyElement>,
    dismiss: fn(&mut Workspace),
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    gpui::deferred(
        div()
            .absolute()
            .left(position.x)
            .top(position.y)
            .w(px(220.0))
            .py_1()
            .bg(gpui::rgb(crate::ui::palette().popup_bg))
            .text_color(gpui::rgb(crate::ui::palette().text))
            .border_1()
            .border_color(gpui::rgb(crate::ui::palette().edge))
            .rounded_sm()
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(move |ws, _e, _w, cx| {
                dismiss(ws);
                cx.notify();
            }))
            .children(rows),
    )
    .into_any_element()
}

/// The same toolbar for local and cloud photos; only the action's destination changes.
pub fn top_strip(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let has_doc = ws.doc.is_some();
    let cloud = ws.cloud.show;
    let strip = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(38.0))
        .flex_none()
        .px_2()
        .bg(gpui::rgb(pal().chrome_bg))
        .border_b_1()
        .border_color(gpui::rgb(pal().chrome_edge))
        .child(gallery_button(
            "Import…",
            true,
            |ws, _w, cx| {
                if ws.cloud.show {
                    ws.cloud_pick_upload(false, cx);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if !ws.cloud.show {
                    ws.gallery_import_camera(cx);
                }
            },
            cx,
        ))
        .child(gallery_button(
            "Add Folder…",
            false,
            |ws, window, cx| {
                if ws.cloud.show {
                    ws.cloud_pick_upload(true, cx);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if !ws.cloud.show {
                    ws.gallery_add_folder(window, cx);
                }
                #[cfg(target_arch = "wasm32")]
                let _ = window;
            },
            cx,
        ))
        .child(gallery_button(
            "Refresh",
            false,
            |ws, _w, cx| {
                if ws.cloud.show {
                    ws.cloud_refresh(cx);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if !ws.cloud.show {
                    ws.library_rescan(cx);
                }
            },
            cx,
        ))
        .child(div().flex_grow());
    let strip = if cloud {
        strip
            .children(super::cloud_view::filter_chip(ws, cx))
            .child(super::cloud_view::search_box(ws, cx))
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            super::library_view::local_strip_search(strip, ws, cx)
        }
        #[cfg(target_arch = "wasm32")]
        {
            strip
        }
    };
    strip
        .child(div().flex_grow())
        .child(gallery_button(
            "Settings…",
            false,
            |ws, _w, cx| {
                ws.snapshot_preferences();
                ws.open_modal(Modal::Preferences, cx);
            },
            cx,
        ))
        .child(gallery_button(
            "Open…",
            false,
            crate::keymap::open_file_dialog,
            cx,
        ))
        .child(gallery_button(
            "New File…",
            false,
            |ws, _w, cx| ws.open_new_file_picker(cx),
            cx,
        ))
        .children((has_doc || cfg!(target_arch = "wasm32")).then(|| {
            gallery_button(
                "Back to Editing",
                false,
                |ws, _w, cx| ws.gallery_back_to_editor(cx),
                cx,
            )
        }))
}

pub fn photo_count(count: usize) -> String {
    format!("{count} {}", if count == 1 { "photo" } else { "photos" })
}

/// What a tray button does.
pub type TrayAction = Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// What the tray says about the current selection and library, in
/// either room.
pub struct TrayInfo {
    /// The green Edit button's action, when one photo leads.
    pub edit: Option<TrayAction>,
    /// A second button beside Edit — the cloud's Download….
    pub extra: Option<(&'static str, TrayAction)>,
    pub name: Option<String>,
    pub selected: usize,
    /// Dim remarks: "edited — versions kept beside the file", "3 hidden
    /// by the content filter".
    pub notes: Vec<String>,
    /// "128 photos".
    pub count: String,
}

/// The bottom tray: selection details and the green Edit button on the
/// left, the photo count and size slider on the right.
pub fn tray(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let info = if ws.cloud.show {
        super::cloud_view::tray_info(ws)
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            super::library_view::tray_info(ws)
        }
        #[cfg(target_arch = "wasm32")]
        {
            super::cloud_view::tray_info(ws)
        }
    };
    let thumb_px = ws.gallery_thumb_px();
    let ratio = (thumb_px - 80.0) / 160.0;
    div()
        .id("gallery-tray")
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(40.0))
        .flex_none()
        .px_2()
        .bg(gpui::rgb(pal().tray_bg))
        .border_t_1()
        .border_color(gpui::rgb(pal().chrome_edge))
        .children(
            info.edit
                .map(|edit| gallery_button("Edit", true, move |ws, w, cx| edit(ws, w, cx), cx)),
        )
        .children(
            info.extra.map(|(label, act)| {
                gallery_button(label, false, move |ws, w, cx| act(ws, w, cx), cx)
            }),
        )
        .children(info.name.map(|name| {
            div()
                .text_size(px(12.0))
                .text_color(gpui::rgb(pal().text))
                .child(name)
        }))
        .children((info.selected > 1).then(|| {
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(format!("{} selected", info.selected))
        }))
        .children(info.notes.into_iter().map(|note| {
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(note)
        }))
        .child(div().flex_grow())
        // The editor's status bar is hidden here, so the tray carries the
        // status line — otherwise an import's outcome lands nowhere. A
        // cloud transfer under way shows as a bar instead.
        .child(match ws.cloud.progress.clone() {
            Some((done, total, label)) => {
                let ratio = if total == 0 {
                    0.0
                } else {
                    (done as f32 / total as f32).clamp(0.0, 1.0)
                };
                div()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .w(px(300.0))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.0))
                            .text_color(gpui::rgb(pal().text_dim))
                            .child(label),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(4.0))
                            .rounded_sm()
                            .bg(gpui::rgb(pal().chrome_edge))
                            .child(
                                div()
                                    .h_full()
                                    .w(gpui::relative(ratio))
                                    .rounded_sm()
                                    .bg(gpui::rgb(pal().select_border)),
                            ),
                    )
                    .into_any_element()
            }
            None => div()
                .max_w(px(420.0))
                .truncate()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(ws.status.clone())
                .into_any_element(),
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .children(
                    (ws.cloud.show && ws.cloud.is_loading())
                        .then(|| loading_spinner("cloud-count-loading")),
                )
                .child(info.count),
        )
        .child(size_slider(ratio, cx))
}

impl Workspace {
    /// Thumbnail cell edge in pixels, the tray slider's value. One
    /// setting for both rooms on desktop; the browser has only the
    /// cloud's.
    pub(crate) fn gallery_thumb_px(&self) -> f32 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library.thumb_px
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.cloud.thumb_px
        }
    }

    /// One grouping choice for both local and cloud photos.
    pub(crate) fn gallery_group_by(&self) -> GroupBy {
        #[cfg(not(target_arch = "wasm32"))]
        let group = self.library.group_by;
        #[cfg(target_arch = "wasm32")]
        let group = self.cloud.group_by;
        group
    }

    /// Whether a gallery search box is taking typing, for the key
    /// context.
    pub(crate) fn gallery_typing(&self) -> bool {
        if !self.gallery_open() {
            return false;
        }
        if self.cloud.show {
            return self.cloud.search.active;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library.search.active || self.focused_field == Some("face-name")
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// A key while the gallery has the keyboard and no dialog is up:
    /// the search box first, then the arrows over the grid.
    pub(crate) fn gallery_key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.cloud.show {
            return self.cloud_search_key(ev, cx) || self.cloud_nav_key(ev, cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.gallery_viewer_key(ev, cx)
                || self.gallery_search_key(ev, cx)
                || self.gallery_nav_key(ev, cx)
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Escape in the gallery leaves the search — it is the innermost
    /// thing open. Returns whether there was one to leave.
    pub(crate) fn gallery_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.gallery_open() {
            return false;
        }
        if self.cloud.show {
            return self.cloud_search_clear(cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.local_gallery_escape(cx)
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Enter in the gallery opens the selected photo, unless the search
    /// box has the keyboard. Returns whether the gallery took it.
    pub(crate) fn gallery_enter(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.gallery_open() {
            return false;
        }
        if self.cloud.show {
            if self.cloud.search.active {
                return false;
            }
            if let Some(asset) = self.cloud_lead_asset() {
                self.cloud_open(asset, cx);
            }
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.library.search.active {
                return false;
            }
            if let Some(path) = self.library.lead_selected().cloned() {
                self.open_from_gallery(path, cx);
            }
            true
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// The strip's "Back to Editing".
    pub(crate) fn gallery_back_to_editor(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.toggle_gallery(cx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.cloud_set_visible(false);
            cx.notify();
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Workspace {
    pub fn set_gallery_thumb_px(&mut self, value: f32) {
        self.cloud.thumb_px = value.clamp(80.0, 240.0);
    }

    pub fn nudge_gallery_thumb_px(&mut self, wheel_dy: f32) {
        let value = self.cloud.thumb_px + wheel_dy * 0.2;
        self.set_gallery_thumb_px(value);
    }

    pub fn set_gallery_group(&mut self, group: GroupBy, cx: &mut Context<Self>) {
        self.cloud.group_by = group;
        self.cloud.query.offset = 0;
        self.cloud.query.sort = self.cloud_sort();
        self.cloud_watch_assets(true);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_keys_and_titles() {
        // 2024-03-15T12:00:00Z
        assert_eq!(month_key(1_710_504_000), "2024-03");
        // 1970-01-01
        assert_eq!(month_key(0), "1970-01");
        // 1999-12-31T23:59:59Z
        assert_eq!(month_key(946_684_799), "1999-12");
        assert_eq!(month_title("2024-03"), "March 2024");
        assert_eq!(month_title("0000-00"), "Undated");
        assert_eq!(month_title("nonsense"), "Undated");
    }
}
