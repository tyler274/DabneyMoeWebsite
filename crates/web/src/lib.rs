//! WASM hydration entry point for the web build.
#![recursion_limit = "1024"]
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

#[cfg(feature = "ssr")]
pub fn mount_coda_static<S>(router: axum::Router<S>, site_root: &str) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    use axum::middleware::{from_fn, Next};
    use axum::response::{IntoResponse, Redirect};

    async fn redirect_bare_coda_root(
        req: axum::extract::Request,
        next: Next,
    ) -> axum::response::Response {
        if req.uri().path() == "/coda" {
            let target = match req.uri().query() {
                Some(q) if !q.is_empty() => format!("/coda/?{q}"),
                _ => "/coda/".to_string(),
            };
            return Redirect::permanent(&target).into_response();
        }
        next.run(req).await
    }

    let coda_dir = std::path::Path::new(site_root).join("coda");
    router
        .nest_service("/coda", tower_http::services::ServeDir::new(coda_dir))
        .layer(from_fn(redirect_bare_coda_root))
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use leptos_axum::generate_route_list;
    use std::path::PathBuf;
    use tower::ServiceExt;
    use ui::App;

    use super::mount_coda_static;

    #[test]
    fn app_exposes_at_least_one_route() {
        let routes = generate_route_list(App);
        assert!(
            !routes.is_empty(),
            "expected the router to expose at least one route"
        );
    }

    #[tokio::test]
    async fn coda_subpath_serves_exported_site_demo_and_book() {
        let public_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../public");
        let app = mount_coda_static(Router::<()>::new(), public_root.to_str().unwrap());

        let redirect = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/coda")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            redirect.status().is_redirection(),
            "expected /coda to redirect to /coda/, got {}",
            redirect.status()
        );

        for (uri, needle) in [
            ("/coda/", "https://dabney.moe/coda"),
            ("/coda/physician/", "physician-app.js"),
            ("/coda/book/", "book"),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
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
    }
}
