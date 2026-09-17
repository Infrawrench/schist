//! Schist translations. The headless ABI uses immutable English protocol labels.
#[cfg(not(schist_library))]
mod desktop;
#[cfg(not(schist_library))]
pub use desktop::*;
#[cfg(schist_library)]
mod library;
#[cfg(schist_library)]
pub use library::*;

/// Translate a compile-time choice list without runtime allocations in the
/// headless library. Desktop builds use the selected UI locale.
#[cfg(not(schist_library))]
#[macro_export]
macro_rules! choices {
    ($keys:expr) => {
        $crate::choices($keys)
    };
}
#[cfg(schist_library)]
#[macro_export]
macro_rules! choices {
    ($keys:expr) => {{
        const CHOICES: &[&str] = &{
            let keys = $keys;
            let mut values = [""; $keys.len()];
            let mut i = 0;
            while i < keys.len() {
                values[i] = $crate::t(keys[i]);
                i += 1;
            }
            values
        };
        CHOICES
    }};
}
