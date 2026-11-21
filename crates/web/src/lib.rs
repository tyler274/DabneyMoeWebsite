//! WASM hydration entry point and `/coda` routing for the web build.
#![recursion_limit = "1024"]
//!
//! cargo-leptos compiles this crate as a `cdylib` with the `hydrate` feature
//! and ships the resulting `dabney.wasm` to the browser, where it hydrates the
//! server-rendered markup produced by `ui::shell`.

#[cfg(feature = "ssr")]
pub mod coda_proxy;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(ui::App);
}

#[cfg(feature = "ssr")]
pub fn mount_coda_static<S>(router: axum::Router<S>, site_root: &str) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    mount_coda_routes(router, site_root, None)
}

#[cfg(feature = "ssr")]
pub fn mount_coda_routes<S>(
    router: axum::Router<S>,
    site_root: &str,
    api_proxy: Option<coda_proxy::CodaApiProxyConfig>,
) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    use axum::http::Method;
    use axum::middleware::{from_fn, Next};
    use axum::response::{IntoResponse, Redirect};
    use std::path::PathBuf;
    use std::sync::Arc;

    let coda_dir = Arc::new(PathBuf::from(site_root).join("coda"));
    let redirect_coda_dir = Arc::clone(&coda_dir);

    let redirect_coda_directories = move |req: axum::extract::Request, next: Next| {
        let coda_dir = Arc::clone(&redirect_coda_dir);
        async move {
            let path = req.uri().path();
            if path == "/coda" {
                let target = append_query("/coda/", req.uri().query());
                return Redirect::permanent(&target).into_response();
            }
            if matches!(*req.method(), Method::GET | Method::HEAD)
                && !path.ends_with('/')
                && path != coda_proxy::CODA_API_PREFIX
                && !path.starts_with("/coda/physician/api/")
            {
                if let Some(rel) = path.strip_prefix("/coda/") {
                    if is_safe_relative_subpath(rel)
                        && coda_dir.join(rel).join("index.html").is_file()
                    {
                        let target = append_query(&format!("{path}/"), req.uri().query());
                        return Redirect::permanent(&target).into_response();
                    }
                }
            }
            next.run(req).await
        }
    };

    let mut coda_subrouter = axum::Router::new();
    if let Some(cfg) = api_proxy {
        coda_subrouter = coda_subrouter.merge(coda_proxy::coda_nested_api_proxy_router(cfg));
    } else {
        coda_subrouter = coda_subrouter.merge(coda_proxy::coda_nested_api_disabled_router());
    }
    let coda_subrouter =
        coda_subrouter.fallback_service(tower_http::services::ServeDir::new(coda_dir.as_ref()));

    router
        .nest_service("/coda", coda_subrouter)
        .layer(from_fn(redirect_coda_directories))
}

#[cfg(feature = "ssr")]
fn append_query(base: &str, query: Option<&str>) -> String {
    match query {
        Some(q) if !q.is_empty() => format!("{base}?{q}"),
        _ => base.to_string(),
    }
}

#[cfg(feature = "ssr")]
fn is_safe_relative_subpath(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains('\\')
        && rel
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::extract::Request as AxumRequest;
    use axum::http::{header, Method, Request, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use axum::Router;
    use futures_util::stream;
    use leptos_axum::generate_route_list;
    use std::convert::Infallible;
    use std::path::PathBuf;
    use tower::ServiceExt;
    use ui::App;

    use super::coda_proxy::CodaApiProxyConfig;
    use super::{mount_coda_routes, mount_coda_static};

    #[test]
    fn app_exposes_at_least_one_route() {
        let routes = generate_route_list(App);
        assert!(
            !routes.is_empty(),
            "expected the router to expose at least one route"
        );
    }

    #[tokio::test]
    async fn coda_subpath_serves_exported_site_demo_book_and_directory_redirects() {
        let public_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../public");
        let app = mount_coda_static(Router::<()>::new(), public_root.to_str().unwrap());

        for (bare, expected_loc) in [
            ("/coda", "/coda/"),
            ("/coda/duet?ref=hero", "/coda/duet/?ref=hero"),
            ("/coda/physician", "/coda/physician/"),
            ("/coda/book", "/coda/book/"),
        ] {
            let redirect = app
                .clone()
                .oneshot(Request::builder().uri(bare).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert!(
                redirect.status().is_redirection(),
                "expected {bare} to redirect, got {}",
                redirect.status()
            );
            assert_eq!(
                redirect
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok()),
                Some(expected_loc)
            );
        }

        for (uri, needle) in [
            ("/coda/", "https://dabney.moe/coda"),
            ("/coda/duet/", "Health Connect"),
            ("/coda/physician/", "physician-app.js"),
            ("/coda/book/", "book"),
        ] {
            let resp = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "expected 200 OK for {uri}");
            let bytes = to_bytes(resp.into_body(), 2 * 1024 * 1024).await.unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                text.contains(needle),
                "expected response for {uri} to contain {needle:?}"
            );
        }

        let unconfigured_api = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/coda/physician/api/auth/demo-session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unconfigured_api.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn coda_api_proxy_streams_sse_enforces_origin_and_strips_cookies() {
        let upstream_app = Router::new()
            .route(
                "/auth/demo-session",
                post(|req: AxumRequest| async move {
                    assert!(req.headers().get(header::COOKIE).is_none());
                    assert!(req.headers().get("x-goog-iap-jwt-assertion").is_none());
                    (
                        StatusCode::OK,
                        [(header::SET_COOKIE, "leak=1")],
                        r#"{"session":"demo-tok","physician_id":"demo-1","practice_id":"coda-demo-practice","ends_at":999}"#,
                    )
                }),
            )
            .route(
                "/patient-workspaces/events",
                get(|req: AxumRequest| async move {
                    assert_eq!(req.uri().query(), Some("after=0"));
                    assert_eq!(
                        req.headers()
                            .get(header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok()),
                        Some("Bearer demo-tok")
                    );
                    let chunks = stream::iter(vec![
                        Ok::<_, Infallible>("event: workspace_changed\n".to_string()),
                        Ok::<_, Infallible>("data: {\"workspace_id\":\"pw-1\"}\n\n".to_string()),
                    ]);
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "text/event-stream")],
                        Body::from_stream(chunks),
                    )
                        .into_response()
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, upstream_app).await.unwrap();
        });

        let cfg = CodaApiProxyConfig::new(
            &format!("http://127.0.0.1:{}/", addr.port()),
            Some("https://dabney.moe"),
            false,
        )
        .unwrap();
        let public_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../public");
        let app = mount_coda_routes(
            Router::<()>::new(),
            public_root.to_str().unwrap(),
            Some(cfg),
        );

        // Cross-origin POST is rejected 403 before reaching upstream.
        let bad_origin = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/coda/physician/api/auth/demo-session")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

        // Path traversal is rejected 400.
        let traversal = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/coda/physician/api/%2e%2e/secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(traversal.status(), StatusCode::BAD_REQUEST);

        // Valid same-origin POST succeeds, strips cookies & Set-Cookie, and forces no-store.
        let session_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/coda/physician/api/auth/demo-session")
                    .header(header::ORIGIN, "https://dabney.moe")
                    .header(header::COOKIE, "victim=secret")
                    .header("x-goog-iap-jwt-assertion", "spoofed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session_resp.status(), StatusCode::OK);
        assert!(session_resp.headers().get(header::SET_COOKIE).is_none());
        assert_eq!(
            session_resp
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("private, no-store, max-age=0")
        );

        // Valkey SSE stream passes query + Bearer header and streams event frames.
        let sse_resp = app
            .oneshot(
                Request::builder()
                    .uri("/coda/physician/api/patient-workspaces/events?after=0")
                    .header(header::AUTHORIZATION, "Bearer demo-tok")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sse_resp.status(), StatusCode::OK);
        let sse_bytes = to_bytes(sse_resp.into_body(), 64 * 1024).await.unwrap();
        let sse_text = String::from_utf8_lossy(&sse_bytes);
        assert!(sse_text.contains("event: workspace_changed"));
        assert!(sse_text.contains("\"workspace_id\":\"pw-1\""));
    }
}
