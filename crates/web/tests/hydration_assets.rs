#![cfg(feature = "ssr")]
#![recursion_limit = "256"]

use axum::{body::Body, http::Request, Router};
use leptos::prelude::LeptosOptions;
use leptos_axum::{generate_route_list, LeptosRoutes};
use tower::ServiceExt;

#[tokio::test]
async fn shell_resolves_every_asset_from_the_build_manifest() {
    let directory = std::env::temp_dir().join(format!("dabney-hydration-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let manifest = directory.join("hash.txt");
    std::fs::write(
        &manifest,
        "js: build-js\nwasm: build-wasm\ncss: build-css\n",
    )
    .unwrap();
    let options = LeptosOptions::builder()
        .output_name("dabney")
        .hash_files(true)
        .hash_file(manifest.to_str().unwrap().into())
        .build();
    let app = Router::new()
        .leptos_routes(&options, generate_route_list(ui::App), {
            let options = options.clone();
            move || ui::shell(options.clone())
        })
        .with_state(options);
    for path in ["/", "/community-activity"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        for asset in [
            "/pkg/dabney.build-js.js",
            "/pkg/dabney.build-wasm.wasm",
            "/pkg/dabney.build-css.css",
        ] {
            assert!(html.contains(asset), "{path} must reference {asset}");
        }
        for asset in ["/pkg/dabney.js", "/pkg/dabney.wasm", "/pkg/dabney.css"] {
            assert!(!html.contains(asset), "{path} must not reference {asset}");
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}
