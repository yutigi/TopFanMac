//! Workspace-root shim so plain `cargo run` at the repo root launches the
//! menu-bar app, exactly like `cargo run -p menubar`. Nothing else lives
//! here -- `menubar::run` is the app.

fn main() {
    menubar::run();
}
