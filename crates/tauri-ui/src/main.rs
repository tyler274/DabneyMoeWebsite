//! Client-side rendered entry point for the Tauri shell (Android & desktop).
//!
//! Trunk bundles this binary into static assets that Tauri serves from `dist/`.
//! It mounts the shared `ui::AppWithMode` in [`ui::AppShellMode::ClinicalDemoApp`]
//! so patients open directly into the zero-PHI Dabney.moe Clinical Demo Suite
//! with zero tracking or cookie banners.
#![recursion_limit = "1024"]

use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(|| {
        view! { <ui::AppWithMode mode=ui::AppShellMode::ClinicalDemoApp /> }
    });
}
