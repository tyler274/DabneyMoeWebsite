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
}
