//! WASM hydration entry point and `/coda` routing for the web build.
#![recursion_limit = "1024"]
//!
//! cargo-leptos compiles this crate as a `cdylib` with the `hydrate` feature
//! and ships the resulting `dabney.wasm` to the browser, where it hydrates the
//! server-rendered markup produced by `ui::shell`.

#[cfg(feature = "ssr")]
pub mod coda_proxy;
#[cfg(feature = "ssr")]
pub mod community_activity;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(ui::App);
}

#[cfg(feature = "ssr")]
pub fn mount_removed_pages<S>(router: axum::Router<S>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    async fn gone() -> impl axum::response::IntoResponse {
        (
            axum::http::StatusCode::GONE,
            [
                ("cache-control", "no-store"),
                ("x-robots-tag", "noindex, nofollow, noarchive"),
            ],
            "This page has been removed.",
        )
    }
    router
        .route("/koizetsu", axum::routing::get(gone))
        .route("/koizetsu/", axum::routing::get(gone))
}

#[cfg(feature = "ssr")]
pub fn mount_coda_static<S>(router: axum::Router<S>, site_root: &str) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    mount_coda_routes_with_embedded(router, site_root, None, None)
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
    mount_coda_routes_with_embedded(router, site_root, api_proxy, None)
}

#[cfg(feature = "ssr")]
pub async fn build_embedded_coda_api(site_root: &str) -> Result<axum::Router, String> {
    use std::path::PathBuf;
    use tower_http::limit::RequestBodyLimitLayer;

    let seed_path = PathBuf::from(site_root).join("coda/physician/demo.sqlite");
    let seed_bytes = std::fs::read(&seed_path)
        .map_err(|err| format!("failed to read {}: {err}", seed_path.display()))?;
    let state = physician_demo_api::DemoApiState::from_env_with(seed_bytes, |key| {
        if key == "CODA_DEMO_ALLOW_CORS" {
            return Some(std::env::var(key).ok().unwrap_or_else(|| "0".to_string()));
        }
        std::env::var(key).ok()
    })
    .await?;
    Ok(state
        .into_router()
        .layer(RequestBodyLimitLayer::new(8 * 1024 * 1024)))
}

#[cfg(feature = "ssr")]
pub fn mount_coda_routes_with_embedded<S>(
    router: axum::Router<S>,
    site_root: &str,
    api_proxy: Option<coda_proxy::CodaApiProxyConfig>,
    embedded_api: Option<axum::Router>,
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

    let mut api_router = axum::Router::new();
    if let Some(cfg) = api_proxy {
        api_router = api_router.merge(coda_proxy::coda_nested_api_proxy_router(cfg));
    } else if let Some(embedded) = embedded_api {
        api_router = api_router.nest("/physician/api", embedded);
    } else {
        api_router = api_router.merge(coda_proxy::coda_nested_api_disabled_router());
    }

    let static_router = axum::Router::new()
        .fallback_service(tower_http::services::ServeDir::new(coda_dir.as_ref()))
        .layer(from_fn(coda_static_cache_headers))
        .layer(tower_http::compression::CompressionLayer::new());

    let coda_subrouter = api_router.fallback_service(static_router);

    router
        .nest_service("/coda", coda_subrouter)
        .layer(from_fn(redirect_coda_directories))
}

#[cfg(feature = "ssr")]
async fn coda_static_cache_headers(
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{header, HeaderValue};

    // Nix store files carry mtime = 1970-01-01 00:00:01 UTC; strip conditional
    // mtime headers so browsers with an older 1970 cache entry never get a
    // false `304 Not Modified` across deployments, and strip `Last-Modified`
    // from responses to prevent multi-year RFC 9111 heuristic freshness.
    req.headers_mut().remove(header::IF_MODIFIED_SINCE);
    req.headers_mut().remove(header::IF_UNMODIFIED_SINCE);

    let path = req.uri().path().to_owned();
    let has_version_query = req
        .uri()
        .query()
        .is_some_and(|q| q.split('&').any(|kv| kv.starts_with("v=") && kv.len() > 2));

    let mut resp = next.run(req).await;
    resp.headers_mut().remove(header::LAST_MODIFIED);

    if resp.status().is_success() {
        let is_html = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("text/html"));
        let immutable = !is_html && (has_version_query || path.starts_with("/assets/"));
        let cache_value = if immutable {
            HeaderValue::from_static("public, max-age=31536000, immutable")
        } else {
            HeaderValue::from_static("no-cache")
        };
        resp.headers_mut()
            .insert(header::CACHE_CONTROL, cache_value);
    }
    resp
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
    async fn removed_creator_page_is_gone_without_content_or_navigation() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = super::mount_removed_pages(Router::new())
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .with_state(options);
        for path in ["/koizetsu", "/koizetsu/", "/koizetsu?source=old-link"] {
            for method in [Method::GET, Method::HEAD] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method.clone())
                            .uri(path)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::GONE);
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
                assert_eq!(
                    response.headers()["x-robots-tag"],
                    "noindex, nofollow, noarchive"
                );
                let body = to_bytes(response.into_body(), 1024).await.unwrap();
                if method == Method::HEAD {
                    assert!(body.is_empty());
                } else {
                    assert_eq!(body.as_ref(), b"This page has been removed.");
                }
            }
        }
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(!String::from_utf8(body.to_vec())
            .unwrap()
            .to_lowercase()
            .contains("koizetsu"));
    }

    #[tokio::test]
    async fn creators_is_server_rendered_with_safe_metadata_and_linked_from_home() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = Router::new()
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .with_state(options);

        for path in ["/creators", "/creators?source=footer", "/"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let html = String::from_utf8(body.to_vec()).unwrap();
            let head = html.split("</head>").next().expect("document head");
            let footer = html.split("<footer").nth(1).expect("shared footer");
            assert!(footer.contains("href=\"/creators\""), "{path}");
            assert!(footer.contains("Creators"), "{path}");

            if path.starts_with("/creators") {
                assert!(html.contains(">mnnaz05</h2>"));
                assert!(html.contains("href=\"https://www.twitch.tv/mnnaz05\""));
                assert_eq!(head.matches("<title>").count(), 1);
                assert!(head.contains("<title>Creators | dabney.moe</title>"));
                // Link serializes attributes in component-defined order, not
                // source order. Check the same canonical tag without relying on order.
                let canonicals: Vec<_> = head
                    .split('<')
                    .filter(|tag| tag.starts_with("link ") && tag.contains("rel=\"canonical\""))
                    .collect();
                assert_eq!(canonicals.len(), 1, "{path}: canonical link count");
                assert!(canonicals[0].contains("href=\"https://dabney.moe/creators\""));
                for tag in [
                    "name=\"description\" content=\"mnnaz05 on Twitch.\"",
                    "property=\"og:title\" content=\"Creators | dabney.moe\"",
                    "property=\"og:description\" content=\"mnnaz05 on Twitch.\"",
                    "property=\"og:url\" content=\"https://dabney.moe/creators\"",
                    "name=\"twitter:card\" content=\"summary\"",
                    "name=\"twitter:title\" content=\"Creators | dabney.moe\"",
                    "name=\"twitter:description\" content=\"mnnaz05 on Twitch.\"",
                ] {
                    assert!(head.contains(tag), "missing creator metadata: {tag}");
                }
                for forbidden in [
                    "Tyler",
                    "tyler-dunes",
                    "name=\"author\"",
                    "og:image",
                    "twitter:image",
                ] {
                    assert!(
                        !head.contains(forbidden),
                        "unexpected creator metadata: {forbidden}"
                    );
                }
                for forbidden in [
                    "signing",
                    "signed",
                    "announcement",
                    "<iframe",
                    "player.twitch.tv",
                ] {
                    assert!(
                        !html.to_lowercase().contains(forbidden),
                        "unexpected creator content: {forbidden}"
                    );
                }
            } else {
                assert!(head.contains("Tyler Port - Freelance Software Engineer"));
                assert!(head.contains("https://dabney.moe/photos/tyler-dunes.jpg"));
                assert!(!head.contains("mnnaz05"));
            }
        }
    }

    #[tokio::test]
    async fn community_policy_is_server_rendered_and_linked_from_home() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = Router::new()
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .with_state(options);

        for path in ["/community-policy", "/"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let html = String::from_utf8(body.to_vec()).unwrap();
            assert!(html.contains("href=\"/community-policy\""), "{path}");
            assert!(html.contains("Community policy"), "{path}");
            if path == "/community-policy" {
                for text in [
                    "Community Data and Sharing Policy",
                    "outside Discord",
                    "any previous AI use are not yet documented",
                    "not a complete data-handling disclosure",
                    "AI model training or fine-tuning",
                    "AI retrieval or reference, human training, and memorial preservation",
                    "intended purposes, not a claim",
                    "consent controls are implemented",
                    "Accepting the server rules is permission to archive and publicly publish",
                    "Remaining in the server without accepting the rules is not consent",
                    "is not permission to use them for AI",
                    "impersonate a participant with AI",
                    "each require separate, affirmative permission",
                    "Public sharing by accepting the server rules",
                    "Since November 22, 2026",
                    "Your Discord display name and user ID",
                    "which search engines may also index",
                    "Only messages from authors who have accepted the rules are published",
                    "consent to publication is not consent to an AI or training use",
                    "covers messages you posted before accepting",
                    "Until a member accepts, their earlier messages are not covered",
                    "continued membership, or silence is not acceptance",
                    "Public posting rests on acceptance of the server rules",
                    "private health, legal, or contact",
                    "Contact server moderators",
                    "Deletion capabilities and timelines",
                    "does not promise deletion from every copy",
                    "model unlearning",
                    "We cannot guarantee removal",
                    "new uses require renewed permission",
                    "Silence is not agreement",
                    "all server messages are logged",
                    "logging alone is not permission for public posting or AI uses",
                    "Community activity dashboard",
                    "Public dashboard snapshots will not include any of that material",
                    "Server message logging and the separately tracked searchable archive are not part",
                    "planned community activity dashboard",
                    "does not establish that a listener is active",
                    "message bodies, attachments, searchable conversation archives, tokens",
                    "private consent records, or backup material",
                    "separately tracked searchable archive",
                    "does not authorize posting chats",
                    "Contributing to aggregate activity statistics",
                    "Publishing identified message counts",
                    "Collecting Discord-reported presence",
                    "Publishing identified presence history",
                    "Agreement to one scope does not authorize another",
                    "Participation in the community does not require opting in",
                    "Public-sharing permission must explicitly cover persistent historical downloads",
                    "earlier, narrower explanation does not by itself authorize this expanded scope",
                    "Publication delay and dashboard visibility",
                    "at least one hour after that observation",
                    "delay applies before public snapshot uploads to Git LFS",
                    "rolling seven-day window",
                    "not a promise to delete previously published data after seven days",
                    "Presence describes what Discord reported",
                    "unknown, not as inferred inactivity",
                    "not reconstruct historical presence from message timestamps",
                    "suppress aggregates that could expose nonparticipants or unsafe small groups",
                    "Public snapshots and published history",
                    "LFS pointers, and LFS objects may remain accessible",
                    "does not reliably erase the underlying published objects",
                    "(noindex) do not control historical downloads or prevent copying",
                    "no automatic seven-day purge of published history",
                    "do not guarantee that published history will remain available forever",
                    "Activity withdrawal and removal",
                    "withdraw any activity scope independently through its announced controls",
                    "contacting server moderators privately",
                    "Withdrawal stops the affected collection",
                    "excludes the affected data from future published snapshots",
                    "remove affected individual information from the current dashboard",
                    "invalidate its current serving caches",
                    "must not reinstate withdrawn permission",
                    "Withdrawal does not reliably erase snapshots already published",
                    "Those copies may remain accessible after withdrawal",
                    "does not authorize AI training, AI retrieval, impersonation, public conversations",
                    "Material expansions require renewed affirmative permission",
                    "href=\"https://github.com/tyler274/DabneyMoeWebsite\"",
                ] {
                    assert!(html.contains(text), "missing policy text: {text}");
                }
            }
        }
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

        // Conditional 1970 mtime requests must never return 304 Not Modified,
        // Last-Modified must be stripped, HTML must get Cache-Control: no-cache,
        // and ?v= versioned assets must be gzipped + immutable.
        let cond_html = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/coda/physician/")
                    .header(header::IF_MODIFIED_SINCE, "Thu, 01 Jan 1970 00:00:01 GMT")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cond_html.status(), StatusCode::OK);
        assert!(cond_html.headers().get(header::LAST_MODIFIED).is_none());
        assert_eq!(
            cond_html
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-cache")
        );

        let versioned_js = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/coda/physician/physician-app.js?v=123456789abc")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .header(header::IF_MODIFIED_SINCE, "Thu, 01 Jan 1970 00:00:01 GMT")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(versioned_js.status(), StatusCode::OK);
        assert!(versioned_js.headers().get(header::LAST_MODIFIED).is_none());
        assert_eq!(
            versioned_js
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            versioned_js
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("gzip")
        );

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

    #[tokio::test]
    async fn embedded_coda_api_mints_demo_session_and_serves_today_and_health() {
        let public_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../public");
        let embedded = super::build_embedded_coda_api(public_root.to_str().unwrap())
            .await
            .expect("build embedded Coda physician demo API");
        let app = super::mount_coda_routes_with_embedded(
            Router::<()>::new(),
            public_root.to_str().unwrap(),
            None,
            Some(embedded),
        );

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/coda/physician/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        let session_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/coda/physician/api/auth/demo-session")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session_resp.status(), StatusCode::OK);
        let session_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(session_resp.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        let token = session_json["session"].as_str().expect("session token");

        let today = app
            .oneshot(
                Request::builder()
                    .uri("/coda/physician/api/today")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(today.status(), StatusCode::OK);
    }
}
