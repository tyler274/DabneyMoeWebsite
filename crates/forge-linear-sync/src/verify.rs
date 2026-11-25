//! Webhook authentication on the raw request body, before any parsing.

use std::fmt;

use bytes::Bytes;
use hmac::{Hmac, Mac};
use http::HeaderMap;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::domain::{Forge, RepoPath};

/// A webhook secret or token. Never printed.
#[derive(Clone)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Secret(value.into())
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Header carrying the signature or token for a forge.
pub fn auth_header(forge: Forge) -> &'static str {
    match forge {
        Forge::Forgejo | Forge::Dabney => "x-forgejo-signature",
        Forge::Gitea => "x-gitea-signature",
        Forge::GitLab => "x-gitlab-token",
    }
}

/// Hex HMAC-SHA256 of `body` under `secret`, as Forgejo and Gitea send it.
pub fn sign_hex(secret: &Secret, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.expose()).expect("HMAC accepts any key length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    #[error("missing {header} header")]
    Missing { header: &'static str },
    #[error("{header} header is not valid hex")]
    Malformed { header: &'static str },
    #[error("signature does not match any configured secret")]
    Mismatch,
}

/// Kinds of event the bridge acts on. Everything else is acknowledged with
/// `204` and ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    PullRequest,
    Unhandled,
}

impl EventKind {
    pub fn from_headers(forge: Forge, headers: &HeaderMap) -> Self {
        let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let is_pr = match forge {
            Forge::Forgejo | Forge::Gitea | Forge::Dabney => {
                let event = header("x-forgejo-event").or_else(|| header("x-gitea-event"));
                matches!(event, Some("pull_request" | "pull_request_sync"))
            }
            Forge::GitLab => header("x-gitlab-event") == Some("Merge Request Hook"),
        };
        if is_pr {
            EventKind::PullRequest
        } else {
            EventKind::Unhandled
        }
    }
}

/// A body whose signature verified against the secret of `repo`. Only
/// [`verify`] constructs one from untrusted input, so parsing an unverified
/// body is not expressible.
#[derive(Debug, Clone)]
pub struct VerifiedPayload<T> {
    forge: Forge,
    repo: RepoPath,
    delivery_id: String,
    body: T,
}

impl<T> VerifiedPayload<T> {
    pub fn forge(&self) -> Forge {
        self.forge
    }

    /// The configured repository whose secret verified this body.
    pub fn repo(&self) -> &RepoPath {
        &self.repo
    }

    pub fn delivery_id(&self) -> &str {
        &self.delivery_id
    }

    pub fn body(&self) -> &T {
        &self.body
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> VerifiedPayload<U> {
        VerifiedPayload {
            forge: self.forge,
            repo: self.repo,
            delivery_id: self.delivery_id,
            body: f(self.body),
        }
    }

    /// Rebuild a payload this process verified earlier and stored durably.
    pub(crate) fn from_trusted_store(
        forge: Forge,
        repo: RepoPath,
        delivery_id: String,
        body: T,
    ) -> Self {
        VerifiedPayload {
            forge,
            repo,
            delivery_id,
            body,
        }
    }
}

/// Delivery id from the forge's header, or a body digest when absent.
fn delivery_id(forge: Forge, headers: &HeaderMap, body: &[u8]) -> String {
    let names: &[&str] = match forge {
        Forge::Forgejo | Forge::Dabney => &["x-forgejo-delivery", "x-gitea-delivery"],
        Forge::Gitea => &["x-gitea-delivery"],
        Forge::GitLab => &["x-gitlab-event-uuid", "idempotency-key"],
    };
    names
        .iter()
        .find_map(|n| headers.get(*n).and_then(|v| v.to_str().ok()))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("sha256:{}", hex::encode(Sha256::digest(body))))
}

/// Verify `body` against every `(repo, secrets)` candidate configured for
/// `forge`. Comparison is constant-time per secret; nothing is parsed.
pub fn verify<'a>(
    forge: Forge,
    headers: &HeaderMap,
    body: Bytes,
    candidates: impl IntoIterator<Item = (&'a RepoPath, &'a [Secret])>,
) -> Result<VerifiedPayload<Bytes>, VerifyError> {
    let header = auth_header(forge);
    let presented = headers
        .get(header)
        .map(|v| v.as_bytes())
        .ok_or(VerifyError::Missing { header })?;
    let matches = |secret: &Secret| -> Result<bool, VerifyError> {
        match forge {
            Forge::GitLab => Ok(bool::from(secret.expose().ct_eq(presented))),
            Forge::Forgejo | Forge::Gitea | Forge::Dabney => {
                let text = std::str::from_utf8(presented)
                    .map_err(|_| VerifyError::Malformed { header })?;
                let text = text.strip_prefix("sha256=").unwrap_or(text);
                let expected =
                    hex::decode(text.trim()).map_err(|_| VerifyError::Malformed { header })?;
                let mut mac = Hmac::<Sha256>::new_from_slice(secret.expose())
                    .expect("HMAC accepts any key length");
                mac.update(&body);
                // `verify_slice` compares in constant time.
                Ok(mac.verify_slice(&expected).is_ok())
            }
        }
    };
    for (repo, secrets) in candidates {
        for secret in secrets {
            if matches(secret)? {
                return Ok(VerifiedPayload {
                    forge,
                    repo: repo.clone(),
                    delivery_id: delivery_id(forge, headers, &body),
                    body,
                });
            }
        }
    }
    Err(VerifyError::Mismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> RepoPath {
        RepoPath::parse("tyler/site").unwrap()
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(*k, v.parse().unwrap());
        }
        map
    }

    // RFC 4231 test case 2: key "Jefe", data "what do ya want for nothing?".
    const RFC4231_SIG: &str = "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";

    #[test]
    fn hmac_vector_matches_rfc4231() {
        assert_eq!(
            sign_hex(&Secret::new("Jefe"), b"what do ya want for nothing?"),
            RFC4231_SIG
        );
    }

    #[test]
    fn forgejo_gitea_and_dabney_accept_valid_hmac() {
        let body = Bytes::from_static(b"what do ya want for nothing?");
        let secrets = [Secret::new("Jefe")];
        let r = repo();
        for (forge, header) in [
            (Forge::Forgejo, "x-forgejo-signature"),
            (Forge::Gitea, "x-gitea-signature"),
            (Forge::Dabney, "x-forgejo-signature"),
        ] {
            let h = headers(&[
                (header, RFC4231_SIG),
                ("x-forgejo-delivery", "d-1"),
                ("x-gitea-delivery", "d-1"),
            ]);
            let v = verify(forge, &h, body.clone(), [(&r, &secrets[..])]).unwrap();
            assert_eq!(v.delivery_id(), "d-1");
            assert_eq!(v.repo(), &r);
        }
    }

    #[test]
    fn tampered_body_is_rejected() {
        let secrets = [Secret::new("Jefe")];
        let r = repo();
        let h = headers(&[("x-forgejo-signature", RFC4231_SIG)]);
        let err = verify(
            Forge::Forgejo,
            &h,
            Bytes::from_static(b"what do ya want for nothing!"),
            [(&r, &secrets[..])],
        );
        assert_eq!(err.unwrap_err(), VerifyError::Mismatch);
    }

    #[test]
    fn rotation_overlap_accepts_either_secret() {
        let secrets = [Secret::new("old"), Secret::new("Jefe")];
        let r = repo();
        let h = headers(&[("x-gitea-signature", RFC4231_SIG)]);
        assert!(verify(
            Forge::Gitea,
            &h,
            Bytes::from_static(b"what do ya want for nothing?"),
            [(&r, &secrets[..])]
        )
        .is_ok());
    }

    #[test]
    fn gitlab_token_equality() {
        let secrets = [Secret::new("s3cret-token")];
        let r = repo();
        let ok = headers(&[
            ("x-gitlab-token", "s3cret-token"),
            ("x-gitlab-event-uuid", "u-9"),
        ]);
        let v = verify(
            Forge::GitLab,
            &ok,
            Bytes::from_static(b"{}"),
            [(&r, &secrets[..])],
        )
        .unwrap();
        assert_eq!(v.delivery_id(), "u-9");
        let bad = headers(&[("x-gitlab-token", "s3cret-tokem")]);
        assert_eq!(
            verify(
                Forge::GitLab,
                &bad,
                Bytes::from_static(b"{}"),
                [(&r, &secrets[..])]
            )
            .unwrap_err(),
            VerifyError::Mismatch
        );
    }

    #[test]
    fn missing_and_malformed_signatures() {
        let secrets = [Secret::new("Jefe")];
        let r = repo();
        assert_eq!(
            verify(
                Forge::Forgejo,
                &HeaderMap::new(),
                Bytes::new(),
                [(&r, &secrets[..])]
            )
            .unwrap_err(),
            VerifyError::Missing {
                header: "x-forgejo-signature"
            }
        );
        let h = headers(&[("x-forgejo-signature", "zz-not-hex")]);
        assert!(matches!(
            verify(Forge::Forgejo, &h, Bytes::new(), [(&r, &secrets[..])]).unwrap_err(),
            VerifyError::Malformed { .. }
        ));
    }

    #[test]
    fn missing_delivery_header_falls_back_to_digest() {
        let secrets = [Secret::new("Jefe")];
        let r = repo();
        let h = headers(&[("x-forgejo-signature", RFC4231_SIG)]);
        let v = verify(
            Forge::Forgejo,
            &h,
            Bytes::from_static(b"what do ya want for nothing?"),
            [(&r, &secrets[..])],
        )
        .unwrap();
        assert!(v.delivery_id().starts_with("sha256:"));
    }
}
