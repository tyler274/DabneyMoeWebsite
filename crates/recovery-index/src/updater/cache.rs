//! Persistent disk cache for SAMHSA exports, CMS NPPES ZIP lookups, and facility website fetches.
//!
//! Keeps raw and extracted HTTP responses on disk under `.cache/recovery-index/` (gitignored)
//! with TTL expiration and optional ETag/Last-Modified validators so repeat runs are instant
//! and polite to upstream endpoints.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Cache lookup behavior for a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheMode {
    /// Use cached entries newer than their TTL; fetch on miss or expiry.
    Normal,
    /// Never perform network I/O; return cache hits regardless of TTL, or `None` on miss.
    Offline,
    /// Ignore TTL and revalidate/refetch (still keeping conditional ETag/Last-Modified headers).
    Refresh,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheEnvelope<T> {
    pub cached_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    pub value: T,
}

#[derive(Clone, Debug, Default)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub writes: usize,
}

#[derive(Clone, Debug)]
pub struct DiskCache {
    root: PathBuf,
    mode: CacheMode,
}

impl DiskCache {
    pub fn new(root: PathBuf, mode: CacheMode) -> Self {
        Self { root, mode }
    }

    pub fn mode(&self) -> CacheMode {
        self.mode
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, namespace: &str, key: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hex: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.root.join(namespace).join(format!("{hex}.json"))
    }

    /// Reads an entry from cache. Returns `Some(envelope)` when usable under `self.mode` and `ttl`.
    pub fn get<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &str,
        ttl: Duration,
    ) -> Option<CacheEnvelope<T>> {
        let path = self.path_for(namespace, key);
        let raw = fs::read_to_string(&path).ok()?;
        let envelope: CacheEnvelope<T> = serde_json::from_str(&raw).ok()?;
        match self.mode {
            CacheMode::Offline => Some(envelope),
            CacheMode::Refresh => None,
            CacheMode::Normal => {
                let now = now_unix();
                if now.saturating_sub(envelope.cached_at_unix) <= ttl.as_secs() {
                    Some(envelope)
                } else {
                    None
                }
            }
        }
    }

    /// Reads a cached entry even if expired, e.g. to retrieve ETag / Last-Modified headers for
    /// conditional revalidation or to fall back if an upstream request fails transiently.
    pub fn get_stale<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &str,
    ) -> Option<CacheEnvelope<T>> {
        let path = self.path_for(namespace, key);
        let raw = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn put<T: Serialize>(
        &self,
        namespace: &str,
        key: &str,
        value: &T,
        etag: Option<String>,
        last_modified: Option<String>,
    ) -> Result<(), std::io::Error> {
        let path = self.path_for(namespace, key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let envelope = CacheEnvelope {
            cached_at_unix: now_unix(),
            etag,
            last_modified,
            value,
        };
        let encoded = serde_json::to_string(&envelope)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, encoded)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn today_utc_iso() -> String {
    let secs = now_unix() as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "2026-11-27".to_string())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
