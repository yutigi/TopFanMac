//! Thin binary for the `menubar` crate. All logic lives in the library
//! (see lib.rs) so the workspace-root `cargo run` shim can reuse it.

fn main() {
    menubar::run();
}
