//! Typed pull-request domain, parsed once at the webhook boundary.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use url::Url;

/// A forge whose webhooks this bridge understands. The variant decides the
/// signature scheme and payload shape; it comes from the route, never from
/// the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Forge {
    /// Forgejo (Codeberg). `X-Forgejo-Signature`.
    Forgejo,
    /// Gitea. `X-Gitea-Signature`.
    Gitea,
    /// GitLab. `X-Gitlab-Token`.
    GitLab,
    /// git.dabney.moe, which emits Forgejo-compatible payloads and signatures.
    Dabney,
}

impl Forge {
    pub const ALL: [Forge; 4] = [Forge::Forgejo, Forge::Gitea, Forge::GitLab, Forge::Dabney];

    pub fn as_str(self) -> &'static str {
        match self {
            Forge::Forgejo => "forgejo",
            Forge::Gitea => "gitea",
            Forge::GitLab => "gitlab",
            Forge::Dabney => "dabney",
        }
    }
}

impl fmt::Display for Forge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown forge {0:?}")]
pub struct UnknownForge(pub String);

impl FromStr for Forge {
    type Err = UnknownForge;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Forge::ALL
            .into_iter()
            .find(|f| f.as_str() == s)
            .ok_or_else(|| UnknownForge(s.to_owned()))
    }
}

/// `owner/name` (GitLab: `group/sub/name`), normalized to lowercase without
/// leading/trailing slashes or a `.git` suffix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepoPath(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid repository path {0:?}")]
pub struct InvalidRepoPath(pub String);

impl RepoPath {
    pub fn parse(raw: &str) -> Result<Self, InvalidRepoPath> {
        let trimmed = raw.trim().trim_matches('/');
        let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
        let segments: Vec<&str> = trimmed.split('/').collect();
        let valid_segment = |s: &&str| {
            !s.is_empty()
                && *s != "."
                && *s != ".."
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        if segments.len() < 2 || !segments.iter().all(valid_segment) {
            return Err(InvalidRepoPath(raw.to_owned()));
        }
        Ok(RepoPath(trimmed.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepoRef {
    pub forge: Forge,
    pub path: RepoPath,
}

/// The identity of one pull request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PrKey {
    pub repo: RepoRef,
    pub number: u64,
}

/// The Linear deduplication key: `https`/`http` only, lowercase host, no
/// fragment, no query and no trailing slash.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanonicalUrl(String);

#[derive(Debug, thiserror::Error)]
pub enum InvalidUrl {
    #[error("unparseable URL {raw:?}")]
    Parse {
        raw: String,
        #[source]
        source: url::ParseError,
    },
    #[error("URL {0:?} is not http(s) with a host")]
    Scheme(String),
}

impl CanonicalUrl {
    pub fn parse(raw: &str) -> Result<Self, InvalidUrl> {
        let mut url = Url::parse(raw).map_err(|source| InvalidUrl::Parse {
            raw: raw.to_owned(),
            source,
        })?;
        if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
            return Err(InvalidUrl::Scheme(raw.to_owned()));
        }
        url.set_fragment(None);
        url.set_query(None);
        // `Url` already lowercases registered host names.
        let mut text = url.to_string();
        while text.ends_with('/') {
            text.pop();
        }
        Ok(CanonicalUrl(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A full 40-hex (SHA-1) or 64-hex (SHA-256) commit id, lowercase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CommitSha(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid commit sha {0:?}")]
pub struct InvalidSha(pub String);

impl CommitSha {
    pub fn parse(raw: &str) -> Result<Self, InvalidSha> {
        let ok = matches!(raw.len(), 40 | 64) && raw.bytes().all(|b| b.is_ascii_hexdigit());
        if ok {
            Ok(CommitSha(raw.to_ascii_lowercase()))
        } else {
            Err(InvalidSha(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommitSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a pull request stands. `Merged` carries the commit that landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum PrState {
    Draft,
    Open,
    Merged { sha: CommitSha },
    Closed,
}

impl PrState {
    /// The stored label, also used in attachment metadata.
    pub fn label(&self) -> &'static str {
        match self {
            PrState::Draft => "draft",
            PrState::Open => "open",
            PrState::Merged { .. } => "merged",
            PrState::Closed => "closed",
        }
    }

    /// Whether the pull request can no longer change the code it proposes.
    pub fn is_finished(&self) -> bool {
        matches!(self, PrState::Merged { .. } | PrState::Closed)
    }
}

/// Whether a repository's PR events may move Linear issue state. Comes from
/// configuration, never from a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Authority {
    Canonical,
    External,
}

impl Authority {
    pub fn as_str(self) -> &'static str {
        match self {
            Authority::Canonical => "canonical",
            Authority::External => "external",
        }
    }
}

/// A Linear team key from the configured set (`COD`, `TYL`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TeamKey(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid team key {0:?}: expected 1-7 uppercase ASCII letters")]
pub struct InvalidTeamKey(pub String);

impl TeamKey {
    pub fn parse(raw: &str) -> Result<Self, InvalidTeamKey> {
        let ok = (1..=7).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_uppercase());
        if ok {
            Ok(TeamKey(raw.to_owned()))
        } else {
            Err(InvalidTeamKey(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `COD-42`: a team key from the configured set plus a positive number.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IssueKey {
    pub team: TeamKey,
    pub number: u32,
}

impl fmt::Display for IssueKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.team.as_str(), self.number)
    }
}

/// Finds issue keys in free text, matching only configured team keys so that
/// strings like `UTF-8` or `SHA-256` never link.
#[derive(Debug, Clone)]
pub struct IssueKeyMatcher {
    teams: BTreeSet<TeamKey>,
}

impl IssueKeyMatcher {
    pub fn new(teams: impl IntoIterator<Item = TeamKey>) -> Self {
        IssueKeyMatcher {
            teams: teams.into_iter().collect(),
        }
    }

    /// Keys in `text`, case-insensitive (`cod-42-slug` branch names match),
    /// bounded by non-alphanumerics on both sides.
    pub fn find_in(&self, text: &str) -> BTreeSet<IssueKey> {
        let bytes = text.as_bytes();
        let mut found = BTreeSet::new();
        let mut i = 0;
        while i < bytes.len() {
            let starts_word = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            if !starts_word || !bytes[i].is_ascii_alphabetic() {
                i += 1;
                continue;
            }
            let letters_end = i + bytes[i..]
                .iter()
                .take_while(|b| b.is_ascii_alphabetic())
                .count();
            let rest = &bytes[letters_end..];
            if rest.first() == Some(&b'-') {
                let digits = rest[1..].iter().take_while(|b| b.is_ascii_digit()).count();
                let digits_end = letters_end + 1 + digits;
                let ends_word = bytes
                    .get(digits_end)
                    .is_none_or(|b| !b.is_ascii_alphanumeric());
                if digits > 0 && ends_word {
                    let team = text[i..letters_end].to_ascii_uppercase();
                    let number = text[letters_end + 1..digits_end].parse::<u32>();
                    if let (Some(team), Ok(number)) =
                        (self.teams.iter().find(|t| t.0 == team), number)
                    {
                        if number > 0 {
                            found.insert(IssueKey {
                                team: team.clone(),
                                number,
                            });
                        }
                    }
                }
            }
            i = letters_end;
        }
        found
    }
}

/// Everything the planner needs about one pull request at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrSnapshot {
    pub key: PrKey,
    pub url: CanonicalUrl,
    pub title: String,
    pub head: CommitSha,
    pub state: PrState,
    /// Whether a merge landed on the repository's configured default branch.
    pub target_is_default_branch: bool,
    pub authority: Authority,
    pub forge_updated_at: Timestamp,
    pub issues: BTreeSet<IssueKey>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher() -> IssueKeyMatcher {
        IssueKeyMatcher::new([
            TeamKey::parse("COD").unwrap(),
            TeamKey::parse("TYL").unwrap(),
        ])
    }

    #[test]
    fn matches_only_configured_teams() {
        let keys = matcher().find_in("COD-12: fix UTF-8 and SHA-256 (tyl-3) xCOD-9 COD-0 COD-77x");
        let rendered: Vec<String> = keys.iter().map(ToString::to_string).collect();
        assert_eq!(rendered, ["COD-12", "TYL-3"]);
    }

    #[test]
    fn matches_branch_names() {
        let keys = matcher().find_in("cod-103-gitoxide-forge");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys.first().unwrap().to_string(), "COD-103");
    }

    #[test]
    fn canonical_url_normalizes() {
        let url = CanonicalUrl::parse("https://Codeberg.ORG/tyler/site/pulls/3/?x=1#top").unwrap();
        assert_eq!(url.as_str(), "https://codeberg.org/tyler/site/pulls/3");
        assert!(CanonicalUrl::parse("ftp://x/y").is_err());
    }

    #[test]
    fn repo_path_rejects_traversal() {
        assert!(RepoPath::parse("a/../b").is_err());
        assert!(RepoPath::parse("solo").is_err());
        assert_eq!(
            RepoPath::parse("/Tyler/Site.git").unwrap().as_str(),
            "tyler/site"
        );
    }
}
