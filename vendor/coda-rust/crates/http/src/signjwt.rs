//! Keyless IAP credentials: a service account signs a short-lived JWT for a
//! Cloud Run origin through IAM Credentials `signJwt`, and IAP admits the
//! signed JWT as that account. Only the request and response shapes live
//! here; each caller owns its transport, its access token and its timeouts.
use serde_json::{Value, json};

/// IAM Credentials' service-account collection, the default `signJwt` base.
pub const IAM_CREDENTIALS: &str =
    "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts";

/// How long a signed IAP JWT is valid. IAP refuses longer lifetimes.
pub const LIFETIME_SECONDS: u64 = 600;

/// One `signJwt` call: POST `body` to `url` with the caller's access token.
#[derive(Debug, Clone, PartialEq)]
pub struct SignJwt {
    /// `{base}/{email}:signJwt`.
    pub url: String,
    /// `{"payload": "<claims JSON>"}`.
    pub body: Value,
}

/// The `signJwt` call that makes `email` an IAP credential for `origin`
/// (`https://host`, no path), valid from `now` for [`LIFETIME_SECONDS`].
pub fn iap_request(base: &str, email: &str, origin: &str, now: u64) -> SignJwt {
    let claims = json!({
        "iss": email,
        "sub": email,
        "aud": format!("{}/*", origin.trim_end_matches('/')),
        "iat": now,
        "exp": now + LIFETIME_SECONDS,
    });
    SignJwt {
        url: format!("{}/{email}:signJwt", base.trim_end_matches('/')),
        body: json!({ "payload": claims.to_string() }),
    }
}

/// The signed JWT in a `signJwt` answer, when it has three parts.
pub fn signed_jwt(answer: &Value) -> Option<&str> {
    answer["signedJwt"]
        .as_str()
        .filter(|jwt| jwt.split('.').count() == 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_name_the_account_and_every_path_of_the_origin() {
        let call = iap_request(
            "https://iam.test/v1/projects/-/serviceAccounts/",
            "probe@p.iam.gserviceaccount.com",
            "https://physician.run.app/",
            1_000,
        );
        assert_eq!(
            call.url,
            "https://iam.test/v1/projects/-/serviceAccounts/probe@p.iam.gserviceaccount.com:signJwt"
        );
        let claims: Value = serde_json::from_str(call.body["payload"].as_str().unwrap()).unwrap();
        assert_eq!(claims["iss"], "probe@p.iam.gserviceaccount.com");
        assert_eq!(claims["sub"], "probe@p.iam.gserviceaccount.com");
        assert_eq!(claims["aud"], "https://physician.run.app/*");
        assert_eq!(claims["exp"], 1_600);
    }

    #[test]
    fn only_a_three_part_jwt_is_an_answer() {
        assert_eq!(signed_jwt(&json!({ "signedJwt": "a.b.c" })), Some("a.b.c"));
        assert_eq!(signed_jwt(&json!({ "signedJwt": "a.b" })), None);
        assert_eq!(signed_jwt(&json!({})), None);
    }
}
