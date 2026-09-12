//! The workspace's wiring for the widget kit (`schist_ui`).
//!
//! The components live in `crates/ui` and know nothing about the
//! workspace; what is here adapts them to it: handlers that take
//! `&mut Workspace`, the dialog default-action slot, the dropdown's
//! keyboard state, and the numeric fields that are edited by
//! click-to-focus plus digit keys (see `Workspace::field_key`) because
//! GPUI ships no text editor.

use crate::workspace::{Popup, Workspace};
use gpui::{div, px, Context, IntoElement, ParentElement as _, SharedString, Styled as _};
use schist_ui::{
    Button, Checkbox, DropdownButton, FieldRow, ListItem, Modal, NumberField, Popover, Slider,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub use schist_ui::{
    caret_left, caret_right, is_light, metrics, palette, set_light, tip, touch, LineEdit,
    LineEditKey,
};

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

/// Below this window width the side panels float over the canvas rather
/// than sit beside it, and the menu bar collapses to one button: a phone,
/// or a narrow Split View on an iPad.
pub const COMPACT_WIDTH: f32 = 700.0;

pub fn compact(window: &gpui::Window) -> bool {
    touch() && f32::from(window.viewport_size().width) < COMPACT_WIDTH
}

/// Start a dialog build: forget any previous dialog's default action.
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
    let inc = on_change;
    NumberField::new(id, shown)
        .suffix(suffix)
        .focused(focused)
        .on_focus(cx.listener(move |ws, _e, _w, cx| {
            ws.focus_field(id, committed.clone());
            cx.notify();
        }))
        .on_decrement(cx.listener(move |ws, _e, _w, cx| {
            dec(ws, -step);
            cx.notify();
        }))
        .on_increment(cx.listener(move |ws, _e, _w, cx| {
            inc(ws, step);
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
        .when(width > 0.0, |d| d.w(px(width)))
        .when(width <= 0.0, |d| d.flex_grow())
        .child(
            DropdownButton::new(("dropdown-button", popup_key(popup)), label)
                .w_full()
                .on_press(cx.listener(move |ws, _e, _w, cx| ws.toggle_popup(popup, cx))),
        );
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
            Popover::new("dropdown-items")
                .top(px(22.0))
                .left_0()
                .w(px(width.max(140.0)))
                .max_h(px(300.0))
                .track_scroll(&scroll.handle)
                .on_dismiss(cx.listener(|ws, _e, _w, cx| ws.close_popup(cx)))
                .children(rows),
        ));
    }
    root
}

/// A number that tells one dropdown's button from another's within a
/// frame, for its element id.
fn popup_key(popup: Popup) -> u64 {
    match popup {
        Popup::Menu(ix) => ix as u64,
        Popup::BlendModes => u64::MAX,
        Popup::Field(id) | Popup::Slider(id) => {
            use std::hash::{Hash as _, Hasher as _};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            id.hash(&mut h);
            h.finish()
        }
    }
}

/// A bare slider track that reports a 0..1 ratio while dragged. Panels and
/// dialogs both build on this.
pub fn slider_track(
    id: &'static str,
    ratio: f32,
    width: f32,
    on_change: impl Fn(&mut Workspace, f32, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    Slider::new(id, ratio)
        .w(px(width))
        .on_change(cx.listener(move |ws, r, _w, cx| {
            on_change(ws, *r, cx);
            cx.notify();
        }))
}

/// A labelled row inside a dialog.
pub fn field_row(label: impl Into<SharedString>, control: impl IntoElement) -> impl IntoElement {
    FieldRow::new(label).child(control)
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
    Modal::new(title).width(width).child(body).action(actions)
}

use gpui::prelude::FluentBuilder as _;

#[cfg(test)]
mod tests {
    use super::type_ahead_target;

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
}
