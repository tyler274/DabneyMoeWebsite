//! Personal access tokens (Argon2id-hashed, scoped) and the single
//! authorization rule every route asks.

use std::collections::BTreeSet;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::http::HeaderMap;
use base64::Engine;

use crate::db::{Db, RepoRecord};
use crate::error::ForgeError;
use crate::repo::Slug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// Read private repositories the user can access.
    Read,
    /// Push, open and review pull requests, merge.
    Write,
    /// Post commit statuses (CI).
    Status,
    /// Create repositories.
    Admin,
}

impl Scope {
    const ALL: [Scope; 4] = [Scope::Read, Scope::Write, Scope::Status, Scope::Admin];

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Write => "write",
            Scope::Status => "status",
            Scope::Admin => "admin",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, ForgeError> {
        Scope::ALL
            .into_iter()
            .find(|s| s.as_str() == raw)
            .ok_or_else(|| ForgeError::BadRequest(format!("unknown scope {raw:?}")))
    }
}

pub fn parse_scopes(raw: &str) -> Result<BTreeSet<Scope>, ForgeError> {
    raw.split(',')
        .filter(|s| !s.is_empty())
        .map(|s| Scope::parse(s.trim()))
        .collect()
}

/// An authenticated user and what their token allows.
#[derive(Debug, Clone)]
pub struct Principal {
    pub user: String,
    pub scopes: BTreeSet<Scope>,
}

/// Something a request wants to do to a repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Push,
    /// Open pull requests, review and comment.
    Contribute,
    Merge,
    PostStatus,
}

/// The authorization rule. Anonymous callers may only read public repos.
pub fn can(
    db: &Db,
    principal: Option<&Principal>,
    repo: &RepoRecord,
    action: Action,
) -> Result<bool, ForgeError> {
    let Some(p) = principal else {
        return Ok(action == Action::Read && !repo.private);
    };
    let member = p.scopes.contains(&Scope::Admin)
        || p.user == repo.name.owner.as_str()
        || db.is_collaborator(repo.id, &p.user)?;
    let writer = member && p.scopes.contains(&Scope::Write);
    Ok(match action {
        Action::Read => !repo.private || (member && (p.scopes.contains(&Scope::Read) || writer)),
        Action::Push | Action::Contribute | Action::Merge => writer,
        Action::PostStatus => p.scopes.contains(&Scope::Status) && (member || !repo.private),
    })
}

/// Fail with 401 (anonymous) or 403 (authenticated) unless allowed.
pub fn require(
    db: &Db,
    principal: Option<&Principal>,
    repo: &RepoRecord,
    action: Action,
) -> Result<(), ForgeError> {
    if can(db, principal, repo, action)? {
        return Ok(());
    }
    match principal {
        None => Err(ForgeError::Unauthenticated),
        Some(p) => Err(ForgeError::Forbidden(format!(
            "{} may not {action:?} {}",
            p.user, repo.name
        ))),
    }
}

fn random_hex(bytes: usize) -> Result<String, ForgeError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| ForgeError::Io(std::io::Error::other(e)))?;
    Ok(hex::encode(buf))
}

/// Issue a token `dfp_<id>_<secret>`; only its Argon2id hash is stored.
pub fn issue_token(
    db: &Db,
    user: &str,
    name: &str,
    scopes: &BTreeSet<Scope>,
) -> Result<String, ForgeError> {
    let user = Slug::parse(user)?;
    let id = random_hex(8)?;
    let token = format!("dfp_{id}_{}", random_hex(32)?);
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| ForgeError::Io(std::io::Error::other(e)))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| ForgeError::BadRequest(e.to_string()))?;
    let hash = Argon2::default()
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| ForgeError::BadRequest(e.to_string()))?
        .to_string();
    let scopes: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
    db.insert_token(&id, user.as_str(), name, &scopes.join(","), &hash)?;
    Ok(token)
}

/// Check a presented token; `None` if it is not a valid live token.
pub fn verify_token(db: &Db, token: &str) -> Result<Option<Principal>, ForgeError> {
    let mut parts = token.splitn(3, '_');
    let (Some("dfp"), Some(id), Some(_secret)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(None);
    };
    let Some(record) = db.token(id)? else {
        return Ok(None);
    };
    let Ok(hash) = PasswordHash::new(&record.hash) else {
        return Ok(None);
    };
    if Argon2::default()
        .verify_password(token.as_bytes(), &hash)
        .is_err()
    {
        return Ok(None);
    }
    Ok(Some(Principal {
        user: record.user,
        scopes: parse_scopes(&record.scopes)?,
    }))
}

/// The principal behind `Authorization` (HTTP Basic with the PAT as
/// password, or `token`/`Bearer`). No header is anonymous; a bad one is 401.
pub fn authenticate(db: &Db, headers: &HeaderMap) -> Result<Option<Principal>, ForgeError> {
    let Some(value) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| ForgeError::Unauthenticated)?;
    let (scheme, rest) = value.split_once(' ').ok_or(ForgeError::Unauthenticated)?;
    let (user, token) = match scheme.to_ascii_lowercase().as_str() {
        "basic" => {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(rest.trim())
                .map_err(|_| ForgeError::Unauthenticated)?;
            let decoded = String::from_utf8(decoded).map_err(|_| ForgeError::Unauthenticated)?;
            let (user, token) = decoded.split_once(':').ok_or(ForgeError::Unauthenticated)?;
            (Some(user.to_owned()), token.to_owned())
        }
        "token" | "bearer" => (None, rest.trim().to_owned()),
        _ => return Err(ForgeError::Unauthenticated),
    };
    let principal = verify_token(db, &token)?.ok_or(ForgeError::Unauthenticated)?;
    // A Basic username, when given, must name the token's owner.
    if user
        .as_deref()
        .is_some_and(|u| !u.is_empty() && u != principal.user)
    {
        return Err(ForgeError::Unauthenticated);
    }
    Ok(Some(principal))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::RepoName;

    #[test]
    fn tokens_verify_and_scope() {
        let db = Db::in_memory().unwrap();
        let scopes = parse_scopes("write,read").unwrap();
        let token = issue_token(&db, "alice", "laptop", &scopes).unwrap();
        let p = verify_token(&db, &token).unwrap().unwrap();
        assert_eq!(p.user, "alice");
        assert!(p.scopes.contains(&Scope::Write));
        let mut forged = token.clone();
        forged.pop();
        forged.push('0');
        assert!(verify_token(&db, &forged).unwrap().is_none() || forged == token);
        assert!(verify_token(&db, "dfp_nope_nope").unwrap().is_none());
    }

    #[test]
    fn authorization_rule() {
        let db = Db::in_memory().unwrap();
        let name = RepoName::new("tyler", "site").unwrap();
        let id = db.insert_repo(&name, false, "main").unwrap();
        let repo = db.repo_by_id(id).unwrap();
        let write = |user: &str| Principal {
            user: user.into(),
            scopes: parse_scopes("write").unwrap(),
        };
        assert!(can(&db, None, &repo, Action::Read).unwrap());
        assert!(!can(&db, None, &repo, Action::Push).unwrap());
        assert!(can(&db, Some(&write("tyler")), &repo, Action::Push).unwrap());
        assert!(!can(&db, Some(&write("mallory")), &repo, Action::Push).unwrap());
        db.add_collaborator(id, "bob").unwrap();
        assert!(can(&db, Some(&write("bob")), &repo, Action::Merge).unwrap());
        let ci = Principal {
            user: "ci".into(),
            scopes: parse_scopes("status").unwrap(),
        };
        assert!(can(&db, Some(&ci), &repo, Action::PostStatus).unwrap());
        assert!(!can(&db, Some(&ci), &repo, Action::Push).unwrap());
    }
}
