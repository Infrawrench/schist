//! Small widget kit shared by the panels and dialogs.
//!
//! Deliberately minimal: GPUI ships no widget library, and a full text-input
//! implementation needs an IME handler, so numeric fields here are edited by
//! click-to-focus plus digit keys (see `Workspace::field_key`) rather than
//! by a general-purpose text editor.

use crate::workspace::{Popup, Workspace};
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _,
};
use schist_ui::{Button, Checkbox, IconButton, ListItem};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub use schist_ui::{is_light, palette, set_light, tip};

/// What a dialog does when the user presses Enter.
pub type DialogAction = Rc<dyn Fn(&mut Workspace, &mut gpui::Window, &mut Context<Workspace>)>;

thread_local! {
    /// The primary button built most recently.
    ///
    /// A dialog's default action is, by definition, whatever its primary
    /// button does, and the buttons are built as plain closures deep
    /// inside each dialog body with no path back to the workspace. Rather
    /// than thread an out-parameter through every dialog function, the
    /// primary button leaves its handler here and `dialogs::render` --
    /// which brackets the whole build and does hold `&mut Workspace` --
    /// picks it up. GPUI renders on one thread, synchronously, so the
    /// slot is only ever live for the duration of one dialog build.
    static DEFAULT_ACTION: RefCell<Option<DialogAction>> = const { RefCell::new(None) };
}

/// Start a dialog build: forget any previous dialog's default action.
/// Whether the chrome is driven by fingers: iOS and iPadOS. Everything
/// sized for a pointer grows to a 44pt target there, and the menus that
/// open on hover are replaced by the platform's own.
pub const fn touch() -> bool {
    cfg!(target_os = "ios")
}

/// Whether this is an iPad rather than an iPhone. iPadOS has a menu bar
/// of its own (a swipe down from the top edge, or the pointer), fed by
/// the same menus as the macOS bar, so the in-window bar is only drawn
/// on the phone.
#[cfg(target_os = "ios")]
pub fn ipad() -> bool {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    static IPAD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *IPAD.get_or_init(|| unsafe {
        // UIUserInterfaceIdiomPad.
        const PAD: isize = 1;
        let Some(class) = AnyClass::get(c"UIDevice") else {
            return false;
        };
        let device: *mut AnyObject = msg_send![class, currentDevice];
        if device.is_null() {
            return false;
        }
        let idiom: isize = msg_send![device, userInterfaceIdiom];
        idiom == PAD
    })
}

#[cfg(not(target_os = "ios"))]
pub const fn ipad() -> bool {
    false
}

/// A path as the user should read it. The desktop shows it whole; on
/// iOS the app's container is a long opaque string that changes on
/// every install and means nothing to anyone, so a path under it shows
/// from the container down ("Documents/Photos/IMG_0111.heic") and any
/// other path by its name.
pub fn shown_path(path: &std::path::Path) -> String {
    if !touch() {
        return path.display().to_string();
    }
    if let Some(home) = std::env::var_os("HOME") {
        if let Ok(rest) = path.strip_prefix(&home) {
            if let Some(rest) = rest.to_str() {
                if !rest.is_empty() {
                    return rest.to_string();
                }
            }
        }
    }
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The chrome's dimensions, in points: the desktop's, or the touch set.
/// One table rather than `if touch()` at every call site, so the two
/// layouts can be read side by side.
#[derive(Clone, Copy)]
pub struct Metrics {
    /// The window's base text size.
    pub text: f32,
    /// Text in panel rows, menu rows and tool names.
    pub row_text: f32,
    /// Secondary text: hints, panel titles, the status bar.
    pub small_text: f32,
    pub menu_bar_h: f32,
    pub menu_title_h: f32,
    pub menu_row_h: f32,
    pub menu_w: f32,
    pub options_bar_h: f32,
    pub tab_h: f32,
    pub status_h: f32,
    pub toolbar_w: f32,
    pub tool_slot: f32,
    pub tool_icon: f32,
    pub panel_w: f32,
    pub layer_row_h: f32,
    pub history_row_h: f32,
    pub icon_button: f32,
    pub icon_button_icon: f32,
    pub slider_w: f32,
    pub slider_h: f32,
}

pub const DESKTOP_METRICS: Metrics = Metrics {
    text: 12.0,
    row_text: 12.0,
    small_text: 11.0,
    menu_bar_h: 28.0,
    menu_title_h: 22.0,
    menu_row_h: 24.0,
    menu_w: 230.0,
    options_bar_h: 32.0,
    tab_h: 26.0,
    status_h: 24.0,
    toolbar_w: 40.0,
    tool_slot: 30.0,
    tool_icon: 16.0,
    panel_w: 260.0,
    layer_row_h: 34.0,
    history_row_h: 19.0,
    icon_button: 22.0,
    icon_button_icon: 14.0,
    slider_w: 72.0,
    slider_h: 12.0,
};

/// Apple's 44pt minimum target, larger type, and a wider panel column
/// to carry both.
pub const TOUCH_METRICS: Metrics = Metrics {
    text: 14.0,
    row_text: 15.0,
    small_text: 13.0,
    menu_bar_h: 44.0,
    menu_title_h: 36.0,
    menu_row_h: 44.0,
    menu_w: 280.0,
    options_bar_h: 48.0,
    tab_h: 40.0,
    status_h: 30.0,
    toolbar_w: 56.0,
    tool_slot: 44.0,
    tool_icon: 22.0,
    panel_w: 320.0,
    layer_row_h: 48.0,
    history_row_h: 36.0,
    icon_button: 36.0,
    icon_button_icon: 18.0,
    slider_w: 120.0,
    slider_h: 22.0,
};

pub fn metrics() -> Metrics {
    if touch() {
        TOUCH_METRICS
    } else {
        DESKTOP_METRICS
    }
}

/// Below this window width the side panels float over the canvas rather
/// than sit beside it, and the menu bar collapses to one button: a phone,
/// or a narrow Split View on an iPad.
pub const COMPACT_WIDTH: f32 = 700.0;

pub fn compact(window: &gpui::Window) -> bool {
    touch() && f32::from(window.viewport_size().width) < COMPACT_WIDTH
}

pub fn reset_default_action() {
    DEFAULT_ACTION.with(|slot| *slot.borrow_mut() = None);
}

/// End a dialog build: take whatever its primary button registered.
pub fn take_default_action() -> Option<DialogAction> {
    DEFAULT_ACTION.with(|slot| slot.borrow_mut().take())
}

/// A labelled push button, wired to the workspace.
pub fn button(
    label: impl Into<SharedString>,
    primary: bool,
    on_click: impl Fn(&mut Workspace, &mut gpui::Window, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let on_click: DialogAction = Rc::new(on_click);
    if primary {
        let action = on_click.clone();
        DEFAULT_ACTION.with(|slot| *slot.borrow_mut() = Some(action));
    }
    let label = label.into();
    Button::new(label.clone(), label)
        .when(primary, Button::primary)
        .on_click(cx.listener(move |ws, _e, window, cx| on_click(ws, window, cx)))
}

/// Everything a [`num_field`] needs to draw itself.
///
/// State is passed in rather than read from the entity: these render
/// *during* `Workspace::render`, where reading the entity panics on the
/// outstanding mutable borrow.
pub struct NumField {
    pub id: &'static str,
    pub value: f32,
    pub suffix: &'static str,
    pub step: f32,
    pub focused: bool,
    pub buffer: String,
}

/// A numeric field: click to focus and type digits, or use the ± buttons.
pub fn num_field(
    field: NumField,
    on_change: impl Fn(&mut Workspace, f32) + Clone + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let NumField {
        id,
        value,
        suffix,
        step,
        focused,
        buffer,
    } = field;
    let committed = if value.fract().abs() < 0.01 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    let shown = if focused && !buffer.is_empty() {
        buffer
    } else {
        committed.clone()
    };
    let dec = on_change.clone();
    let inc = on_change.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .w(px(62.0))
                .h(px(20.0))
                .px_1()
                .rounded_sm()
                .bg(gpui::rgb(palette().field_bg))
                .border_1()
                .border_color(gpui::rgb(if focused {
                    palette().accent
                } else {
                    palette().field_bg
                }))
                .text_size(px(11.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _e, _w, cx| {
                        ws.focus_field(id, committed.clone());
                        cx.notify();
                    }),
                )
                .child(format!("{shown}{suffix}")),
        )
        .child(step_button(id, "minus", move |ws| dec(ws, -step), cx))
        .child(step_button(id, "plus", move |ws| inc(ws, step), cx))
}

fn step_button(
    field: &'static str,
    icon_name: &'static str,
    on_click: impl Fn(&mut Workspace) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    IconButton::new(
        SharedString::from(format!("{field}-{icon_name}")),
        icon_name,
    )
    .filled()
    .size(18.0)
    .icon_size(11.0)
    .on_click(cx.listener(move |ws, _e, _w, cx| {
        on_click(ws);
        cx.notify();
    }))
}

/// A checkbox with a label to its right, keyed by that label.
pub fn checkbox(
    label: impl Into<SharedString>,
    checked: bool,
    on_toggle: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let label = label.into();
    Checkbox::new(
        SharedString::from(format!("checkbox-{label}")),
        label,
        checked,
    )
    .on_change(cx.listener(move |ws, _checked, _w, cx| {
        on_toggle(ws, cx);
        cx.notify();
    }))
}

/// State of the open dropdown's option list: its scroll, the row the
/// keyboard has walked or typed to, and the type-ahead buffer.
///
/// One instance serves every dropdown because only one popup can be open
/// at a time. Cloning shares the underlying state, which is how the
/// per-frame `DialogState` snapshot hands it to dialog widgets.
#[derive(Clone)]
pub struct DropdownState {
    handle: gpui::ScrollHandle,
    /// Whether the open dropdown has been scrolled to its value yet:
    /// once per opening, after which the list is the user's to scroll.
    scrolled: Rc<Cell<bool>>,
    /// The row the keyboard has landed on. Separate from the committed
    /// value: walking the list with the arrows or typing a prefix moves
    /// this, and Enter is what turns it into a choice.
    highlight: Rc<Cell<Option<usize>>>,
    /// What has been typed so far, and when the last character arrived.
    /// A pause ends the word, so "ar" a second later starts afresh at
    /// the first "a" rather than looking for "arar".
    typed: Rc<RefCell<(String, Option<std::time::Instant>)>>,
}

impl Default for DropdownState {
    fn default() -> Self {
        DropdownState {
            handle: gpui::ScrollHandle::new(),
            scrolled: Default::default(),
            highlight: Default::default(),
            typed: Default::default(),
        }
    }
}

/// How long a pause ends a type-ahead word. Native lists use about a
/// second; Cocoa's is a little under.
const TYPE_AHEAD_PAUSE: std::time::Duration = std::time::Duration::from_millis(1000);

impl DropdownState {
    /// Forget the last scroll, highlight and typing, so the next dropdown
    /// to open gets one scroll to its selection and a clean slate. Called
    /// whenever a popup opens or closes.
    pub fn reset(&self) {
        self.scrolled.set(false);
        self.highlight.set(None);
        self.typed.borrow_mut().0.clear();
    }

    /// The row the keyboard is on, if it has moved at all.
    pub fn highlight(&self) -> Option<usize> {
        self.highlight.get()
    }

    /// Put the keyboard on row `ix` and bring it into view.
    pub fn set_highlight(&self, ix: usize) {
        self.highlight.set(Some(ix));
        self.handle.scroll_to_item(ix);
    }

    /// Add `text` to the type-ahead word and say which row it now names,
    /// given the row the keyboard is on (`at`) for letter cycling.
    pub fn type_ahead(
        &self,
        text: &str,
        labels: &[SharedString],
        at: Option<usize>,
    ) -> Option<usize> {
        let now = std::time::Instant::now();
        let mut typed = self.typed.borrow_mut();
        let stale = typed
            .1
            .is_some_and(|last| now.duration_since(last) > TYPE_AHEAD_PAUSE);
        if stale {
            typed.0.clear();
        }
        typed.0.push_str(text);
        typed.1 = Some(now);
        type_ahead_target(labels, &typed.0, at)
    }

    /// Drop the type-ahead word (Backspace).
    pub fn clear_typed(&self) {
        self.typed.borrow_mut().0.clear();
    }
}

/// The row a type-ahead word lands on: the first row whose label starts
/// with `typed`, ignoring case. When nothing starts with it and the word
/// is one letter pressed over and over, the presses walk through the
/// rows that start with that letter instead, from the row the keyboard
/// is on (`at`), the way every native list does.
pub fn type_ahead_target(
    labels: &[impl AsRef<str>],
    typed: &str,
    at: Option<usize>,
) -> Option<usize> {
    let word = typed.to_lowercase();
    let mut chars = word.chars();
    let first = chars.next()?;
    let starts_with =
        |ix: usize, prefix: &str| labels[ix].as_ref().to_lowercase().starts_with(prefix);
    if let Some(ix) = (0..labels.len()).find(|&ix| starts_with(ix, &word)) {
        return Some(ix);
    }
    let repeated = word.chars().count() > 1 && chars.all(|c| c == first);
    if !repeated {
        return None;
    }
    let letter = first.to_string();
    let n = labels.len();
    let from = at.map_or(0, |i| i + 1);
    (0..n)
        .map(|k| (from + k) % n)
        .find(|&ix| starts_with(ix, &letter))
}

/// The dropdown open in the frame most recently built, so the keystrokes
/// that arrive between frames can walk and pick its rows.
///
/// Like [`DEFAULT_ACTION`]: a dropdown's rows and its select handler are
/// built as plain values deep inside a panel or dialog body with no path
/// back to the workspace, so the open one leaves them here as it renders
/// and `Workspace::dropdown_key` reads them back.
type DropdownSelect = dyn Fn(&mut Workspace, usize, &mut Context<Workspace>);

pub struct OpenDropdown {
    pub labels: Vec<SharedString>,
    /// Row of the committed value, where keyboard walking starts from.
    pub current: Option<usize>,
    select: Rc<DropdownSelect>,
}

impl OpenDropdown {
    /// Choose row `ix` as if it had been clicked.
    pub fn select(&self, ws: &mut Workspace, ix: usize, cx: &mut Context<Workspace>) {
        (self.select)(ws, ix, cx)
    }
}

thread_local! {
    static OPEN_DROPDOWN: RefCell<Option<Rc<OpenDropdown>>> = const { RefCell::new(None) };
}

/// Start a frame: no dropdown has rendered open yet.
pub fn reset_open_dropdown() {
    OPEN_DROPDOWN.with(|slot| *slot.borrow_mut() = None);
}

/// The dropdown that rendered open in the last frame, if any.
pub fn open_dropdown() -> Option<Rc<OpenDropdown>> {
    OPEN_DROPDOWN.with(|slot| slot.borrow().clone())
}

/// Placement and state for a [`dropdown`].
pub struct Dropdown<T> {
    pub popup: Popup,
    pub is_open: bool,
    pub current: T,
    pub label: SharedString,
    /// Button width in pixels; zero fills the row it sits in.
    pub width: f32,
    pub options: Vec<(SharedString, T)>,
}

/// A dropdown button that opens its popup with the given options.
pub fn dropdown<T: Clone + PartialEq + 'static>(
    scroll: &DropdownState,
    spec: Dropdown<T>,
    on_select: impl Fn(&mut Workspace, T, &mut Context<Workspace>) + Clone + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    dropdown_impl(scroll, spec, false, on_select, cx)
}

/// A [`dropdown`] whose rows are each set in the typeface they name, the
/// way Figma's font menu previews its families. Falls back to the UI font
/// for a family the window's text system cannot resolve.
pub fn font_dropdown<T: Clone + PartialEq + 'static>(
    scroll: &DropdownState,
    spec: Dropdown<T>,
    on_select: impl Fn(&mut Workspace, T, &mut Context<Workspace>) + Clone + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    dropdown_impl(scroll, spec, true, on_select, cx)
}

fn dropdown_impl<T: Clone + PartialEq + 'static>(
    scroll: &DropdownState,
    spec: Dropdown<T>,
    preview_fonts: bool,
    on_select: impl Fn(&mut Workspace, T, &mut Context<Workspace>) + Clone + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let Dropdown {
        popup,
        is_open,
        current,
        label,
        width,
        options,
    } = spec;
    let current = &current;
    let mut root = div()
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .when(width > 0.0, |d| d.w(px(width)))
        .when(width <= 0.0, |d| d.flex_grow())
        .h(px(20.0))
        .px_1()
        .rounded_sm()
        .bg(gpui::rgb(palette().field_bg))
        .text_size(px(11.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _e, _w, cx| ws.toggle_popup(popup, cx)),
        )
        .child(div().flex_1().min_w_0().text_ellipsis().child(label))
        .child(crate::panels::icon(
            "chevron-down",
            11.0,
            palette().text_dim,
        ));
    if is_open {
        let current_ix = options.iter().position(|(_, v)| v == current);
        // Open at the current value rather than the top of the list, so
        // re-opening a long menu (fonts, blend modes) shows where you are
        // instead of starting from the beginning. Once per opening: after
        // that the list is the user's to scroll.
        if !scroll.scrolled.replace(true) {
            if let Some(ix) = current_ix {
                scroll.handle.scroll_to_top_of_item(ix);
            }
        }
        // Leave the rows where the keyboard can find them.
        {
            let labels: Vec<SharedString> = options.iter().map(|(t, _)| t.clone()).collect();
            let values: Vec<T> = options.iter().map(|(_, v)| v.clone()).collect();
            let on_select = on_select.clone();
            let open = OpenDropdown {
                labels,
                current: current_ix,
                select: Rc::new(move |ws, ix, cx| {
                    if let Some(value) = values.get(ix) {
                        on_select(ws, value.clone(), cx);
                    }
                }),
            };
            OPEN_DROPDOWN.with(|slot| *slot.borrow_mut() = Some(Rc::new(open)));
        }
        let highlight = scroll.highlight();
        let rows: Vec<gpui::AnyElement> = options
            .into_iter()
            .enumerate()
            .map(|(ix, (text, value))| {
                let selected = value == *current;
                let keyed = highlight == Some(ix) && !selected;
                let on_select = on_select.clone();
                ListItem::new(("dropdown-row", ix))
                    .h(px(20.0))
                    .text_size(px(11.0))
                    // Each family's name set in itself is what tells you
                    // what you are choosing; the label alone does not.
                    .when(preview_fonts, |d| d.font_family(text.clone()))
                    .selected(selected)
                    .highlighted(keyed)
                    .on_click(cx.listener(move |ws, _e, _w, cx| {
                        ws.close_popup(cx);
                        on_select(ws, value.clone(), cx);
                        cx.notify();
                    }))
                    .child(text)
                    .into_any_element()
            })
            .collect();
        root = root.child(gpui::deferred(
            div()
                .id("dropdown-items")
                .absolute()
                .top(px(22.0))
                .left_0()
                .w(px(width.max(140.0)))
                .max_h(px(300.0))
                .overflow_y_scroll()
                .track_scroll(&scroll.handle)
                .py_1()
                .bg(gpui::rgb(palette().popup_bg))
                .text_color(gpui::rgb(palette().text))
                .border_1()
                .border_color(gpui::rgb(palette().edge))
                .rounded_sm()
                .shadow_lg()
                .occlude()
                .on_mouse_down_out(cx.listener(|ws, _e, _w, cx| ws.close_popup(cx)))
                .children(rows),
        ));
    }
    root
}

/// A bare slider track that reports a 0..1 ratio while dragged. Panels and
/// dialogs both build on this.
pub fn slider_track(
    id: &'static str,
    ratio: f32,
    width: f32,
    on_change: impl Fn(&mut Workspace, f32, &mut Context<Workspace>) + Clone + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let entity = cx.entity();
    let down = on_change.clone();
    let moved = on_change;
    div()
        .relative()
        .w(px(width))
        .h(px(12.0))
        .flex_none()
        .rounded_sm()
        .bg(gpui::rgb(palette().field_bg))
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(width * ratio.clamp(0.0, 1.0)))
                .rounded_sm()
                .bg(gpui::rgb(palette().accent)),
        )
        .child(
            gpui::canvas(
                move |bounds, _window, cx| {
                    entity.update(cx, |ws, _| ws.record_slider_bounds(id, bounds));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, ev: &gpui::MouseDownEvent, _w, cx| {
                ws.begin_slider(id, ratio);
                if let Some(r) = ws.slider_ratio(id, ev.position) {
                    down(ws, r, cx);
                }
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |ws, ev: &gpui::MouseMoveEvent, _w, cx| {
            if ev.pressed_button == Some(MouseButton::Left) && ws.dragging_slider(id) {
                if let Some(r) = ws.slider_ratio(id, ev.position) {
                    moved(ws, r, cx);
                    cx.notify();
                }
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |ws, _ev: &gpui::MouseUpEvent, _w, _cx| {
                ws.end_slider(id);
            }),
        )
}

/// A labelled row inside a dialog.
/// The previous char boundary in `s` before byte position `at` — what
/// a left arrow moves a field's caret by.
pub fn caret_left(s: &str, at: usize) -> usize {
    s[..at.min(s.len())]
        .char_indices()
        .next_back()
        .map_or(0, |(i, _)| i)
}

/// The next char boundary in `s` after byte position `at`.
pub fn caret_right(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    at + s[at..].chars().next().map_or(0, |c| c.len_utf8())
}

/// A focused field's inside: the text split around a caret bar that
/// blinks. The bar keeps its one-pixel slot while off, so the text
/// does not shuffle as it blinks; `color` is the field's text colour,
/// since the gallery has its own palette.
pub fn caret_run(before: String, after: String, on: bool, color: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .max_w_full()
        .overflow_hidden()
        .children((!before.is_empty()).then(|| div().flex_none().child(SharedString::from(before))))
        .child(div().flex_none().w(px(1.0)).h(px(13.0)).bg(if on {
            gpui::rgba((color << 8) | 0xFF)
        } else {
            gpui::rgba(0x00000000)
        }))
        .children((!after.is_empty()).then(|| div().flex_none().child(SharedString::from(after))))
}

pub fn field_row(label: impl Into<SharedString>, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
        .h(px(26.0))
        .child(
            div()
                .w(px(110.0))
                .flex_none()
                .text_size(px(12.0))
                .text_color(gpui::rgb(palette().text_dim))
                .child(label.into()),
        )
        .child(control)
}

/// Centred modal frame with a title bar and an action row. `width` is
/// what the dialog asks for; a window narrower than that (a phone)
/// gets the dialog at the window's width less a margin, its text
/// rewrapped, and a dialog taller than the window scrolls its body.
pub fn modal_frame(
    title: impl Into<SharedString>,
    width: f32,
    body: impl IntoElement,
    actions: impl IntoElement,
) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .flex()
        .items_center()
        .justify_center()
        .p_2()
        .bg(gpui::rgba(0x00000080))
        // The backdrop must swallow the pointer, or the canvas underneath
        // keeps its hit box and the active tool edits the document while
        // the dialog is open -- dragging a slider would also drag the layer.
        .occlude()
        .child(
            div()
                .flex()
                .flex_col()
                .w(px(width))
                .max_w_full()
                .max_h_full()
                .p_3()
                .gap_2()
                .rounded_md()
                .bg(gpui::rgb(palette().panel_bg))
                .border_1()
                .border_color(gpui::rgb(palette().edge))
                .shadow_lg()
                .text_color(gpui::rgb(palette().text))
                .child(
                    div()
                        .text_size(px(13.0))
                        .pb_1()
                        .border_b_1()
                        .border_color(gpui::rgb(palette().divider))
                        .child(title.into()),
                )
                .child(
                    div()
                        .id("modal-body")
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .child(body),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_none()
                        .justify_end()
                        .gap_2()
                        .pt_2()
                        .child(actions),
                ),
        )
}

use gpui::prelude::FluentBuilder as _;

#[cfg(test)]
mod tests {
    use super::{caret_left, caret_right, type_ahead_target};

    #[test]
    fn type_ahead_finds_the_first_row_starting_with_the_word() {
        let rows = [
            "Normal",
            "Dissolve",
            "Darken",
            "Multiply",
            "Color Burn",
            "Lighten",
        ];
        assert_eq!(type_ahead_target(&rows, "d", None), Some(1));
        assert_eq!(type_ahead_target(&rows, "Da", None), Some(2));
        assert_eq!(type_ahead_target(&rows, "col", Some(5)), Some(4));
        assert_eq!(type_ahead_target(&rows, "z", None), None);
        assert_eq!(type_ahead_target(&rows, "", None), None);
    }

    #[test]
    fn a_repeated_letter_cycles_through_its_rows() {
        let rows = [
            "Normal",
            "Dissolve",
            "Darken",
            "Multiply",
            "Darker Color",
            "Lighten",
        ];
        // The first press finds the first D; each further press moves on
        // from wherever the keyboard is, wrapping at the end.
        assert_eq!(type_ahead_target(&rows, "d", None), Some(1));
        assert_eq!(type_ahead_target(&rows, "dd", Some(1)), Some(2));
        assert_eq!(type_ahead_target(&rows, "ddd", Some(2)), Some(4));
        assert_eq!(type_ahead_target(&rows, "dddd", Some(4)), Some(1));
        // But a real prefix wins over cycling: "aa" finds Aardvark.
        let rows = ["Abel", "Aardvark", "Arial"];
        assert_eq!(type_ahead_target(&rows, "aa", Some(0)), Some(1));
    }

    #[test]
    fn the_caret_moves_by_whole_characters_and_stays_in_bounds() {
        // "aé🙂" — one, two and four byte characters.
        let s = "a\u{e9}\u{1f642}";
        assert_eq!(caret_right(s, 0), 1);
        assert_eq!(caret_right(s, 1), 3);
        assert_eq!(caret_right(s, 3), 7);
        // At (or past) the end there is nowhere further to go.
        assert_eq!(caret_right(s, 7), 7);
        assert_eq!(caret_right(s, 99).min(s.len()), 7);
        assert_eq!(caret_left(s, 7), 3);
        assert_eq!(caret_left(s, 3), 1);
        assert_eq!(caret_left(s, 1), 0);
        assert_eq!(caret_left(s, 0), 0);
        assert_eq!(caret_left(s, 99), 3);
        assert_eq!(caret_left("", 0), 0);
    }
}

/// A one-line text box's state: the text, a caret on a char boundary,
/// a whole-line selection, and whether it is taking keystrokes. The
/// gallery search boxes (local and cloud) share it, so typing, ⌘A,
/// paste and the arrows behave identically in both.
#[derive(Clone, Debug, Default)]
pub struct LineEdit {
    pub text: String,
    /// Byte position, always on a char boundary — arrows move it,
    /// typing inserts at it.
    pub cursor: usize,
    /// ⌘A selected the whole line: the next keystroke replaces it,
    /// backspace clears it, ⌘C/⌘X take it — the minimal selection a
    /// one-line box owes the keyboard.
    pub selected: bool,
    pub active: bool,
}

/// What a keystroke did to a [`LineEdit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEditKey {
    /// Not for the box; let it propagate.
    Ignored,
    /// The caret or selection moved; the text is unchanged.
    Moved,
    /// The text changed.
    Changed,
    /// Enter: the box gave the keyboard back.
    Submitted,
}

impl LineEdit {
    /// A click lands a caret at the end, not a selection.
    pub fn focus(&mut self) {
        self.active = true;
        self.selected = false;
        self.cursor = self.text.len();
    }

    /// Empty and inactive.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selected = false;
        self.active = false;
    }

    pub fn set_text(&mut self, text: String) {
        self.cursor = text.len();
        self.text = text;
        self.selected = false;
    }

    fn replace_selection(&mut self) {
        if self.selected {
            self.text.clear();
            self.cursor = 0;
            self.selected = false;
        }
    }

    /// A keystroke while the box is active. `cx` is for the clipboard.
    pub fn key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut gpui::App) -> LineEditKey {
        if !self.active {
            return LineEditKey::Ignored;
        }
        let primary = ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control;
        // Keep the caret on the rails whatever changed the text.
        self.cursor = self.cursor.min(self.text.len());
        match ev.keystroke.key.as_str() {
            "a" if primary => {
                self.selected = !self.text.is_empty();
                self.cursor = self.text.len();
                LineEditKey::Moved
            }
            "c" if primary && self.selected => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(self.text.clone()));
                LineEditKey::Moved
            }
            "x" if primary && self.selected => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(self.text.clone()));
                self.replace_selection();
                LineEditKey::Changed
            }
            "v" if primary => {
                let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return LineEditKey::Moved;
                };
                // One line: a pasted paragraph flattens rather than
                // breaking the box.
                let pasted: String = pasted
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                self.replace_selection();
                let at = self.cursor;
                self.text.insert_str(at, &pasted);
                self.cursor = at + pasted.len();
                LineEditKey::Changed
            }
            "left" | "right" if primary => {
                // ⌘←/⌘→: the ends of the line.
                self.selected = false;
                self.cursor = if ev.keystroke.key == "left" {
                    0
                } else {
                    self.text.len()
                };
                LineEditKey::Moved
            }
            "left" => {
                self.cursor = if self.selected {
                    0
                } else {
                    caret_left(&self.text, self.cursor)
                };
                self.selected = false;
                LineEditKey::Moved
            }
            "right" => {
                self.cursor = if self.selected {
                    self.text.len()
                } else {
                    caret_right(&self.text, self.cursor).min(self.text.len())
                };
                self.selected = false;
                LineEditKey::Moved
            }
            "home" | "up" => {
                self.cursor = 0;
                self.selected = false;
                LineEditKey::Moved
            }
            "end" | "down" => {
                self.cursor = self.text.len();
                self.selected = false;
                LineEditKey::Moved
            }
            "backspace" => {
                if self.selected {
                    self.replace_selection();
                } else if self.cursor > 0 {
                    let from = caret_left(&self.text, self.cursor);
                    self.text.replace_range(from..self.cursor, "");
                    self.cursor = from;
                }
                LineEditKey::Changed
            }
            "delete" => {
                if self.selected {
                    self.replace_selection();
                } else if self.cursor < self.text.len() {
                    let to = caret_right(&self.text, self.cursor);
                    self.text.replace_range(self.cursor..to, "");
                }
                LineEditKey::Changed
            }
            "enter" => {
                self.active = false;
                self.selected = false;
                LineEditKey::Submitted
            }
            _ => {
                let Some(text) = ev.keystroke.key_char.as_deref() else {
                    return LineEditKey::Ignored;
                };
                if text.chars().any(char::is_control) {
                    return LineEditKey::Ignored;
                }
                // Typing over a selection replaces it, as anywhere.
                self.replace_selection();
                let at = self.cursor;
                self.text.insert_str(at, text);
                self.cursor = at + text.len();
                LineEditKey::Changed
            }
        }
    }
}
