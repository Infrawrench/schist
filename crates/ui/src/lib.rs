//! Schist's widget kit: the chrome components every panel and dialog
//! builds from, and the palette they draw with.
//!
//! GPUI ships no widget library, so before this crate each panel drew
//! its own buttons from bare `div()`s, and every one of them fired its
//! action from `on_mouse_down` -- the moment the button went down, not
//! when it came back up. Native buttons (AppKit, UIKit, Win32, the web)
//! all commit on *release*, and only if the pointer is still over the
//! control; pressing, changing your mind and sliding off cancels. That
//! is what GPUI's `on_click` implements, and it is the one thing every
//! pressable component here has in common: nothing in this crate acts on
//! the press alone (issue #119).
//!
//! The components know nothing about the workspace. They take plain
//! `Fn(&ClickEvent, &mut Window, &mut App)` handlers, which is exactly
//! what `Context::listener` produces, so a panel writes
//!
//! ```ignore
//! Button::new("ok", "OK")
//!     .primary()
//!     .on_click(cx.listener(|ws, _e, window, cx| ws.confirm(window, cx)))
//! ```
//!
//! Every component is a [`gpui::RenderOnce`] value that implements
//! [`gpui::Styled`], so a caller can adjust its size or colours with the
//! usual fluent methods; those refinements win over the component's own
//! defaults. The ones that hold content also implement
//! [`gpui::ParentElement`].
//!
//! The one exception is the gestures native chrome starts on the way
//! down: a field taking the keyboard ([`TextInput::on_focus`]), a
//! dropdown opening ([`DropdownButton::on_press`]), a popup dismissing
//! on a press outside it ([`Popover::on_dismiss`]), a slider's drag
//! beginning. Those take a [`PressHandler`], and the release then goes
//! where it belongs. On a touch screen they wait for the finger to lift
//! instead, so a swipe never fires them; see [`touch`].
//!
//! The components size themselves from [`metrics`]: the desktop's
//! table, or the touch set with its 44pt targets and larger type.
//!
//! What is here:
//!
//! - pressables: [`Button`], [`IconButton`], [`Checkbox`], [`Radio`],
//!   [`Chip`], [`Link`], [`Swatch`], [`Tab`], [`ListItem`], [`MenuItem`],
//!   [`DropdownButton`];
//! - fields: [`TextInput`] (drawing a [`LineEdit`], which answers the
//!   keystrokes), [`NumberField`], [`Slider`];
//! - frames: [`Popover`], [`Modal`], [`FieldRow`], [`Heading`],
//!   [`Divider`], [`Badge`], [`ProgressBar`], [`Spinner`], [`Tooltip`].
//!
//! The components draw from [`palette`], and the ones that chrome on
//! another palette needs (the gallery's) take explicit colours:
//! [`ButtonColors`], [`TextInputColors`], [`TrackColors`],
//! [`ChipColors`].
//!
//! Icons are monochrome SVGs served by the host application's asset
//! source under `icons/<name>.svg`; see [`icon`].

mod button;
mod checkbox;
mod chip;
mod icon;
mod layout;
mod line_edit;
mod link;
mod list_item;
mod number_field;
mod popover;
mod radio;
mod slider;
mod spinner;
mod swatch;
mod tab;
mod text_input;
mod theme;
mod tooltip;

pub use button::{Button, ButtonColors, ButtonVariant, IconButton};
pub use checkbox::Checkbox;
pub use chip::{Badge, Chip, ChipColors};
pub use icon::icon;
pub use layout::{Divider, FieldRow, Heading, Modal};
pub use line_edit::{caret_left, caret_right, LineEdit, LineEditKey};
pub use link::Link;
pub use list_item::ListItem;
pub use number_field::NumberField;
pub use popover::{menu_separator, DropdownButton, MenuItem, Popover};
pub use radio::Radio;
pub use slider::{ProgressBar, Slider, TrackColors};
pub use spinner::Spinner;
pub use swatch::Swatch;
pub use tab::Tab;
pub use text_input::{TextInput, TextInputColors};
pub use theme::{
    is_light, metrics, palette, set_light, touch, Metrics, Palette, DARK, DESKTOP_METRICS, LIGHT,
    TOUCH_METRICS,
};
pub use tooltip::{tip, Tooltip};

/// What a component's `on_click` takes: fired on release, over the
/// control, or from the keyboard (Enter or Space while focused).
pub type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static>;

/// A handler told a yes-or-no: a checkbox's new state, a row's hover.
pub type BoolHandler = Box<dyn Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static>;

/// A handler with no event payload, for the secondary gestures a
/// component offers (a tab's middle-click close, for instance).
pub type Handler = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut gpui::App) + 'static>;

/// What the components that act on the *press* take: a field taking
/// the keyboard, a dropdown opening, a popup dismissing on a press
/// outside it. These are the gestures native chrome starts on the way
/// down, not the way up.
///
/// On a touch screen ([`touch`]) the press is delivered when the finger
/// lifts off the element instead: the backend cancels a press the
/// moment the finger moves, so a swipe or scroll that starts on a field
/// or a dropdown never opens it, and a tap still does. The handler gets
/// the same event either way.
pub type PressHandler =
    Box<dyn Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static>;

/// Bind a [`PressHandler`] to the left button the way the platform
/// expects: on the press itself, or on touch on the finger lifting.
fn on_press<E: gpui::InteractiveElement>(el: E, handler: PressHandler) -> E {
    if touch() {
        el.on_mouse_up(gpui::MouseButton::Left, move |ev, window, cx| {
            let press = gpui::MouseDownEvent {
                button: ev.button,
                position: ev.position,
                modifiers: ev.modifiers,
                click_count: ev.click_count,
                first_mouse: false,
                pressure: ev.pressure,
            };
            handler(&press, window, cx)
        })
    } else {
        el.on_mouse_down(gpui::MouseButton::Left, handler)
    }
}
