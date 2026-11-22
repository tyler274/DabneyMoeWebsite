//! Local, deliberately public fixtures; never fetch Google keys during tests.
use super::fixture::{
    EMAIL, KID, SUB, assertion, identity_assertion, jwks, key_file, sign, verifier,
};
use super::*;
use axum::http::HeaderMap;
use base64::Engine;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

const AUDIENCE: &str = "/projects/134570912572/locations/us-east1/services/public-site-staging";
fn fixture() -> Value {
    key_file()
}
fn claims(now: u64) -> Value {
    super::fixture::claims(AUDIENCE, now)
}
fn cached_gate() -> IapVerifier {
    verifier(AUDIENCE)
}
fn headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ASSERTION_HEADER, token.parse().unwrap());
    headers
}

#[test]
fn signature_claim_types_and_exact_time_boundaries() {
    let now = 2_000_000_000;
    let key = decoding_key(&jwks(), KID).unwrap();
    let good = claims(now);
    assert!(verify(&sign(&good, Some(KID)), AUDIENCE, &key, now).is_some());
    for (name, value) in [
        ("iss", json!("https://attacker.test")),
        ("iss", json!(format!("{ISSUER}/"))),
        ("aud", json!("wrong")),
        ("aud", json!([AUDIENCE])),
        ("aud", json!(null)),
        ("iat", json!(now + 30)),
        ("iat", json!(now + 31)),
        ("exp", json!(now - 30)),
        ("exp", json!(now - 31)),
        ("exp", json!(now + 651)), // lifetime 661
        ("exp", json!(now - 10)),  // exp == iat
        ("exp", json!(now - 11)),
        ("nbf", json!(now + 31)),
        ("nbf", json!(null)),
        ("nbf", json!("123")),
        ("nbf", json!(-1)),
        ("nbf", json!(1.5)),
        ("iat", json!("123")),
        ("exp", json!("123")),
        ("iat", json!(null)),
        ("exp", json!(null)),
        ("iat", json!(-1)),
        ("exp", json!(-1)),
        ("iat", json!(1.5)),
        ("exp", json!(1.5)),
        ("iat", json!(u64::MAX)),
        ("exp", json!(u64::MAX)),
    ] {
        let mut bad = good.clone();
        bad[name] = value;
        assert!(
            verify(&sign(&bad, Some(KID)), AUDIENCE, &key, now).is_none(),
            "{bad}"
        );
    }
    for field in ["iss", "aud", "iat", "exp"] {
        let mut bad = good.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(
            verify(&sign(&bad, Some(KID)), AUDIENCE, &key, now).is_none(),
            "missing {field}"
        );
    }
    for (iat, exp, nbf) in [
        (now + 29, now + 600, now + 30),
        (now - 600, now - 29, now - 600),
        (now - 10, now + 650, now),
    ] {
        let mut valid = good.clone();
        valid["iat"] = json!(iat);
        valid["exp"] = json!(exp);
        valid["nbf"] = json!(nbf);
        assert!(
            verify(&sign(&valid, Some(KID)), AUDIENCE, &key, now).is_some(),
            "{valid}"
        );
    }
    let token = sign(&good, Some(KID));
    let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
    let replacement = if parts[2].starts_with('A') { "B" } else { "A" };
    parts[2].replace_range(0..1, replacement);
    assert!(verify(&parts.join("."), AUDIENCE, &key, now).is_none());
}

#[tokio::test]
async fn identity_needs_a_valid_assertion_naming_subject_and_email() {
    let gate = cached_gate();
    let token = identity_assertion(AUDIENCE, "accounts.google.com:42", "a@codahealth.ai");
    assert_eq!(
        gate.identity(&token).await,
        Some(IapIdentity {
            sub: "accounts.google.com:42".into(),
            email: "a@codahealth.ai".into(),
        })
    );
    assert_eq!(
        gate.identity(&assertion(AUDIENCE)).await,
        Some(IapIdentity {
            sub: SUB.into(),
            email: EMAIL.into(),
        })
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for (field, value) in [
        ("email", None),
        ("email", Some(json!(""))),
        ("sub", None),
        ("sub", Some(json!(""))),
    ] {
        let mut partial = claims(now);
        match value {
            Some(value) => partial[field] = value,
            None => {
                partial.as_object_mut().unwrap().remove(field);
            }
        }
        let token = sign(&partial, Some(KID));
        assert!(gate.verifies(&token).await, "{partial}");
        assert_eq!(gate.identity(&token).await, None, "{partial}");
    }
    let other = "/projects/134570912572/locations/us-east1/services/observability-relay";
    let token = identity_assertion(other, SUB, EMAIL);
    assert!(!gate.verifies(&token).await);
    assert_eq!(gate.identity(&token).await, None);
    assert_eq!(gate.identity("garbage").await, None);
}

#[tokio::test]
async fn only_one_signed_iap_header_authorizes() {
    let gate = cached_gate();
    let token = assertion(AUDIENCE);
    assert!(gate.allows(&headers(&token)).await);
    let mut legacy = HeaderMap::new();
    for (name, value) in [
        ("cf-access-client-id", "id"),
        ("cf-access-client-secret", "secret"),
        ("cf-access-jwt-assertion", token.as_str()),
        (
            "x-goog-authenticated-user-email",
            "accounts.google.com:engineer@example.test",
        ),
        ("x-goog-authenticated-user-id", "accounts.google.com:123"),
        ("x-coda-staging-token", "probe"),
        ("authorization", "Bearer unsigned"),
    ] {
        legacy.insert(name, value.parse().unwrap());
    }
    assert!(!gate.allows(&legacy).await);
    for invalid in [
        "",
        " ",
        "garbage",
        &format!("{token}, {token}"),
        &"a".repeat(16385),
    ] {
        assert!(!gate.allows(&headers(invalid)).await);
    }
    let mut duplicate = headers(&token);
    duplicate.append("x-goog-iap-jwt-assertion", token.parse().unwrap());
    assert!(!gate.allows(&duplicate).await);
    for kid in [None, Some(""), Some("unknown"), Some(&"k".repeat(257))] {
        assert!(
            !gate
                .allows(&headers(&sign(&claims(2_000_000_000), kid)))
                .await
        );
    }
    for algorithm in [Algorithm::HS256, Algorithm::HS384, Algorithm::HS512] {
        let mut header = Header::new(algorithm);
        header.kid = Some(KID.into());
        let token = encode(
            &header,
            &claims(2_000_000_000),
            &EncodingKey::from_secret(b"not-a-key"),
        )
        .unwrap();
        assert!(!gate.allows(&headers(&token)).await);
    }
    for algorithm in ["none", "RS256", "ES384"] {
        let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
        parts[0] = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(json!({"alg": algorithm, "kid": KID}).to_string());
        assert!(!gate.allows(&headers(&parts.join("."))).await);
    }
    assert!(!IapVerifier::new(None).allows(&headers(&token)).await);
}

#[test]
fn rejects_wrong_key_type_curve_usage_algorithm_and_ambiguous_kid() {
    for (name, value) in [
        ("alg", json!("RS256")),
        ("crv", json!("P-384")),
        ("use", json!("enc")),
        ("key_ops", json!(["sign"])),
    ] {
        let mut set = fixture();
        set["keys"][0][name] = value;
        assert!(decoding_key(&serde_json::from_value(set).unwrap(), KID).is_none());
    }
    let rsa = json!({"keys":[{"kty":"RSA","kid":KID,"n":"AQAB","e":"AQAB"}]});
    assert!(decoding_key(&serde_json::from_value(rsa).unwrap(), KID).is_none());
    let mut keys = jwks();
    keys.keys.push(keys.keys[0].clone());
    assert!(decoding_key(&keys, KID).is_none());
}

// Real reqwest transport, injectable only under cfg(test). This proves redirects,
// status/size bounds and cache behavior without changing the production trust URL.
async fn source(
    status: u16,
    body: String,
) -> (IapVerifier, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let app = Router::new().route(
        "/keys",
        get(move || {
            let count = count.clone();
            let body = body.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::from_u16(status).unwrap(),
                    [("location", "/keys")],
                    body,
                )
                    .into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/keys", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut gate = IapVerifier::new(Some(AUDIENCE.into()));
    gate.client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .connect_timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    gate.key_url = url;
    (gate, calls, task)
}

#[tokio::test]
async fn transport_bounds_chunked_body_and_stalled_response() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for stalled in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            // One read is enough to know the client sent its request line.
            let read = socket.read(&mut request).await.unwrap();
            assert!(read > 0);
            if stalled {
                std::future::pending::<()>().await;
            } else {
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                    .await
                    .unwrap();
                let chunk = " ".repeat(16 * 1024);
                // No Content-Length: the incremental limit must stop this body.
                for _ in 0..17 {
                    if socket
                        .write_all(format!("4000\r\n{chunk}\r\n").as_bytes())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                let _ = socket.write_all(b"0\r\n\r\n").await;
            }
        });
        let mut gate = cached_gate();
        gate.client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .connect_timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        gate.key_url = url;
        let started = Instant::now();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), gate.fetch_keys())
                .await
                .unwrap()
                .is_none()
        );
        if stalled {
            assert!(started.elapsed() >= Duration::from_secs(2));
        }
        task.abort();
    }
}

#[tokio::test]
async fn fetch_rotation_throttle_and_singleflight() {
    let (gate, calls, task) = source(200, fixture().to_string()).await;
    let mut callers = Vec::new();
    for _ in 0..20 {
        let gate = gate.clone();
        callers.push(tokio::spawn(async move {
            gate.allows(&headers(&assertion(AUDIENCE))).await
        }));
    }
    for caller in callers {
        assert!(caller.await.unwrap());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(gate.key("unknown").await.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    {
        let mut cache = gate.cache.lock().await;
        cache.keys.as_mut().unwrap().keys[0].common.key_id = Some("old-key".into());
        cache.attempted = Some(Instant::now() - REFRESH_INTERVAL);
    }
    assert!(gate.key(KID).await.is_some()); // rotation refresh for new kid
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(gate.key("old-key").await.is_none());
    {
        let mut cache = gate.cache.lock().await;
        cache.fetched = Some(Instant::now() - CACHE_TTL);
        cache.attempted = Some(Instant::now() - REFRESH_INTERVAL);
    }
    assert!(gate.key(KID).await.is_some()); // expired cache must fetch again
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    task.abort();
}

#[tokio::test]
async fn failed_refresh_never_uses_stale_keys_and_is_throttled() {
    let (gate, calls, task) = source(503, "unavailable".into()).await;
    {
        let mut cache = gate.cache.lock().await;
        cache.keys = Some(jwks());
        cache.fetched = Some(Instant::now() - CACHE_TTL);
    }
    for _ in 0..3 {
        assert!(gate.key(KID).await.is_none());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    task.abort();
    gate.cache.lock().await.attempted = None;
    assert!(gate.key(KID).await.is_none()); // connection failure also fails closed
}

#[tokio::test]
async fn fetch_rejects_redirects_invalid_json_empty_and_oversized_sets() {
    let mut excessive = fixture();
    excessive["keys"] = Value::Array(vec![excessive["keys"][0].clone(); 33]);
    for (status, body) in [
        (302, fixture().to_string()),
        (500, fixture().to_string()),
        (200, "not json".into()),
        (200, json!({"keys":[]}).to_string()),
        (200, excessive.to_string()),
        (200, " ".repeat(MAX_KEY_BYTES + 1)),
    ] {
        let (gate, calls, task) = source(status, body).await;
        assert!(gate.key(KID).await.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
    }
}
