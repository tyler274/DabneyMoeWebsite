//! Client-side rendered entry point for the Tauri shell.
//!
//! Trunk bundles this binary into static assets that Tauri serves from `dist/`.
//! It mounts the very same `ui::App` used by the SSR web build.

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(ui::App);
}
