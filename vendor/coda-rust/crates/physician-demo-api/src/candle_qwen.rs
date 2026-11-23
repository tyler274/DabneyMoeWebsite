//! Pure-Rust quantized Qwen3 / Qwen2 GGUF inference via `candle-transformers` and `tokenizers`.

use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::{quantized_qwen2, quantized_qwen3};
use futures_util::StreamExt;
use harness::engine::recipe::{Observer, Provider, Request, StreamEvent, Usage};
use harness::engine::{Cancellation, Cause, Failure, Invocation, Item, RunLimits, Turn};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;
use tokio::sync::{OnceCell, Semaphore, mpsc};

/// HuggingFace URL for `Qwen3-1.7B` Q4_K_M GGUF weights.
pub const QWEN_GGUF_URL: &str =
    "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf";
/// Verified SHA-256 hex digest of `Qwen3-1.7B-Q4_K_M.gguf`.
pub const QWEN_GGUF_SHA256: &str =
    "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897";
/// Official HuggingFace URL for `Qwen3-1.7B` `tokenizer.json`.
pub const QWEN_TOKENIZER_URL: &str =
    "https://huggingface.co/Qwen/Qwen3-1.7B/resolve/main/tokenizer.json";
/// Verified SHA-256 hex digest of `Qwen3-1.7B` `tokenizer.json`.
pub const QWEN_TOKENIZER_SHA256: &str =
    "aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4";

const DEFAULT_CACHED_GGUF_FILENAME: &str = "Qwen3-1.7B-Q4_K_M.gguf";
const DEFAULT_CACHED_TOKENIZER_FILENAME: &str = "qwen3-tokenizer.json";
const DEFAULT_MODEL_LABEL: &str = "qwen3-1.7b-q4_k_m";

/// Env var specifying pre-baked local GGUF model path (skips network download).
pub const CODA_CANDLE_GGUF_PATH_ENV: &str = "CODA_CANDLE_GGUF_PATH";
/// Env var specifying pre-baked local `tokenizer.json` path.
pub const CODA_CANDLE_TOKENIZER_PATH_ENV: &str = "CODA_CANDLE_TOKENIZER_PATH";
/// Env var overriding the automatic SHA-256 cache directory (defaults to `/tmp/coda-candle-cache`).
pub const CODA_CANDLE_CACHE_DIR_ENV: &str = "CODA_CANDLE_CACHE_DIR";

const MAX_INFERENCE_CONCURRENCY: usize = 1;
const DEFAULT_MAX_NEW_TOKENS: usize = 192;

use crate::harness_product::{
    TOOL_BRIEFING_READ, TOOL_CGM_INGEST, TOOL_CHART_READ, TOOL_CHART_SEARCH, TOOL_FOLLOW_UP_CREATE,
    TOOL_FOLLOW_UP_LIST, TOOL_LINGO_CGM_INGEST, TOOL_MEMORY_READ, TOOL_MEMORY_SAVE,
    TOOL_MENTAL_HEALTH_LOG,
};

/// Errors raised while resolving, verifying, or running the Candle GGUF model.
#[derive(Debug, thiserror::Error)]
pub enum CandleError {
    /// Failed local file I/O when reading or caching model artifacts.
    #[error("candle artifact I/O error at `{path}`")]
    Io {
        /// File path involved in the failing operation.
        path: PathBuf,
        /// Underlying OS I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Failed HTTP fetch of model weights or tokenizer.
    #[error("candle artifact download failed from `{url}`")]
    Download {
        /// Source URL requested.
        url: String,
        /// Underlying HTTP transport error.
        #[source]
        source: reqwest::Error,
    },
    /// Unexpected non-success HTTP status while downloading artifact.
    #[error("candle artifact download from `{url}` returned HTTP {status}")]
    HttpStatus {
        /// Source URL requested.
        url: String,
        /// HTTP status code returned by upstream.
        status: u16,
    },
    /// Downloaded or cached artifact failed SHA-256 integrity verification.
    #[error("SHA-256 mismatch for `{path}`: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Artifact file path.
        path: PathBuf,
        /// Expected SHA-256 hex digest.
        expected: String,
        /// Computed SHA-256 hex digest.
        actual: String,
    },
    /// Configured pre-baked artifact file is missing or empty.
    #[error("missing pre-baked candle artifact at `{path}`")]
    MissingArtifact {
        /// Missing artifact path.
        path: PathBuf,
    },
    /// Failed to load or run the Candle model tensor weights.
    #[error("candle model error")]
    Model(#[from] candle_core::Error),
    /// GGUF `general.architecture` metadata is neither `qwen3` nor `qwen2`.
    #[error("unsupported GGUF architecture `{0}` (expected `qwen3` or `qwen2`)")]
    UnsupportedArchitecture(String),
    /// Failed to load or invoke the Rust tokenizer.
    #[error("tokenizer error: {0}")]
    Tokenizer(String),
    /// Inference blocking task panicked or aborted.
    #[error("candle inference worker failed")]
    Join(#[from] tokio::task::JoinError),
}

/// Configuration locating GGUF weights and `tokenizer.json` for [`CandleQwenRuntime`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandleQwenConfig {
    /// Explicit local path to a Qwen3 or Qwen2 GGUF file (for example `Qwen3-0.6B-Q8_0.gguf`,
    /// `Qwen3-1.7B-Q4_K_M.gguf`, or `Qwen3-4B-Q4_K_M.gguf`).
    pub gguf_path: Option<PathBuf>,
    /// Explicit local path to `tokenizer.json`.
    pub tokenizer_path: Option<PathBuf>,
    /// Cache directory used when explicit paths are unset.
    pub cache_dir: PathBuf,
    /// Download URL for GGUF weights.
    pub gguf_url: String,
    /// Expected SHA-256 hex digest for GGUF weights.
    pub gguf_sha256: String,
    /// Download URL for `tokenizer.json`.
    pub tokenizer_url: String,
    /// Expected SHA-256 hex digest for `tokenizer.json`.
    pub tokenizer_sha256: String,
    /// Maximum tokens generated per exchange turn.
    pub max_new_tokens: usize,
}

impl Default for CandleQwenConfig {
    fn default() -> Self {
        Self {
            gguf_path: None,
            tokenizer_path: None,
            cache_dir: std::env::temp_dir().join("coda-candle-cache"),
            gguf_url: QWEN_GGUF_URL.to_owned(),
            gguf_sha256: QWEN_GGUF_SHA256.to_owned(),
            tokenizer_url: QWEN_TOKENIZER_URL.to_owned(),
            tokenizer_sha256: QWEN_TOKENIZER_SHA256.to_owned(),
            max_new_tokens: DEFAULT_MAX_NEW_TOKENS,
        }
    }
}

impl CandleQwenConfig {
    /// Reads Candle artifact paths and cache configuration from `get`.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Self {
        let non_empty_path = |key: &str| {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let default_cfg = Self::default();
        Self {
            gguf_path: non_empty_path(CODA_CANDLE_GGUF_PATH_ENV),
            tokenizer_path: non_empty_path(CODA_CANDLE_TOKENIZER_PATH_ENV),
            cache_dir: non_empty_path(CODA_CANDLE_CACHE_DIR_ENV).unwrap_or(default_cfg.cache_dir),
            ..default_cfg
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QwenArchitecture {
    Qwen3,
    Qwen2,
}

impl QwenArchitecture {
    fn from_gguf_metadata(content: &gguf_file::Content) -> Result<Self, CandleError> {
        let arch = match content.metadata.get("general.architecture") {
            Some(gguf_file::Value::String(s)) => Some(s.trim().to_ascii_lowercase()),
            _ => None,
        };
        match arch.as_deref() {
            Some("qwen3") => Ok(Self::Qwen3),
            Some("qwen2") => Ok(Self::Qwen2),
            Some(other) => Err(CandleError::UnsupportedArchitecture(other.to_owned())),
            None => Err(CandleError::UnsupportedArchitecture("missing".to_owned())),
        }
    }

    fn provider_label(self) -> &'static str {
        match self {
            Self::Qwen3 => "candle_qwen3",
            Self::Qwen2 => "candle_qwen2",
        }
    }

    fn assistant_prefix(self) -> &'static str {
        match self {
            Self::Qwen3 => "<|im_start|>assistant\n<think>\n\n</think>\n\n",
            Self::Qwen2 => "<|im_start|>assistant\n",
        }
    }
}

enum QwenWeights {
    Qwen3(quantized_qwen3::ModelWeights),
    Qwen2(quantized_qwen2::ModelWeights),
}

impl QwenWeights {
    fn from_gguf<R: std::io::Read + std::io::Seek>(
        arch: QwenArchitecture,
        content: gguf_file::Content,
        reader: &mut R,
        device: &Device,
    ) -> candle_core::Result<Self> {
        match arch {
            QwenArchitecture::Qwen3 => Ok(Self::Qwen3(quantized_qwen3::ModelWeights::from_gguf(
                content, reader, device,
            )?)),
            QwenArchitecture::Qwen2 => Ok(Self::Qwen2(quantized_qwen2::ModelWeights::from_gguf(
                content, reader, device,
            )?)),
        }
    }

    fn clear_kv_cache(&mut self) {
        match self {
            Self::Qwen3(w) => w.clear_kv_cache(),
            Self::Qwen2(w) => w.clear_kv_cache(),
        }
    }

    fn forward(&mut self, input: &Tensor, index_pos: usize) -> candle_core::Result<Tensor> {
        match self {
            Self::Qwen3(w) => w.forward(input, index_pos),
            Self::Qwen2(w) => w.forward(input, index_pos),
        }
    }
}

struct LoadedQwen {
    architecture: QwenArchitecture,
    model_name: String,
    weights: Mutex<QwenWeights>,
    tokenizer: Tokenizer,
    eos_token_ids: Vec<u32>,
}

/// Shared lazy-loaded pure-Rust Qwen3 / Qwen2 GGUF runtime bounded by a semaphore.
#[derive(Clone)]
pub struct CandleQwenRuntime {
    config: CandleQwenConfig,
    model_label: String,
    loaded: Arc<OnceCell<Arc<LoadedQwen>>>,
    semaphore: Arc<Semaphore>,
    client: reqwest::Client,
}

impl std::fmt::Debug for CandleQwenRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CandleQwenRuntime")
            .field("cache_dir", &self.config.cache_dir)
            .field("gguf_path", &self.config.gguf_path)
            .field("model_label", &self.model_label)
            .finish_non_exhaustive()
    }
}

impl CandleQwenRuntime {
    /// Creates a runtime handle (defers disk/network loading until the first turn or `ensure_loaded`).
    pub fn new(config: CandleQwenConfig) -> Self {
        let model_label = config
            .gguf_path
            .as_deref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| DEFAULT_MODEL_LABEL.to_owned());
        Self {
            config,
            model_label,
            loaded: Arc::new(OnceCell::new()),
            semaphore: Arc::new(Semaphore::new(MAX_INFERENCE_CONCURRENCY)),
            client: reqwest::Client::new(),
        }
    }

    /// Configured model label exposed before weights load (GGUF file stem or default `qwen3-1.7b-q4_k_m`).
    pub fn model_label(&self) -> &str {
        &self.model_label
    }

    /// Creates a [`CandleQwenProvider`] sharing this runtime's loaded model weights.
    pub fn provider(&self) -> CandleQwenProvider {
        CandleQwenProvider {
            runtime: self.clone(),
        }
    }

    /// Ensures the GGUF weights and tokenizer are cached/verified and loaded into memory.
    pub async fn ensure_loaded(&self) -> Result<(), CandleError> {
        let _ = self.get_or_load().await?;
        Ok(())
    }

    async fn get_or_load(&self) -> Result<&Arc<LoadedQwen>, CandleError> {
        self.loaded
            .get_or_try_init(|| async {
                let gguf_path = resolve_artifact(
                    &self.client,
                    self.config.gguf_path.as_deref(),
                    &self.config.cache_dir.join(DEFAULT_CACHED_GGUF_FILENAME),
                    &self.config.gguf_url,
                    &self.config.gguf_sha256,
                )
                .await?;
                let tok_path = resolve_artifact(
                    &self.client,
                    self.config.tokenizer_path.as_deref(),
                    &self
                        .config
                        .cache_dir
                        .join(DEFAULT_CACHED_TOKENIZER_FILENAME),
                    &self.config.tokenizer_url,
                    &self.config.tokenizer_sha256,
                )
                .await?;

                tokio::task::spawn_blocking(move || load_qwen_blocking(&gguf_path, &tok_path))
                    .await?
            })
            .await
    }
}

fn load_qwen_blocking(gguf_path: &Path, tok_path: &Path) -> Result<Arc<LoadedQwen>, CandleError> {
    let tokenizer =
        Tokenizer::from_file(tok_path).map_err(|err| CandleError::Tokenizer(err.to_string()))?;
    let mut eos_token_ids = Vec::new();
    for eos in ["<|im_end|>", "<|endoftext|>"] {
        if let Some(id) = tokenizer.token_to_id(eos)
            && !eos_token_ids.contains(&id)
        {
            eos_token_ids.push(id);
        }
    }
    let file = File::open(gguf_path).map_err(|source| CandleError::Io {
        path: gguf_path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let content = gguf_file::Content::read(&mut reader)?;
    let architecture = QwenArchitecture::from_gguf_metadata(&content)?;
    let model_name = match content.metadata.get("general.name") {
        Some(gguf_file::Value::String(name)) if !name.trim().is_empty() => name.trim().to_owned(),
        _ => gguf_path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_MODEL_LABEL)
            .to_owned(),
    };
    let weights = QwenWeights::from_gguf(architecture, content, &mut reader, &Device::Cpu)?;
    Ok(Arc::new(LoadedQwen {
        architecture,
        model_name,
        weights: Mutex::new(weights),
        tokenizer,
        eos_token_ids,
    }))
}

async fn resolve_artifact(
    client: &reqwest::Client,
    explicit_path: Option<&Path>,
    cache_path: &Path,
    url: &str,
    expected_sha256: &str,
) -> Result<PathBuf, CandleError> {
    if let Some(path) = explicit_path {
        let meta = fs::metadata(path).map_err(|source| CandleError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if meta.len() == 0 {
            return Err(CandleError::MissingArtifact {
                path: path.to_path_buf(),
            });
        }
        return Ok(path.to_path_buf());
    }

    if cache_path.exists()
        && let Ok(actual) = file_sha256_blocking(cache_path).await
        && actual == expected_sha256
    {
        return Ok(cache_path.to_path_buf());
    }

    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|source| CandleError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let tmp_path = cache_path.with_extension(format!("part-{}", uuid::Uuid::new_v4().simple()));
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|source| CandleError::Download {
            url: url.to_owned(),
            source,
        })?;
    if !resp.status().is_success() {
        return Err(CandleError::HttpStatus {
            url: url.to_owned(),
            status: resp.status().as_u16(),
        });
    }

    let mut tmp_file = File::create(&tmp_path).map_err(|source| CandleError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|source| CandleError::Download {
            url: url.to_owned(),
            source,
        })?;
        hasher.update(&bytes);
        tmp_file
            .write_all(&bytes)
            .map_err(|source| CandleError::Io {
                path: tmp_path.clone(),
                source,
            })?;
    }
    tmp_file.flush().map_err(|source| CandleError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    drop(tmp_file);

    let actual = encode_hex(&hasher.finalize());
    if actual != expected_sha256 {
        let _ = fs::remove_file(&tmp_path);
        return Err(CandleError::ChecksumMismatch {
            path: cache_path.to_path_buf(),
            expected: expected_sha256.to_owned(),
            actual,
        });
    }
    fs::rename(&tmp_path, cache_path).map_err(|source| CandleError::Io {
        path: cache_path.to_path_buf(),
        source,
    })?;
    Ok(cache_path.to_path_buf())
}

async fn file_sha256_blocking(path: &Path) -> Result<String, CandleError> {
    let owned = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut f = File::open(&owned).map_err(|source| CandleError::Io {
            path: owned.clone(),
            source,
        })?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = f.read(&mut buf).map_err(|source| CandleError::Io {
                path: owned.clone(),
                source,
            })?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(encode_hex(&hasher.finalize()))
    })
    .await?
}

use crate::provider::wait_cancelled;

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// [`harness::engine::recipe::Provider`] backed by [`CandleQwenRuntime`].
pub struct CandleQwenProvider {
    runtime: CandleQwenRuntime,
}

struct ChartExcerpt {
    citation_number: usize,
    title: String,
    recorded_at: String,
    content: String,
}

enum PromptPhase {
    NeedTool {
        query: String,
    },
    NeedRead {
        candidate_ids: Vec<String>,
    },
    ReadyToAnswer {
        excerpts: Vec<ChartExcerpt>,
        workspace_notes: Vec<String>,
    },
}

fn is_workspace_tool(name: &str) -> bool {
    matches!(
        name,
        TOOL_MEMORY_READ
            | TOOL_MEMORY_SAVE
            | TOOL_FOLLOW_UP_LIST
            | TOOL_FOLLOW_UP_CREATE
            | TOOL_BRIEFING_READ
            | TOOL_MENTAL_HEALTH_LOG
            | TOOL_LINGO_CGM_INGEST
            | TOOL_CGM_INGEST
    )
}

fn inspect_prompt_phase(request: &Request) -> PromptPhase {
    let tools_present = request.tools.iter().any(|t| {
        let name = t
            .get("name")
            .or_else(|| t.pointer("/function/name"))
            .and_then(Value::as_str);
        matches!(name, Some(TOOL_CHART_SEARCH)) || name.is_some_and(is_workspace_tool)
    });
    if !tools_present {
        return PromptPhase::ReadyToAnswer {
            excerpts: Vec::new(),
            workspace_notes: Vec::new(),
        };
    }

    let mut calls_by_id = std::collections::HashMap::<&str, &str>::new();
    let mut searched_candidates = Vec::new();
    let mut has_search_result = false;
    let mut has_workspace_result = false;
    let mut excerpts = Vec::new();
    let mut workspace_notes = Vec::new();
    let mut last_user = String::new();

    for item in &request.items {
        match item {
            Item::Call(call) => {
                calls_by_id.insert(call.id.as_str(), call.name.as_str());
            }
            Item::MaterializedResult { call_id, output } => {
                match calls_by_id.get(call_id.as_str()).copied() {
                    Some(TOOL_CHART_SEARCH) => {
                        has_search_result = true;
                        if let Ok(parsed) = serde_json::from_str::<Value>(output)
                            && let Some(cands) = parsed.get("candidates").and_then(Value::as_array)
                        {
                            for c in cands {
                                if let Some(id) = c.get("candidate_id").and_then(Value::as_str)
                                    && !searched_candidates.iter().any(|s| s == id)
                                {
                                    searched_candidates.push(id.to_owned());
                                }
                            }
                        }
                    }
                    Some(TOOL_CHART_READ) => {
                        if let Ok(parsed) = serde_json::from_str::<Value>(output)
                            && let Some(records) = parsed.get("records").and_then(Value::as_array)
                        {
                            for (idx, rec) in records.iter().enumerate() {
                                let num = rec
                                    .get("citation_number")
                                    .and_then(Value::as_u64)
                                    .and_then(|n| usize::try_from(n).ok())
                                    .unwrap_or_else(|| excerpts.len() + idx + 1);
                                excerpts.push(ChartExcerpt {
                                    citation_number: num,
                                    title: rec
                                        .get("title")
                                        .and_then(Value::as_str)
                                        .unwrap_or("Chart record")
                                        .to_owned(),
                                    recorded_at: rec
                                        .get("recorded_at")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_owned(),
                                    content: rec
                                        .get("body")
                                        .or_else(|| rec.get("content"))
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_owned(),
                                });
                            }
                        }
                    }
                    Some(tool_name) if is_workspace_tool(tool_name) => {
                        has_workspace_result = true;
                        let bounded: String = output.chars().take(900).collect();
                        workspace_notes.push(format!("{tool_name}: {bounded}"));
                    }
                    _ => {}
                }
            }
            Item::Message { role, text } if role == "user" => {
                last_user = text.clone();
            }
            _ => {}
        }
    }

    if !excerpts.is_empty() || has_workspace_result {
        PromptPhase::ReadyToAnswer {
            excerpts,
            workspace_notes,
        }
    } else if has_search_result && !searched_candidates.is_empty() {
        PromptPhase::NeedRead {
            candidate_ids: searched_candidates.into_iter().take(3).collect(),
        }
    } else if has_search_result {
        PromptPhase::ReadyToAnswer {
            excerpts: Vec::new(),
            workspace_notes: Vec::new(),
        }
    } else {
        PromptPhase::NeedTool { query: last_user }
    }
}

fn format_chatml_prompt(
    request: &Request,
    phase: &PromptPhase,
    architecture: QwenArchitecture,
) -> String {
    if let PromptPhase::ReadyToAnswer {
        excerpts,
        workspace_notes,
    } = phase
    {
        let mut prompt = String::from(
            "<|im_start|>system\n\
             You are Coda, a clinical chart assistant. Answer the physician's question in 2-4 concise, natural plain-English sentences using ONLY the retrieved chart excerpts and workspace tool results.\n",
        );
        if excerpts.len() > 1 {
            let max_num = excerpts
                .iter()
                .map(|ex| ex.citation_number)
                .max()
                .unwrap_or(excerpts.len());
            prompt.push_str(&format!(
                "You have {} retrieved chart excerpts ([1] through [{}]). Synthesize across all relevant excerpts and cite EVERY applicable excerpt number ([1], [2], ...) directly beside the specific clinical claim from that excerpt — do not cite only [1] when claims come from multiple excerpts.\n",
                excerpts.len(),
                max_num
            ));
        } else if !excerpts.is_empty() {
            prompt.push_str(
                "Cite each clinical claim inline using [1], [2], etc. matching the excerpt numbers.\n",
            );
        } else {
            prompt.push_str(
                "Confirm the completed workspace action or summarize the retrieved workspace state clearly without inventing [N] chart citations.\n",
            );
        }
        prompt.push_str(
            "Write plain-English clinical prose ONLY. Never output JSON, braces, or tool calls.<|im_end|>\n",
        );
        let mut last_user = String::new();
        for item in &request.items {
            if let Item::Message { role, text } = item {
                let chat_role = match role.as_str() {
                    "assistant" => "assistant",
                    "developer" | "system" => "system",
                    _ => {
                        last_user = text.clone();
                        continue;
                    }
                };
                prompt.push_str(&format!("<|im_start|>{chat_role}\n{text}<|im_end|>\n"));
            }
        }
        if excerpts.is_empty() && workspace_notes.is_empty() {
            prompt.push_str(&format!("<|im_start|>user\n{last_user}<|im_end|>\n"));
        } else {
            prompt.push_str("<|im_start|>user\n");
            if !excerpts.is_empty() {
                prompt.push_str("Retrieved chart excerpts:\n");
                for ex in excerpts {
                    let snippet: String = ex.content.chars().take(700).collect();
                    let date_part = if ex.recorded_at.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", ex.recorded_at)
                    };
                    prompt.push_str(&format!(
                        "[{}] {}{}: {}\n",
                        ex.citation_number, ex.title, date_part, snippet
                    ));
                }
            }
            if !workspace_notes.is_empty() {
                if !excerpts.is_empty() {
                    prompt.push('\n');
                }
                prompt.push_str("Workspace tool results:\n");
                for note in workspace_notes {
                    prompt.push_str(&format!("- {note}\n"));
                }
            }
            prompt.push_str(&format!("\nPhysician request: {last_user}<|im_end|>\n"));
        }
        prompt.push_str(architecture.assistant_prefix());
        return prompt;
    }

    let mut prompt = String::from("<|im_start|>system\n");
    prompt.push_str(request.instructions.trim());
    if !request.tools.is_empty() {
        prompt.push_str("\n\n# Tools\nAvailable tools:\n");
        for tool in &request.tools {
            let name = tool
                .get("name")
                .or_else(|| tool.pointer("/function/name"))
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let desc = tool
                .get("description")
                .or_else(|| tool.pointer("/function/description"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let params = tool
                .get("parameters")
                .or_else(|| tool.pointer("/function/parameters"))
                .cloned()
                .unwrap_or(Value::Null);
            prompt.push_str(&format!(
                "<tool>{{\"name\":\"{name}\",\"description\":\"{desc}\",\"parameters\":{params}}}</tool>\n"
            ));
        }
        prompt.push_str(
            "To invoke a tool, emit <tool_call>{\"name\":\"...\",\"arguments\":{...}}</tool_call>.\n",
        );
    }
    prompt.push_str("<|im_end|>\n");

    for item in &request.items {
        match item {
            Item::Message { role, text } => {
                let chat_role = match role.as_str() {
                    "assistant" => "assistant",
                    "developer" | "system" => "system",
                    _ => "user",
                };
                prompt.push_str(&format!("<|im_start|>{chat_role}\n{text}<|im_end|>\n"));
            }
            Item::Feedback { text } => {
                prompt.push_str(&format!("<|im_start|>user\n[Guidance] {text}<|im_end|>\n"));
            }
            Item::FinishRefused(refusal) => {
                prompt.push_str(&format!(
                    "<|im_start|>user\n[Correction] {}<|im_end|>\n",
                    refusal.reason()
                ));
            }
            Item::Call(call) => {
                prompt.push_str(&format!(
                    "<|im_start|>assistant\n<tool_call>{{\"name\":\"{}\",\"arguments\":{}}}</tool_call><|im_end|>\n",
                    call.name, call.arguments
                ));
            }
            Item::MaterializedResult { output, .. } => {
                let bounded: String = output.chars().take(2800).collect();
                prompt.push_str(&format!(
                    "<|im_start|>user\n<tool_response>{bounded}</tool_response><|im_end|>\n"
                ));
            }
            Item::ProductMessage(_)
            | Item::OutputCutoff
            | Item::Result { .. }
            | Item::Opaque(_) => {}
        }
    }
    prompt.push_str(architecture.assistant_prefix());
    prompt
}

fn strip_think_blocks_raw(raw: &str, hold_partial_tag: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + "<think>".len()..];
        if let Some(end) = after_open.find("</think>") {
            rest = &after_open[end + "</think>".len()..];
        } else {
            rest = "";
            break;
        }
    }
    if hold_partial_tag {
        for tag in ["<think>", "</think>"] {
            for len in (1..tag.len()).rev() {
                if rest.ends_with(&tag[..len]) {
                    rest = &rest[..rest.len() - len];
                    break;
                }
            }
        }
    }
    out.push_str(rest);
    out.replace("</think>", "")
}

/// Strips any Qwen3 `<think>...</think>` blocks (including unclosed trailing `<think>` spans).
pub(crate) fn strip_think_blocks(raw: &str) -> String {
    strip_think_blocks_raw(raw, false).trim().to_owned()
}

fn generate_tokens_blocking(
    loaded: &LoadedQwen,
    prompt: &str,
    max_new_tokens: usize,
    stop_on_tool_call: bool,
    cancel: &Cancellation,
    chunk_tx: &mpsc::UnboundedSender<String>,
) -> Result<(String, u64, u64), Cause> {
    let encoding = loaded
        .tokenizer
        .encode(prompt, true)
        .map_err(|err| Box::new(CandleError::Tokenizer(err.to_string())) as Cause)?;
    let mut prompt_ids: Vec<u32> = encoding.get_ids().to_vec();
    if prompt_ids.is_empty() {
        prompt_ids.push(0);
    }
    let input_len = prompt_ids.len() as u64;

    let mut weights = loaded
        .weights
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cancel.is_cancelled() {
        return Err(Box::new(Failure::Cancelled));
    }
    weights.clear_kv_cache();

    let mut generated_ids: Vec<u32> = Vec::with_capacity(max_new_tokens);
    let mut prev_decoded_len = 0usize;

    let input_tensor =
        Tensor::new(prompt_ids.as_slice(), &Device::Cpu).map_err(|e| Box::new(e) as Cause)?;
    let input_tensor = input_tensor
        .unsqueeze(0)
        .map_err(|e| Box::new(e) as Cause)?;
    let mut next_logits = weights
        .forward(&input_tensor, 0)
        .map_err(|e| Box::new(e) as Cause)?;
    for (index_pos, step) in (prompt_ids.len()..).zip(0..max_new_tokens) {
        if cancel.is_cancelled() {
            return Err(Box::new(Failure::Cancelled));
        }
        let next_tok = argmax_last_token(&next_logits).map_err(|e| Box::new(e) as Cause)?;
        if loaded.eos_token_ids.contains(&next_tok) {
            break;
        }
        generated_ids.push(next_tok);
        if let Ok(decoded) = loaded.tokenizer.decode(&generated_ids, true) {
            let visible_raw = strip_think_blocks_raw(&decoded, true);
            let visible = visible_raw.trim_start();
            if visible.len() > prev_decoded_len
                && let Some(delta) = visible.get(prev_decoded_len..)
            {
                prev_decoded_len = visible.len();
                let _ = chunk_tx.send(delta.to_owned());
            }
            if stop_on_tool_call && visible.contains("</tool_call>") {
                break;
            }
        }
        if step + 1 >= max_new_tokens {
            break;
        }
        let tok_tensor =
            Tensor::new(&[next_tok], &Device::Cpu).map_err(|e| Box::new(e) as Cause)?;
        let tok_tensor = tok_tensor.unsqueeze(0).map_err(|e| Box::new(e) as Cause)?;
        next_logits = weights
            .forward(&tok_tensor, index_pos)
            .map_err(|e| Box::new(e) as Cause)?;
    }

    let final_text = loaded
        .tokenizer
        .decode(&generated_ids, true)
        .map_err(|err| Box::new(CandleError::Tokenizer(err.to_string())) as Cause)?;
    Ok((final_text, input_len, generated_ids.len() as u64))
}

fn argmax_last_token(logits: &Tensor) -> candle_core::Result<u32> {
    let squeezed = match logits.dims().len() {
        3 => logits.squeeze(0)?.get(logits.dim(1)? - 1)?,
        2 => logits.squeeze(0)?,
        _ => logits.clone(),
    };
    let vec = squeezed.to_vec1::<f32>()?;
    let mut best_idx = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (idx, &val) in vec.iter().enumerate() {
        if val > best_val {
            best_val = val;
            best_idx = idx;
        }
    }
    Ok(best_idx as u32)
}

/// Parses Qwen3 / Qwen2 `<tool_call>{...}</tool_call>` or fenced/bare JSON tool invocations.
pub(crate) fn parse_qwen_tool_calls(raw: &str, allowed_tools: &[String]) -> Vec<Invocation> {
    let cleaned = strip_think_blocks(raw);
    let raw = cleaned.as_str();
    let mut calls = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel_start) = raw[search_from..].find("<tool_call>") {
        let start = search_from + rel_start + "<tool_call>".len();
        let rest = &raw[start..];
        let body = if let Some(end) = rest.find("</tool_call>") {
            search_from = start + end + "</tool_call>".len();
            rest[..end].trim()
        } else {
            search_from = raw.len();
            rest.trim()
        };
        if let Some(call) = parse_single_call_json(body, allowed_tools, calls.len() + 1) {
            calls.push(call);
        }
    }
    if calls.is_empty()
        && let Some(start) = raw.find('{')
        && let Some(end) = raw.rfind('}')
        && end > start
        && let Some(call) =
            parse_single_call_json(&raw[start..=end], allowed_tools, calls.len() + 1)
    {
        calls.push(call);
    }
    calls
}

fn parse_single_call_json(
    raw: &str,
    allowed_tools: &[String],
    ordinal: usize,
) -> Option<Invocation> {
    let val: Value = serde_json::from_str(raw).ok()?;
    let name = val.get("name").and_then(Value::as_str)?.trim();
    if !allowed_tools.iter().any(|allowed| allowed == name) {
        return None;
    }
    let args = val
        .get("arguments")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    Some(Invocation {
        id: format!("candle-call-{ordinal}-{}", uuid::Uuid::new_v4().simple()),
        name: name.to_owned(),
        arguments: args.to_string(),
    })
}

fn fallback_initial_tool_call(query: &str, allowed_tools: &[String]) -> Invocation {
    let lower = query.to_ascii_lowercase();
    let allows = |name: &str| allowed_tools.iter().any(|t| t == name);
    let trimmed = query.trim();
    if allows(TOOL_MEMORY_SAVE)
        && (lower.contains("save to memory")
            || lower.contains("save in memory")
            || lower.contains("memory note")
            || lower.starts_with("remember "))
    {
        return Invocation {
            id: format!("candle-memory-save-{}", uuid::Uuid::new_v4().simple()),
            name: TOOL_MEMORY_SAVE.to_owned(),
            arguments: json!({ "note": trimmed }).to_string(),
        };
    }
    if allows(TOOL_FOLLOW_UP_CREATE)
        && (lower.contains("create a follow-up")
            || lower.contains("create follow-up")
            || lower.contains("schedule a follow-up")
            || lower.contains("schedule follow-up")
            || lower.contains("add a follow-up"))
    {
        let title: String = trimmed.chars().take(80).collect();
        return Invocation {
            id: format!("candle-followup-create-{}", uuid::Uuid::new_v4().simple()),
            name: TOOL_FOLLOW_UP_CREATE.to_owned(),
            arguments: json!({
                "title": title,
                "instructions": trimmed,
            })
            .to_string(),
        };
    }
    if allows(TOOL_MEMORY_READ)
        && (lower.contains("patient memory")
            || lower.contains("read memory")
            || lower.contains("in memory"))
    {
        return Invocation {
            id: format!("candle-memory-read-{}", uuid::Uuid::new_v4().simple()),
            name: TOOL_MEMORY_READ.to_owned(),
            arguments: json!({}).to_string(),
        };
    }
    if allows(TOOL_FOLLOW_UP_LIST)
        && (lower.contains("list follow-up")
            || lower.contains("open follow-up")
            || lower.contains("current follow-up")
            || lower.contains("scheduled follow-up"))
    {
        return Invocation {
            id: format!("candle-followup-list-{}", uuid::Uuid::new_v4().simple()),
            name: TOOL_FOLLOW_UP_LIST.to_owned(),
            arguments: json!({}).to_string(),
        };
    }
    if allows(TOOL_BRIEFING_READ) && (lower.contains("briefing") || lower.contains("pre-visit")) {
        return Invocation {
            id: format!("candle-briefing-read-{}", uuid::Uuid::new_v4().simple()),
            name: TOOL_BRIEFING_READ.to_owned(),
            arguments: json!({}).to_string(),
        };
    }
    Invocation {
        id: format!("candle-search-{}", uuid::Uuid::new_v4().simple()),
        name: TOOL_CHART_SEARCH.to_owned(),
        arguments: json!({ "query": query }).to_string(),
    }
}

fn normalize_answer_citations(
    raw: &str,
    excerpts: &[ChartExcerpt],
    workspace_notes: &[String],
) -> String {
    let stripped = strip_think_blocks(raw);
    let trimmed = stripped
        .replace("<tool_call>", "")
        .replace("</tool_call>", "")
        .trim()
        .to_owned();
    let looks_like_tool_json = trimmed.starts_with('{')
        || trimmed.contains("\"candidate_ids\"")
        || trimmed.contains("\"chart_read\"")
        || trimmed.contains("\"chart_search\"")
        || trimmed.contains("\"memory_save\"")
        || trimmed.contains("\"follow_up_create\"")
        || trimmed.contains("\"mental_health_log\"")
        || trimmed.contains("\"lingo_cgm_ingest\"")
        || trimmed.contains("\"cgm_ingest\"");
    if looks_like_tool_json {
        if !excerpts.is_empty() {
            return excerpts
                .iter()
                .take(3)
                .map(|ex| {
                    let first = ex
                        .content
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or(ex.content.as_str())
                        .trim();
                    let short: String = first.chars().take(140).collect();
                    format!("{} reports: {short} [{}].", ex.title, ex.citation_number)
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
        if !workspace_notes.is_empty() {
            return "Updated the patient workspace as requested.".to_owned();
        }
    }

    let read_count = excerpts.len();
    let mut normalized = trimmed;
    for i in 1..=read_count.max(4) {
        normalized = normalized.replace(&format!("[S{i}]"), &format!("[{i}]"));
    }
    if read_count > 0 && !(1..=read_count).any(|i| normalized.contains(&format!("[{i}]"))) {
        if normalized.is_empty() {
            normalized = "Reviewed the patient's chart records [1].".to_owned();
        } else {
            normalized.push_str(" [1]");
        }
    }
    normalized
}

impl Provider for CandleQwenProvider {
    async fn exchange(
        &mut self,
        request: &Request,
        cancel: &Cancellation,
        _limits: &RunLimits,
        observer: &mut dyn Observer,
    ) -> Result<Turn, Cause> {
        cancel.check()?;
        let permit = tokio::select! {
            acquired = self.runtime.semaphore.clone().acquire_owned() => {
                acquired.map_err(|err| Box::new(err) as Cause)?
            }
            () = wait_cancelled(cancel) => return Err(Box::new(Failure::Cancelled)),
        };

        let loaded = tokio::select! {
            loaded = self.runtime.get_or_load() => {
                Arc::clone(loaded.map_err(|err| Box::new(err) as Cause)?)
            }
            () = wait_cancelled(cancel) => return Err(Box::new(Failure::Cancelled)),
        };

        let phase = inspect_prompt_phase(request);
        let architecture = loaded.architecture;
        let model_name = loaded.model_name.clone();
        let prompt = format_chatml_prompt(request, &phase, architecture);
        let max_new = match phase {
            PromptPhase::NeedTool { .. } | PromptPhase::NeedRead { .. } => {
                self.runtime.config.max_new_tokens.min(64)
            }
            PromptPhase::ReadyToAnswer { .. } => self.runtime.config.max_new_tokens.min(128),
        };
        let allowed_tools: Vec<String> = request
            .tools
            .iter()
            .filter_map(|t| {
                t.get("name")
                    .or_else(|| t.pointer("/function/name"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();

        let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel::<String>();
        let cancel_bg = cancel.clone();
        let stream_live_text = matches!(phase, PromptPhase::ReadyToAnswer { .. });
        let stop_on_tool_call = !stream_live_text;

        let handle = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            generate_tokens_blocking(
                &loaded,
                &prompt,
                max_new,
                stop_on_tool_call,
                &cancel_bg,
                &chunk_tx,
            )
        });

        while let Some(delta) = tokio::select! {
            msg = chunk_rx.recv() => msg,
            () = wait_cancelled(cancel) => return Err(Box::new(Failure::Cancelled)),
        } {
            if stream_live_text {
                observer.emit(StreamEvent::Text(delta.into())).await?;
            }
        }

        let (raw_text, input_tokens, output_tokens) = tokio::select! {
            res = handle => res.map_err(|e| Box::new(CandleError::from(e)) as Cause)??,
            () = wait_cancelled(cancel) => return Err(Box::new(Failure::Cancelled)),
        };

        let items = match phase {
            PromptPhase::NeedTool { query } => {
                let mut calls = parse_qwen_tool_calls(&raw_text, &allowed_tools);
                if calls.is_empty() {
                    calls.push(fallback_initial_tool_call(&query, &allowed_tools));
                }
                for call in &calls {
                    observer
                        .emit(StreamEvent::CallBegan {
                            name: call.name.clone(),
                        })
                        .await?;
                }
                calls.into_iter().map(Item::Call).collect()
            }
            PromptPhase::NeedRead { candidate_ids } => {
                let mut calls = parse_qwen_tool_calls(&raw_text, &allowed_tools)
                    .into_iter()
                    .filter(|c| c.name == TOOL_CHART_READ || is_workspace_tool(&c.name))
                    .collect::<Vec<_>>();
                if calls.is_empty() {
                    calls.push(Invocation {
                        id: format!("candle-read-{}", uuid::Uuid::new_v4().simple()),
                        name: TOOL_CHART_READ.to_owned(),
                        arguments: json!({ "candidate_ids": candidate_ids }).to_string(),
                    });
                }
                for call in &calls {
                    observer
                        .emit(StreamEvent::CallBegan {
                            name: call.name.clone(),
                        })
                        .await?;
                }
                calls.into_iter().map(Item::Call).collect()
            }
            PromptPhase::ReadyToAnswer {
                excerpts,
                workspace_notes,
            } => {
                vec![Item::assistant(normalize_answer_citations(
                    &raw_text,
                    &excerpts,
                    &workspace_notes,
                ))]
            }
        };

        Ok(Turn {
            items,
            receipt: json!({
                "provider": architecture.provider_label(),
                "model": model_name,
            }),
            usage: Usage {
                input_tokens: Some(input_tokens),
                cached_input_tokens: None,
                output_tokens: Some(output_tokens),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen_tool_call_parser_extracts_tagged_and_bare_calls() {
        let allowed = vec!["chart_search".to_owned(), "chart_read".to_owned()];
        let tagged = r#"Let me search first.
<tool_call>
{"name": "chart_search", "arguments": {"query": "potassium"}}
</tool_call>"#;
        let parsed = parse_qwen_tool_calls(tagged, &allowed);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "chart_search");
        assert!(parsed[0].arguments.contains("potassium"));

        let unknown = r#"<tool_call>{"name": "shell_exec", "arguments": {}}</tool_call>"#;
        assert!(parse_qwen_tool_calls(unknown, &allowed).is_empty());
    }

    #[test]
    fn citation_normalization_maps_s_tags_and_appends_when_missing() {
        let excerpts = vec![ChartExcerpt {
            citation_number: 1,
            title: "BMP".to_owned(),
            recorded_at: "2026-07-15".to_owned(),
            content: "Serum potassium 5.2 mmol/L.".to_owned(),
        }];
        assert_eq!(
            normalize_answer_citations("Serum potassium was 5.2 mmol/L [S1].", &excerpts, &[]),
            "Serum potassium was 5.2 mmol/L [1]."
        );
        assert_eq!(
            normalize_answer_citations("Serum potassium was elevated.", &excerpts, &[]),
            "Serum potassium was elevated. [1]"
        );
        let prose = normalize_answer_citations(
            r#"{"name":"chart_read","arguments":{"candidate_ids":["a"]}}"#,
            &excerpts,
            &[],
        );
        assert!(prose.contains("BMP") && prose.contains("[1]") && !prose.contains("{\"name\""));
        assert_eq!(
            normalize_answer_citations(
                "Saved your note to Patient Memory.",
                &[],
                &["memory_save: {\"saved\":true}".to_owned()]
            ),
            "Saved your note to Patient Memory.",
            "workspace-only answer must not append [1]"
        );
    }

    #[tokio::test]
    async fn cached_artifact_checksum_matches_without_redownload() {
        let dir =
            std::env::temp_dir().join(format!("coda-sha-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sample.bin");
        let payload = b"coda-candle-checksum-test";
        std::fs::write(&file, payload).unwrap();
        let mut h = Sha256::new();
        h.update(payload);
        let expected = encode_hex(&h.finalize());
        let actual = file_sha256_blocking(&file).await.unwrap();
        assert_eq!(actual, expected);
        let resolved = resolve_artifact(
            &reqwest::Client::new(),
            None,
            &file,
            "http://127.0.0.1:9/bad",
            &expected,
        )
        .await
        .unwrap();
        assert_eq!(resolved, file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspect_prompt_phase_captures_chart_read_body_field() {
        let req = Request {
            cache_key: None,
            model: "qwen".to_owned(),
            context_window_tokens: 8192,
            max_input_tokens: 4096,
            options: Value::Null,
            instructions: "Be concise".to_owned(),
            tools: vec![json!({ "name": TOOL_CHART_SEARCH })],
            items: vec![
                Item::Message {
                    role: "user".to_owned(),
                    text: "Review medications".to_owned(),
                },
                Item::Call(Invocation {
                    id: "c1".to_owned(),
                    name: TOOL_CHART_READ.to_owned(),
                    arguments: r#"{"candidate_ids":["rec-1"]}"#.to_owned(),
                }),
                Item::MaterializedResult {
                    call_id: "c1".to_owned(),
                    output: json!({
                        "records": [{
                            "citation_number": 1,
                            "candidate_id": "rec-1",
                            "title": "DM follow-up",
                            "recorded_at": "2026-07-15",
                            "body": "Atorvastatin 20 mg daily; LDL 130 mg/dL."
                        }]
                    })
                    .to_string(),
                },
            ],
        };
        let phase = inspect_prompt_phase(&req);
        let PromptPhase::ReadyToAnswer {
            excerpts,
            workspace_notes,
        } = phase
        else {
            panic!("expected ReadyToAnswer");
        };
        assert_eq!(excerpts.len(), 1);
        assert!(workspace_notes.is_empty());
        assert_eq!(
            excerpts[0].content,
            "Atorvastatin 20 mg daily; LDL 130 mg/dL."
        );
        let prompt_q3 = format_chatml_prompt(
            &req,
            &PromptPhase::ReadyToAnswer {
                excerpts,
                workspace_notes,
            },
            QwenArchitecture::Qwen3,
        );
        assert!(prompt_q3.contains("Atorvastatin 20 mg daily; LDL 130 mg/dL."));
        assert!(
            prompt_q3.ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\n"),
            "Qwen3 prompt must append official non-thinking prefix"
        );
        let prompt_q2 = format_chatml_prompt(
            &req,
            &PromptPhase::NeedTool {
                query: "medications".to_owned(),
            },
            QwenArchitecture::Qwen2,
        );
        assert!(prompt_q2.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn multi_excerpt_prompt_and_workspace_tool_phases_are_grounded() {
        let req = Request {
            cache_key: None,
            model: "qwen".to_owned(),
            context_window_tokens: 8192,
            max_input_tokens: 4096,
            options: Value::Null,
            instructions: "Be concise".to_owned(),
            tools: vec![
                json!({ "name": TOOL_CHART_SEARCH }),
                json!({ "name": TOOL_MEMORY_SAVE }),
                json!({ "name": TOOL_FOLLOW_UP_CREATE }),
            ],
            items: vec![
                Item::Message {
                    role: "user".to_owned(),
                    text: "Summarize vitals, note, and medications".to_owned(),
                },
                Item::Call(Invocation {
                    id: "c-read".to_owned(),
                    name: TOOL_CHART_READ.to_owned(),
                    arguments: r#"{"candidate_ids":["v1","n1","m1"]}"#.to_owned(),
                }),
                Item::MaterializedResult {
                    call_id: "c-read".to_owned(),
                    output: json!({
                        "records": [
                            { "citation_number": 1, "title": "Vitals", "recorded_at": "2026-07-15", "body": "BP 127/82." },
                            { "citation_number": 2, "title": "DM follow-up", "recorded_at": "2026-07-15", "body": "Feet numb." },
                            { "citation_number": 3, "title": "Medications", "recorded_at": "2026-07-15", "body": "Atorvastatin 20 mg." }
                        ]
                    })
                    .to_string(),
                },
            ],
        };
        let phase = inspect_prompt_phase(&req);
        let prompt = format_chatml_prompt(&req, &phase, QwenArchitecture::Qwen3);
        assert!(
            prompt.contains("3 retrieved chart excerpts ([1] through [3])"),
            "multi-excerpt prompt must explicitly instruct citing [1]..[3]:\n{prompt}"
        );
        assert!(
            prompt.contains("[1] Vitals")
                && prompt.contains("[2] DM follow-up")
                && prompt.contains("[3] Medications")
        );

        let mem_req = Request {
            items: vec![
                Item::Message {
                    role: "user".to_owned(),
                    text: "Remember patient prefers morning labs".to_owned(),
                },
                Item::Call(Invocation {
                    id: "c-mem".to_owned(),
                    name: TOOL_MEMORY_SAVE.to_owned(),
                    arguments: r#"{"note":"Prefers morning labs"}"#.to_owned(),
                }),
                Item::MaterializedResult {
                    call_id: "c-mem".to_owned(),
                    output: json!({ "saved": true, "note_number": "N1" }).to_string(),
                },
            ],
            ..req
        };
        let PromptPhase::ReadyToAnswer {
            excerpts,
            workspace_notes,
        } = inspect_prompt_phase(&mem_req)
        else {
            panic!("memory_save result must transition to ReadyToAnswer");
        };
        assert!(excerpts.is_empty());
        assert_eq!(workspace_notes.len(), 1);
        assert!(workspace_notes[0].contains("memory_save"));

        let allowed = vec![
            TOOL_CHART_SEARCH.to_owned(),
            TOOL_MEMORY_SAVE.to_owned(),
            TOOL_FOLLOW_UP_CREATE.to_owned(),
        ];
        assert_eq!(
            fallback_initial_tool_call("Remember patient prefers morning labs", &allowed).name,
            TOOL_MEMORY_SAVE
        );
        assert_eq!(
            fallback_initial_tool_call("Schedule a follow-up to recheck potassium", &allowed).name,
            TOOL_FOLLOW_UP_CREATE
        );
        assert_eq!(
            fallback_initial_tool_call("What is the latest A1c?", &allowed).name,
            TOOL_CHART_SEARCH
        );
    }

    #[test]
    fn qwen3_think_blocks_are_stripped_from_tool_calls_and_prose() {
        assert_eq!(
            strip_think_blocks("<think>let me check potassium</think>\nPotassium is 5.2 [1]."),
            "Potassium is 5.2 [1]."
        );
        assert_eq!(
            strip_think_blocks("Lead text.<think>unclosed reasoning"),
            "Lead text."
        );
        assert_eq!(
            strip_think_blocks_raw("Answer <thi", true),
            "Answer ",
            "streaming holds partial <think> prefix"
        );
        let allowed = vec!["chart_search".to_owned()];
        let raw = r#"<think>need to search</think><tool_call>{"name":"chart_search","arguments":{"query":"A1c"}}</tool_call>"#;
        let calls = parse_qwen_tool_calls(raw, &allowed);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "chart_search");
        assert!(calls[0].arguments.contains("A1c"));
    }

    #[test]
    fn default_config_targets_qwen3_1_7b_q4_k_m() {
        let cfg = CandleQwenConfig::default();
        assert_eq!(
            cfg.gguf_url,
            "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf"
        );
        assert_eq!(
            cfg.gguf_sha256,
            "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897"
        );
        assert_eq!(
            cfg.tokenizer_url,
            "https://huggingface.co/Qwen/Qwen3-1.7B/resolve/main/tokenizer.json"
        );
        assert_eq!(
            cfg.tokenizer_sha256,
            "aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4"
        );
        assert_eq!(DEFAULT_CACHED_GGUF_FILENAME, "Qwen3-1.7B-Q4_K_M.gguf");
        let rt = CandleQwenRuntime::new(cfg);
        assert_eq!(rt.model_label(), "qwen3-1.7b-q4_k_m");
    }
}
