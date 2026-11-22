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
mod telemetry;

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::routing::post;
    use axum::Router;
    use http::HeaderName;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use telemetry::{init_tracing, report_client_error, scrub_path};
    use tower_http::{
        limit::RequestBodyLimitLayer,
        request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
        trace::{DefaultOnFailure, DefaultOnResponse, TraceLayer},
    };
    use tracing::Level;
    use ui::{shell, App};

    init_tracing();

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);
    let coda_api_proxy =
        web::coda_proxy::CodaApiProxyConfig::from_env(|key| std::env::var(key).ok())
            .unwrap_or_else(|err| {
                tracing::error!(error = %err, "invalid CODA_PHYSICIAN_API_URL configuration");
                std::process::exit(1);
            });
    let embedded_coda_api = if coda_api_proxy.is_some() {
        tracing::info!(
            service = "dabney-web",
            mount = web::coda_proxy::CODA_API_PREFIX,
            "enabled Coda physician live API proxy"
        );
        None
    } else {
        match web::build_embedded_coda_api(&leptos_options.site_root).await {
            Ok(router) => {
                tracing::info!(
                    service = "dabney-web",
                    mount = web::coda_proxy::CODA_API_PREFIX,
                    "enabled embedded Coda physician crates/harness API"
                );
                Some(router)
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to initialize embedded Coda physician API");
                None
            }
        }
    };

    let site_routes = Router::new()
        .route("/api/telemetry/errors", post(report_client_error))
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .layer(RequestBodyLimitLayer::new(8 * 1024))
        .with_state(leptos_options.clone());

    let app = web::mount_coda_routes_with_embedded(
        Router::new(),
        &leptos_options.site_root,
        coda_api_proxy,
        embedded_coda_api,
    )
    .merge(site_routes)
    .layer(axum::middleware::from_fn(
        web::community_activity::privacy_headers,
    ))
    .layer(SetRequestIdLayer::new(
        HeaderName::from_static("x-request-id"),
        MakeRequestUuid,
    ))
    .layer(PropagateRequestIdLayer::new(HeaderName::from_static(
        "x-request-id",
    )))
    .layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &axum::http::Request<axum::body::Body>| {
                let path = scrub_path(request.uri().path());
                tracing::info_span!(
                    "http_request",
                    service = "dabney-web",
                    method = %request.method(),
                    path = %path,
                )
            })
            .on_response(DefaultOnResponse::new().level(Level::INFO))
            .on_failure(DefaultOnFailure::new().level(Level::ERROR)),
    );
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!(%addr, service = "dabney-web", "listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap();
}

#[cfg(not(feature = "ssr"))]
fn main() {
    // Intentionally empty: the hydrate build uses `lib.rs`, not this binary.
}
