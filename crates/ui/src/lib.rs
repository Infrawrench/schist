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
//! Icons are monochrome SVGs served by the host application's asset
//! source under `icons/<name>.svg`; see [`icon`].

mod button;
mod checkbox;
mod icon;
mod link;
mod list_item;
mod swatch;
mod tab;
mod theme;
mod tooltip;

pub use button::{Button, ButtonColors, ButtonVariant, IconButton};
pub use checkbox::Checkbox;
pub use icon::icon;
pub use link::Link;
pub use list_item::ListItem;
pub use swatch::Swatch;
pub use tab::Tab;
pub use theme::{is_light, palette, set_light, Palette, DARK, LIGHT};
pub use tooltip::{tip, Tooltip};

/// What a component's `on_click` takes: fired on release, over the
/// control, or from the keyboard (Enter or Space while focused).
pub type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static>;

/// A handler told a yes-or-no: a checkbox's new state, a row's hover.
pub type BoolHandler = Box<dyn Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static>;

/// A handler with no event payload, for the secondary gestures a
/// component offers (a tab's middle-click close, for instance).
pub type Handler = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut gpui::App) + 'static>;
