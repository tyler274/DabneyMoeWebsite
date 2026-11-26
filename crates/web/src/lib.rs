//! WASM hydration entry point and clinical API routing for the web build.
#![recursion_limit = "1024"]
//!
//! cargo-leptos compiles this crate as a `cdylib` with the `hydrate` feature
//! and ships the resulting `dabney.wasm` to the browser, where it hydrates the
//! server-rendered markup produced by `ui::shell`.

#[cfg(feature = "ssr")]
pub mod clinical_proxy;

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
    REMOVED_PAGES.iter().fold(router, |router, page| {
        router
            .route(page, axum::routing::get(gone))
            .route(&format!("{page}/"), axum::routing::get(gone))
    })
}

/// Paths of pages taken off the site; each answers 410 Gone, with or without a
/// trailing slash.
#[cfg(feature = "ssr")]
const REMOVED_PAGES: [&str; 3] = ["/koizetsu", "/community-policy", "/community-activity"];

#[cfg(feature = "ssr")]
pub fn mount_ownership_json<S>(router: axum::Router<S>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    async fn ownership_json() -> impl axum::response::IntoResponse {
        (
            axum::http::StatusCode::OK,
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "application/json; charset=utf-8",
                ),
                (axum::http::header::CACHE_CONTROL, "public, max-age=300"),
            ],
            ui::ownership::OWNERSHIP_METRICS_JSON,
        )
    }
    router.route("/ownership.json", axum::routing::get(ownership_json))
}

#[cfg(feature = "ssr")]
pub fn mount_clinical_api_routes<S>(
    router: axum::Router<S>,
    api_proxy: Option<clinical_proxy::ClinicalApiProxyConfig>,
) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    match api_proxy {
        Some(cfg) => router.merge(clinical_proxy::clinical_api_proxy_router(cfg)),
        None => router.nest_service(
            clinical_proxy::CLINICAL_API_PREFIX,
            clinical_workspace::api::router(),
        ),
    }
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
    use tower::ServiceExt;
    use ui::App;

    use super::clinical_proxy::ClinicalApiProxyConfig;
    use super::mount_clinical_api_routes;

    #[test]
    fn app_exposes_at_least_one_route() {
        let routes = generate_route_list(App);
        assert!(
            !routes.is_empty(),
            "expected the router to expose at least one route"
        );
    }

    #[tokio::test]
    async fn removed_pages_are_gone_without_content_or_navigation() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = super::mount_removed_pages(Router::new())
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .with_state(options);
        for path in [
            "/koizetsu",
            "/koizetsu/",
            "/koizetsu?source=old-link",
            "/community-policy",
            "/community-policy/",
            "/community-activity",
            "/community-activity/",
        ] {
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
        let home = String::from_utf8(body.to_vec()).unwrap().to_lowercase();
        for removed in [
            "koizetsu",
            "community-policy",
            "community-activity",
            "discord",
        ] {
            assert!(!home.contains(removed), "home still mentions {removed}");
        }
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

    #[test]
    fn clinical_api_proxy_config_validates_origins_and_env() {
        assert_eq!(ClinicalApiProxyConfig::from_env(|_| None).unwrap(), None);
        assert_eq!(
            ClinicalApiProxyConfig::from_env(|key| {
                (key == "CLINICAL_API_URL").then(|| "   ".to_string())
            })
            .unwrap(),
            None
        );

        // Local development accepts loopback HTTP and leaves expected_origin unset by default.
        for local_url in [
            "http://127.0.0.1:8080",
            "http://localhost:8080/",
            "http://[::1]:8080",
        ] {
            let cfg = ClinicalApiProxyConfig::new(local_url, None, false).unwrap();
            assert_eq!(cfg.expected_origin(), None);
        }

        // Non-loopback HTTP, paths, query strings, fragments, credentials, and whitespace fail.
        for invalid in [
            "http://api.example.com",
            "https://user:pass@api.example.com",
            "https://api.example.com/v1",
            "https://api.example.com/?a=1",
            "https://api.example.com/#frag",
            " https://api.example.com",
            "ftp://api.example.com",
        ] {
            assert!(
                ClinicalApiProxyConfig::new(invalid, None, false).is_err(),
                "expected {invalid:?} to fail validation"
            );
        }

        // Deployed mode rejects loopback HTTP and defaults expected_origin to https://dabney.moe.
        assert!(ClinicalApiProxyConfig::new("http://127.0.0.1:8080", None, true).is_err());
        let deployed_cfg = ClinicalApiProxyConfig::from_env(|key| match key {
            "CLINICAL_API_URL" => Some("https://clinical-api.example.com".to_string()),
            "K_SERVICE" => Some("dabney-web".to_string()),
            _ => None,
        })
        .unwrap()
        .expect("deployed proxy config");
        assert_eq!(deployed_cfg.expected_origin(), Some("https://dabney.moe"));
        assert_eq!(
            deployed_cfg.upstream().as_str(),
            "https://clinical-api.example.com/"
        );

        let custom_origin = ClinicalApiProxyConfig::from_env(|key| match key {
            "CLINICAL_API_URL" => Some("https://clinical-api.example.com".to_string()),
            "CLINICAL_PUBLIC_ORIGIN" => Some("https://staging.dabney.moe/".to_string()),
            "LEPTOS_ENV" => Some("PROD".to_string()),
            _ => None,
        })
        .unwrap()
        .expect("custom origin proxy config");
        assert_eq!(
            custom_origin.expected_origin(),
            Some("https://staging.dabney.moe")
        );
    }

    #[tokio::test]
    async fn embedded_clinical_api_serves_health_session_today_and_grounded_chat() {
        let app = mount_clinical_api_routes(Router::<()>::new(), None);

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(
            health.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let health_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(health.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(health_json["status"], "ok");
        assert_eq!(health_json["synthetic_data"], true);

        let session_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/auth/demo-session")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session_resp.status(), StatusCode::OK);
        assert_eq!(
            session_resp.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let session_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(session_resp.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        assert!(session_json["session_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));

        let today = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/today")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(today.status(), StatusCode::OK);
        assert_eq!(
            today.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let today_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(today.into_body(), 64 * 1024).await.unwrap()).unwrap();
        assert!(today_json["patients"]
            .as_array()
            .is_some_and(|pts| pts.iter().any(|p| p["id"] == "pt-nadia-kulkarni")));

        let chat = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/patients/pt-nadia-kulkarni/chat")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"question":"What is her latest HbA1c and CGM time in range?"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(chat.status(), StatusCode::OK);
        assert_eq!(
            chat.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let chat_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(chat.into_body(), 64 * 1024).await.unwrap()).unwrap();
        assert_eq!(chat_json["patient_id"], "pt-nadia-kulkarni");
        assert!(chat_json["answer"]
            .as_str()
            .is_some_and(|ans| ans.contains("6.7%")));
        assert!(chat_json["citations"]
            .as_array()
            .is_some_and(|cites| !cites.is_empty()));

        let missing = app
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/patients/pt-unknown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            missing.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
    }

    #[tokio::test]
    async fn clinical_api_proxy_streams_sse_enforces_origin_and_strips_cookies() {
        let upstream_app = Router::new()
            .route(
                "/auth/demo-session",
                post(|req: AxumRequest| async move {
                    assert!(req.headers().get(header::COOKIE).is_none());
                    assert!(req.headers().get("x-goog-iap-jwt-assertion").is_none());
                    (
                        StatusCode::OK,
                        [(header::SET_COOKIE, "leak=1")],
                        r#"{"session_id":"demo-tok","clinician_label":"Demo Clinician","mode":"remote_proxy","synthetic_data":true}"#,
                    )
                }),
            )
            .route(
                "/events",
                get(|req: AxumRequest| async move {
                    assert_eq!(req.uri().query(), Some("after=0"));
                    assert_eq!(
                        req.headers()
                            .get(header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok()),
                        Some("Bearer demo-tok")
                    );
                    let chunks = stream::iter(vec![
                        Ok::<_, Infallible>("event: chart_updated\n".to_string()),
                        Ok::<_, Infallible>(
                            "data: {\"patient_id\":\"pt-nadia-kulkarni\"}\n\n".to_string(),
                        ),
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

        let cfg = ClinicalApiProxyConfig::new(
            &format!("http://127.0.0.1:{}/", addr.port()),
            Some("https://dabney.moe"),
            false,
        )
        .unwrap();
        let app = mount_clinical_api_routes(Router::<()>::new(), Some(cfg));

        // Cross-origin POST is rejected 403 before reaching upstream.
        let bad_origin = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/auth/demo-session")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            bad_origin.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        assert_eq!(bad_origin.headers()[header::VARY], "*");

        // Missing Origin and Sec-Fetch-Site: cross-site are rejected 403.
        let missing_origin = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/auth/demo-session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_origin.status(), StatusCode::FORBIDDEN);

        let cross_site = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/auth/demo-session")
                    .header(header::ORIGIN, "https://dabney.moe")
                    .header("sec-fetch-site", "cross-site")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cross_site.status(), StatusCode::FORBIDDEN);

        // Path traversal is rejected 400.
        let traversal = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/%2e%2e/secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(traversal.status(), StatusCode::BAD_REQUEST);

        // TRACE is rejected 405.
        let trace = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::TRACE)
                    .uri("/clinical/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(trace.status(), StatusCode::METHOD_NOT_ALLOWED);

        // Valid same-origin POST succeeds, strips cookies & Set-Cookie, and forces no-store.
        let session_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/auth/demo-session")
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
        assert_eq!(
            session_resp
                .headers()
                .get(header::VARY)
                .and_then(|v| v.to_str().ok()),
            Some("*")
        );

        // SSE stream passes query + Bearer header and streams event frames unbuffered.
        let sse_resp = app
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/events?after=0")
                    .header(header::AUTHORIZATION, "Bearer demo-tok")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sse_resp.status(), StatusCode::OK);
        assert_eq!(
            sse_resp.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        assert_eq!(sse_resp.headers()[header::VARY], "*");
        let sse_bytes = to_bytes(sse_resp.into_body(), 64 * 1024).await.unwrap();
        let sse_text = String::from_utf8_lossy(&sse_bytes);
        assert!(sse_text.contains("event: chart_updated"));
        assert!(sse_text.contains("\"patient_id\":\"pt-nadia-kulkarni\""));
    }

    #[tokio::test]
    async fn patient_demo_and_privacy_pages_render_zero_phi_disclosures() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = mount_clinical_api_routes(Router::new(), None)
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .with_state(options);

        let patient_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/clinical/patient")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patient_resp.status(), StatusCode::OK);
        let patient_html = String::from_utf8(
            to_bytes(patient_resp.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(patient_html.contains("Patient Experience Demo"));
        assert!(patient_html.contains("Zero PHI"));
        assert!(patient_html.contains("Nadia Kulkarni"));

        let privacy_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/privacy")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(privacy_resp.status(), StatusCode::OK);
        let privacy_html = String::from_utf8(
            to_bytes(privacy_resp.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(privacy_html.contains("moe.dabney.demo"));
        assert!(privacy_html.contains("Zero PHI"));

        let api_resp = app
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/patient-demo/pt-nadia-kulkarni")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api_resp.status(), StatusCode::OK);
        let api_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(api_resp.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(api_json["patient"]["id"], "pt-nadia-kulkarni");
        assert_eq!(api_json["preview"]["private_note_count"], 1);
    }

    #[tokio::test]
    async fn ownership_json_endpoint_serves_valid_metrics_json() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = super::mount_ownership_json(Router::new())
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .with_state(options);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ownership.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/json; charset=utf-8"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, max-age=300"
        );
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let raw = String::from_utf8(body.to_vec()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["website"]["repo"], "DabneyMoeWebsite");
        assert_eq!(parsed["website"]["tyler_pct"], 100.0);
        assert_eq!(parsed["website"]["external_lines"], 0);
        assert_eq!(parsed["upstream"]["name"], "dabney-moe");
        assert!(parsed["generated_at"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.ends_with('Z')));
        assert!(parsed["rea_evidence"]
            .as_array()
            .is_some_and(|items| !items.is_empty()));
        let legacy_brand = ["co", "da"].concat();
        assert!(!raw.to_ascii_lowercase().contains(&legacy_brand));

        let head = app
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri("/ownership.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(
            head.headers()[header::CONTENT_TYPE],
            "application/json; charset=utf-8"
        );
        assert_eq!(head.headers()[header::CACHE_CONTROL], "public, max-age=300");
    }

    #[tokio::test]
    async fn android_distribution_page_is_server_rendered_with_verified_metadata() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = Router::new()
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .with_state(options);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/android")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        for expected in [
            ui::android_download::ANDROID_APP_PACKAGE,
            ui::android_download::FDROID_REPO_URL,
            ui::android_download::FDROID_REPO_FINGERPRINT,
            ui::android_download::APK_SHA256,
            ui::android_download::SIGNING_CERT_SHA256,
            "/fdroid-repo-qr.svg",
            "/obtainium-dabney-demo.json",
            "Zero PHI",
            "https://dabney.moe/android",
        ] {
            assert!(html.contains(expected), "missing from /android: {expected}");
        }

        // Clinical overview links to /android in Website mode.
        let clinical_resp = app
            .oneshot(
                Request::builder()
                    .uri("/clinical")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(clinical_resp.status(), StatusCode::OK);
        let clinical_html = String::from_utf8(
            to_bytes(clinical_resp.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(clinical_html.contains("href=\"/android\""));
    }
}
