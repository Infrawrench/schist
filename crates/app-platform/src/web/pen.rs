//! Capture pen orientation before GPUI handles the same DOM pointer event.
//! The browser keeps its capture-phase routing so orientation stays paired
//! with the exact DOM sample, without fabricating mouse tilt.
use std::cell::Cell;
use wasm_bindgen::{closure::Closure, JsCast as _};

thread_local! {
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    static TILT: Cell<Option<[f32; 2]>> = const { Cell::new(None) };
}

pub fn pen_tilt() -> Option<[f32; 2]> {
    TILT.with(Cell::get)
}

pub fn install_pen_tilt() {
    if INSTALLED.with(Cell::get) {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let handler =
        Closure::<dyn FnMut(web_sys::PointerEvent)>::new(|event: web_sys::PointerEvent| {
            // GPUI dispatches every pointer event, including secondary ones;
            // keep orientation paired with that exact event instead of reusing
            // an earlier pen's orientation for another pointer.
            let tilt = if event.pointer_type() == "pen" && event.type_() != "pointercancel" {
                Some([event.tilt_x() as f32, event.tilt_y() as f32])
            } else {
                None
            };
            TILT.with(|value| value.set(tilt));
        });
    for kind in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
        if window
            .add_event_listener_with_callback_and_bool(kind, handler.as_ref().unchecked_ref(), true)
            .is_err()
        {
            // The callback may already be registered; retain it even on failure.
            log::warn!("Could not install pen orientation listener for {kind}");
        }
    }
    handler.forget(); // Single application-lifetime listener, installed once.
    INSTALLED.with(|installed| installed.set(true));
}
