//! WASM hydration entry point for the web build.
#![recursion_limit = "256"]
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

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use leptos_axum::generate_route_list;
    use ui::App;

    #[test]
    fn app_exposes_at_least_one_route() {
        let routes = generate_route_list(App);
        assert!(
            !routes.is_empty(),
            "expected the router to expose at least one route"
        );
    }
}
