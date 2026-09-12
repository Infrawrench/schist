//! The executable: every platform but Android starts here. Android loads
//! the same crate as a shared library and starts in its `android_main`
//! instead; see `lib.rs`.

fn main() {
    schist_app::main();
}
