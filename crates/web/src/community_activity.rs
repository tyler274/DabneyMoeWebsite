//! No-store applies to both HTML and the hydration server function, including errors.
pub async fn privacy_headers(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let protected = request.uri().path().trim_end_matches('/') == "/community-activity"
        || request.uri().path().starts_with("/api/community-activity/");
    let mut response = next.run(request).await;
    if protected {
        response
            .headers_mut()
            .insert("cache-control", "private, no-store".parse().unwrap());
        response.headers_mut().insert(
            "x-robots-tag",
            "noindex, nofollow, noarchive".parse().unwrap(),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, middleware::from_fn, routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn dashboard_ssr_is_honest_and_not_indexed() {
        use leptos::prelude::LeptosOptions;
        use leptos_axum::{generate_route_list, LeptosRoutes};
        let options = LeptosOptions::builder().output_name("dabney").build();
        let app = Router::new()
            .leptos_routes(&options, generate_route_list(ui::App), {
                let options = options.clone();
                move || ui::shell(options.clone())
            })
            .layer(from_fn(privacy_headers))
            .with_state(options);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/community-activity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let bytes = axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        for text in [
            "Community activity",
            "No publishable data",
            "noindex",
            "href=\"/community-policy\"",
        ] {
            assert!(html.contains(text), "{text}");
        }
        let api = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/community-activity/snapshot")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api.status(), axum::http::StatusCode::OK);
        assert_eq!(api.headers()["cache-control"], "private, no-store");
        let body = axum::body::to_bytes(api.into_body(), 1_048_576)
            .await
            .unwrap();
        assert_eq!(&body[..], b"null");
        for text in [
            "Synthetic Alice",
            "effective_at",
            "contributions",
            "bot_token",
            "message_body",
        ] {
            assert!(!html.contains(text), "{text}");
        }
    }

    #[tokio::test]
    async fn headers_cover_page_api_and_errors_without_affecting_home() {
        let app = Router::new()
            .route("/", get(|| async { "home" }))
            .layer(from_fn(privacy_headers));
        for path in [
            "/community-activity",
            "/community-activity/",
            "/api/community-activity/snapshot",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            assert_eq!(
                response.headers()["x-robots-tag"],
                "noindex, nofollow, noarchive"
            );
        }
        let home = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(!home.headers().contains_key("x-robots-tag"));
    }
}
