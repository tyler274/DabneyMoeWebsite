//! Axum SSR server for dabney.moe.
#![recursion_limit = "1024"]
//!
//! Built by cargo-leptos with the `ssr` feature. The non-ssr `main` exists only
//! so the crate still compiles when cargo-leptos builds the hydrate `cdylib`.

// Hardened (MI_SECURE) mimalloc as the process-wide allocator for the server.
#[cfg(feature = "ssr")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use ui::{shell, App};

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    let app = Router::new()
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    log!("dabney.moe listening on http://{addr}");
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
fn main() {
    // Intentionally empty: the hydrate build uses `lib.rs`, not this binary.
}
