//! WASM hydration entry point and clinical API routing for the web build.
#![recursion_limit = "1024"]
//!
//! cargo-leptos compiles this crate as a `cdylib` with the `hydrate` feature
//! and ships the resulting `dabney.wasm` to the browser, where it hydrates the
//! server-rendered markup produced by `ui::shell`.

#[cfg(feature = "ssr")]
pub mod clinical_proxy;
#[cfg(feature = "ssr")]
pub mod community_activity;
#[cfg(feature = "ssr")]
pub mod ehr_roi_api;

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

/// Legacy `/coda` showcase URLs, published by earlier versions of this site
/// and by older documentation, mapped to their current homes.
#[cfg(feature = "ssr")]
pub const LEGACY_REDIRECTS: &[(&str, &str)] = &[
    ("/coda", "/clinical"),
    ("/coda/", "/clinical"),
    ("/coda/{*rest}", "/clinical"),
    ("/coda/physician", "/clinical/physician"),
    ("/coda/physician/", "/clinical/physician"),
    ("/coda/physician/{*rest}", "/clinical/physician"),
];

/// Answers each [`LEGACY_REDIRECTS`] route with `301 Moved Permanently`.
#[cfg(feature = "ssr")]
pub fn mount_legacy_redirects<S>(router: axum::Router<S>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    LEGACY_REDIRECTS
        .iter()
        .fold(router, |router, &(path, location)| {
            router.route(
                path,
                axum::routing::get(move || async move {
                    (
                        axum::http::StatusCode::MOVED_PERMANENTLY,
                        [(axum::http::header::LOCATION, location)],
                    )
                }),
            )
        })
}

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
        None => {
            let roi_webhook =
                ehr_roi_api::RoiWebhookConfig::from_env(|key| std::env::var(key).ok())
                    .unwrap_or_else(|err| {
                        tracing::warn!(error = %err, "ignoring invalid ROI_DISPATCH_WEBHOOK_URL");
                        None
                    });
            let is_prod = std::env::var("K_SERVICE").is_ok()
                || std::env::var("LEPTOS_ENV")
                    .ok()
                    .as_deref()
                    .is_some_and(|v| v.eq_ignore_ascii_case("PROD"));
            let combined =
                clinical_workspace::api::router().merge(ehr_roi_api::router(roi_webhook, !is_prod));
            router.nest_service(clinical_proxy::CLINICAL_API_PREFIX, combined)
        }
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::extract::Request as AxumRequest;
    use axum::http::{header, Method, Request, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use axum::{Json, Router};
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
    async fn legacy_coda_urls_redirect_permanently_to_clinical() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = super::mount_legacy_redirects(Router::new())
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .with_state(options);

        for (path, location) in [
            ("/coda", "/clinical"),
            ("/coda/", "/clinical"),
            ("/coda/invite", "/clinical"),
            ("/coda/some/deep/path", "/clinical"),
            ("/coda?source=old-link", "/clinical"),
            ("/coda/physician", "/clinical/physician"),
            ("/coda/physician/", "/clinical/physician"),
            ("/coda/physician/api/health", "/clinical/physician"),
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
                assert_eq!(
                    response.status(),
                    StatusCode::MOVED_PERMANENTLY,
                    "{method} {path}"
                );
                assert_eq!(
                    response.headers()[header::LOCATION],
                    location,
                    "{method} {path}"
                );
            }
        }

        // Valid routes are untouched.
        for path in ["/", "/clinical", "/clinical/physician"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
    }

    #[tokio::test]
    async fn unknown_routes_render_not_found_with_404() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::LeptosRoutes;

        let options = LeptosOptions::builder()
            .output_name("dabney")
            .site_root("target/site-test-does-not-exist")
            .build();
        let app = Router::new()
            .leptos_routes(&options, generate_route_list(App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .fallback(leptos_axum::file_and_error_handler(ui::shell))
            .with_state(options);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/definitely-not-a-route")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("Page not found"));
        assert!(html.contains("<title>Page not found | dabney.moe</title>"));
        assert!(html.contains(r#"name="robots" content="noindex""#));
        // The home page's sections are not rendered at the wrong address.
        assert!(!html.contains(r#"id="about""#));
        assert!(!html.contains(r#"id="contact""#));
    }

    #[tokio::test]
    async fn footer_links_clinical_and_nav_menus_expose_menuitem_links() {
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
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        let footer = html.split("<footer").nth(1).expect("shared footer");
        assert!(footer.contains(r#"href="/clinical""#));
        assert!(footer.contains(r#"href="/ownership""#));
        // "Built with" sits on its own line, not after a dangling middot.
        let links = footer.split("Built with").next().unwrap();
        assert!(links.ends_with(r#"<p class="text-xs text-slate-400">"#));
        // Both dropdown buttons start closed and are announced as menus.
        assert_eq!(html.matches(r#"aria-haspopup="menu""#).count(), 2);
        assert_eq!(html.matches(r#"aria-expanded="false""#).count(), 3);
        assert!(!html.contains(r#"role="menuitem""#), "menus start closed");
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
            .clone()
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

        let tools = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/tools")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(tools.status(), StatusCode::OK);
        let tools_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(tools.into_body(), 64 * 1024).await.unwrap()).unwrap();
        assert!(tools_json["physician"]
            .as_array()
            .is_some_and(|list| list.len() >= 5));
        assert!(tools_json["patient"]
            .as_array()
            .is_some_and(|list| list.len() >= 5));

        let agent_json_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/patients/pt-nadia-kulkarni/agent-chat")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"question":"Summarize her latest HbA1c, CGM time in range, and medications","audience":"physician","max_steps":6}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(agent_json_resp.status(), StatusCode::OK);
        assert_eq!(
            agent_json_resp.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let agent_json: serde_json::Value = serde_json::from_slice(
            &to_bytes(agent_json_resp.into_body(), 128 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(agent_json["patient_id"], "pt-nadia-kulkarni");
        assert_eq!(agent_json["outcome"]["state"], "finished");
        assert!(agent_json["steps"]
            .as_array()
            .is_some_and(|steps| steps.len() >= 3));

        let agent_sse_resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/patients/pt-nadia-kulkarni/agent-chat/stream")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"question":"What does my CGM show?","audience":"patient","max_steps":4}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(agent_sse_resp.status(), StatusCode::OK);
        assert_eq!(
            agent_sse_resp.headers()[header::CONTENT_TYPE],
            "text/event-stream; charset=utf-8"
        );
        assert_eq!(
            agent_sse_resp.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let sse_text = String::from_utf8(
            to_bytes(agent_sse_resp.into_body(), 128 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(sse_text.contains("event: run_started"));
        assert!(sse_text.contains("event: tool_called"));
        assert!(sse_text.contains("event: tool_completed"));
        assert!(sse_text.contains("event: answer_ready"));
        assert!(sse_text.contains("event: run_finished"));
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
    async fn ehr_portals_passport_and_roi_endpoints_work_end_to_end() {
        let app = mount_clinical_api_routes(Router::<()>::new(), None);

        // 1. Searchable EHR portal directory
        let dir = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/portals/directory?q=stanford")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dir.status(), StatusCode::OK);
        assert_eq!(
            dir.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let dir_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(dir.into_body(), 64 * 1024).await.unwrap()).unwrap();
        assert_eq!(dir_json["organizations"].as_array().unwrap().len(), 1);
        assert!(dir_json["organizations"][0]["name"]
            .as_str()
            .unwrap()
            .contains("Stanford"));

        // 2. SMART on FHIR R4 PKCE S256 launch URL generation
        let launch = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/portals/smart-launch")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "organization_id": "org-epic-sandbox",
                            "client_id": "dabney-patient-demo",
                            "redirect_uri": "https://dabney.moe/clinical/patient",
                            "state": "state-abcdef12",
                            "code_verifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(launch.status(), StatusCode::OK);
        let launch_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(launch.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(launch_json["code_challenge_method"], "S256");
        assert_eq!(
            launch_json["code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );

        // 3. FHIR R4 Bundle ingestion
        let nadia = clinical_workspace::find_patient("pt-nadia-kulkarni").unwrap();
        let bundle = clinical_workspace::sandbox_fhir_bundle_for_org("org-epic-sandbox", &nadia);
        let ingest = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/portals/ingest-fhir")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "source_label": "Epic MyChart Sandbox",
                            "portal_id": "org-epic-sandbox",
                            "bundle_json": bundle,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ingest.status(), StatusCode::OK);
        let ingest_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(ingest.into_body(), 128 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(ingest_json["records"].as_array().unwrap().len(), 4);
        assert_eq!(ingest_json["medications"].as_array().unwrap().len(), 1);

        // 4. Portable Health Passport export (JSON + FHIR IPS + HTML + Markdown)
        let export = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/passport/export")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"patient_id":"pt-nadia-kulkarni","audience":"emergency_card","emergency_note":"Sulfa rash"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(export.status(), StatusCode::OK);
        let export_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(export.into_body(), 256 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(
            export_json["passport"]["schema_version"],
            clinical_workspace::HEALTH_PASSPORT_SCHEMA
        );
        assert_eq!(export_json["fhir_ips_bundle"]["resourceType"], "Bundle");
        assert!(export_json["html"]
            .as_str()
            .unwrap()
            .contains("Emergency / Urgent Care Card"));

        // 5. Automated ROI batch dispatch (dispatches signed req, keeps draft unsigned)
        let reqs = clinical_workspace::default_roi_requests_for_patient(&nadia);
        let roi_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/roi/dispatch-live")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "requests": reqs }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(roi_resp.status(), StatusCode::OK);
        let roi_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(roi_resp.into_body(), 128 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(roi_json["mode"], "local_sandbox");
        assert_eq!(roi_json["report"]["dispatched_count"], 1);

        // 6. SMART discovery SSRF guard rejects private metadata hosts and plain HTTP
        let ssrf = app
            .oneshot(
                Request::builder()
                    .uri("/clinical/api/portals/smart-discover?iss=http://169.254.169.254/latest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ssrf.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn smart_discover_and_roi_webhook_relay_preserve_errors_and_deliver_signed_packets() {
        let mock_server = Router::new()
            .route(
                "/fhir/.well-known/smart-configuration",
                get(|| async {
                    Json(serde_json::json!({
                        "authorization_endpoint": "https://portal.example.org/oauth2/authorize",
                        "token_endpoint": "https://portal.example.org/oauth2/token",
                        "capabilities": ["launch-standalone", "client-public", "permission-patient"]
                    }))
                }),
            )
            .route(
                "/roi-hook",
                post(|req: AxumRequest| async move {
                    assert_eq!(
                        req.headers()
                            .get(header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok()),
                        Some("Bearer roi-secret-tok")
                    );
                    (StatusCode::ACCEPTED, Json(serde_json::json!({"ok": true})))
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, mock_server).await.unwrap();
        });

        let webhook = super::ehr_roi_api::RoiWebhookConfig::from_env(|key| match key {
            "ROI_DISPATCH_WEBHOOK_URL" => {
                Some(format!("http://127.0.0.1:{}/roi-hook", addr.port()))
            }
            "ROI_DISPATCH_BEARER_TOKEN" => Some("roi-secret-tok".to_string()),
            _ => None,
        })
        .unwrap();
        let router = super::ehr_roi_api::router(webhook, true);

        let disc = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/portals/smart-discover?iss=http%3A%2F%2F127.0.0.1%3A{}%2Ffhir",
                        addr.port()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(disc.status(), StatusCode::OK);
        let disc_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(disc.into_body(), 64 * 1024).await.unwrap()).unwrap();
        assert_eq!(
            disc_json["authorization_endpoint"],
            "https://portal.example.org/oauth2/authorize"
        );

        let nadia = clinical_workspace::find_patient("pt-nadia-kulkarni").unwrap();
        let reqs = clinical_workspace::default_roi_requests_for_patient(&nadia);
        let dispatch = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/roi/dispatch-live")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "requests": reqs }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dispatch.status(), StatusCode::OK);
        let dispatch_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(dispatch.into_body(), 128 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(dispatch_json["mode"], "webhook_gateway");
        assert_eq!(dispatch_json["report"]["dispatched_count"], 1);
        assert_eq!(
            dispatch_json["requests"][0]["status"]["state"],
            "dispatched"
        );
        assert_eq!(dispatch_json["requests"][1]["status"]["state"], "draft");
    }

    #[tokio::test]
    async fn shlink_inspect_kipu_verify_and_multi_format_ingest_enforce_ssrf_and_part2() {
        let app = mount_clinical_api_routes(Router::<()>::new(), None);
        let nadia = clinical_workspace::find_patient("pt-nadia-kulkarni").unwrap();

        // 1. Valid SMART Health Link inspection (offline, never echoes raw decryption key)
        let shl_uri = clinical_workspace::sandbox_smart_health_link_uri(&nadia);
        let shl_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/ehr/shlink/inspect")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "uri": shl_uri }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(shl_resp.status(), StatusCode::OK);
        assert_eq!(
            shl_resp.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        assert_eq!(shl_resp.headers()[header::VARY], "*");
        let shl_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(shl_resp.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(shl_json["valid"], true);
        assert_eq!(shl_json["key_bytes"], 32);
        assert_eq!(shl_json["passcode_required"], true);
        assert!(shl_json.get("key").is_none());

        // 2. SHLink SSRF guard rejects cloud metadata IP inside payload
        let valid_key = clinical_workspace::parse_smart_health_link(&shl_uri, None)
            .unwrap()
            .key;
        let ssrf_payload = serde_json::json!({
            "url": "https://169.254.169.254/latest/meta-data",
            "key": valid_key,
            "v": 1
        })
        .to_string();
        let bad_shl = format!(
            "shlink:/{}",
            clinical_workspace::ehr_portal::base64url_encode(ssrf_payload.as_bytes())
        );
        let ssrf_shl_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/ehr/shlink/inspect")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "uri": bad_shl }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ssrf_shl_resp.status(), StatusCode::BAD_REQUEST);

        // 3. Kipu API v3 zero-PHI sandbox envelope verification (redacted auth)
        let kipu_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/ehr/kipu/verify")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "method": "GET",
                            "path": "/api/patients/census?app_id=demo&phi_level=low",
                            "auth_scheme": "api_auth_hmac_sha1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(kipu_resp.status(), StatusCode::OK);
        assert_eq!(
            kipu_resp.headers()[header::CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        assert_eq!(kipu_resp.headers()[header::VARY], "*");
        let kipu_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(kipu_resp.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(kipu_json["mode"], "sandbox_envelope");
        assert_eq!(kipu_json["accept"], clinical_workspace::KIPU_API_V3_ACCEPT);
        assert_eq!(kipu_json["part2_protected"], true);
        assert!(kipu_json["authorization_redacted"]
            .as_str()
            .unwrap()
            .ends_with(":[REDACTED]"));

        // 4. Kipu verify fails closed on loopback/private hosts in prod mode (allow_loopback=false)
        let prod_router = super::ehr_roi_api::router(None, false);
        let kipu_ssrf = prod_router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/ehr/kipu/verify")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "base_url": "http://127.0.0.1:9999",
                            "path": "/api/patients/census"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(kipu_ssrf.status(), StatusCode::BAD_REQUEST);

        // 5. Kipu live probe against local mock server verifies real HMAC header + Accept v3
        let mock_kipu = Router::new().route(
            "/api/patients/census",
            get(|req: AxumRequest| async move {
                assert_eq!(
                    req.headers()
                        .get(header::ACCEPT)
                        .and_then(|v| v.to_str().ok()),
                    Some(clinical_workspace::KIPU_API_V3_ACCEPT)
                );
                let auth = req
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert!(auth.starts_with("APIAuth test-access:"));
                (StatusCode::OK, Json(serde_json::json!({ "patients": [] })))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, mock_kipu).await.unwrap();
        });
        let dev_router = super::ehr_roi_api::router(None, true);
        let live_kipu = dev_router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/ehr/kipu/verify")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "base_url": format!("http://127.0.0.1:{}", addr.port()),
                            "path": "/api/patients/census",
                            "access_id": "test-access",
                            "secret_key": "test-secret",
                            "live_probe": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(live_kipu.status(), StatusCode::OK);
        let live_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(live_kipu.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(live_json["mode"], "live_probe");
        assert_eq!(live_json["probe_status"], 200);
        assert_eq!(live_json["probe_ok"], true);

        // 6. Unified multi-format ingest endpoint handles C-CDA R2.1 XML & HL7 v2
        let ccda_xml = clinical_workspace::sandbox_ccda_xml_for_patient(&nadia);
        let doc_resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/clinical/api/portals/ingest-document")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "source_label": "SimplePractice C-CDA",
                            "portal_id": "org-simplepractice-clinic",
                            "document": ccda_xml
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(doc_resp.status(), StatusCode::OK);
        let doc_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(doc_resp.into_body(), 128 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(doc_json["format"], "ccda_r21_xml");
        assert!(doc_json["import"]["records"]
            .as_array()
            .is_some_and(|r| !r.is_empty()));
    }
}
