//! WASM hydration entry point for the web build.
//!
//! cargo-leptos compiles this crate as a `cdylib` with the `hydrate` feature
//! and ships the resulting `dabney.wasm` to the browser, where it hydrates the
//! server-rendered markup produced by `ui::shell`.

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(ui::App);
}
