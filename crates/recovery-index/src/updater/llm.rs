//! Optional, content-addressed cached LLM extractor for facility websites.
//!
//! Supports multiple local/cloud LLM backends out of the box:
//! - `codex`  : OpenAI Codex CLI (`codex exec --sandbox read-only --ephemeral --output-schema ...`)
//! - `claude` : Claude Code CLI (`claude -p --output-format json --json-schema ...`)
//! - `openai` : Any OpenAI-compatible HTTP `/chat/completions` endpoint (`OPENAI_BASE_URL` / `OPENAI_API_KEY`)
//! - `ollama` : Local Ollama OpenAI-compatible endpoint (`http://127.0.0.1:11434/v1`)
//!
//! Every extraction is cached on disk under `.cache/recovery-index/llm/` keyed by
//! `sha256(PROMPT_VERSION | backend | model | content_sha256)` so unchanged websites never pay
//! for a repeat LLM call, and `--offline` still applies previously cached extractions.

use std::fs;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::schema::{
    Amenity, DataSource, DietaryOption, ElectronicsPolicy, EnrichmentMeta, Facility, FoodSituation,
    SmokingPolicy, StaffLicense, VapingPolicy,
};
use crate::updater::cache::{sha256_hex, today_utc_iso, CacheMode, DiskCache};
use crate::updater::website::CachedWebPage;

const PROMPT_VERSION: &str = "v1";
const LLM_CACHE_TTL: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const MAX_PROMPT_CHARS: usize = 14_000;
const CLI_TIMEOUT: Duration = Duration::from_secs(90);

/// Which LLM backend to use for structured website extraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmBackend {
    Codex,
    Claude,
    OpenAi,
    Ollama,
}

impl LlmBackend {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "codex" => Some(Self::Codex),
            "claude" => Some(Self::Claude),
            "openai" | "openai-compat" => Some(Self::OpenAi),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::OpenAi => "openai",
            Self::Ollama => "ollama",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub backend: LlmBackend,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub concurrency: usize,
}

impl LlmConfig {
    pub fn new(backend: LlmBackend) -> Self {
        Self {
            backend,
            model: None,
            base_url: None,
            api_key: None,
            concurrency: 3,
        }
    }

    fn effective_model_label(&self) -> String {
        if let Some(m) = &self.model {
            return m.clone();
        }
        match self.backend {
            LlmBackend::Codex => "codex-default".to_string(),
            LlmBackend::Claude => "claude-default".to_string(),
            LlmBackend::OpenAi => {
                std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4.1-mini".to_string())
            }
            LlmBackend::Ollama => {
                std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5:14b".to_string())
            }
        }
    }
}

/// Strict typed extraction produced by an LLM from a facility's cached website text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmExtraction {
    #[serde(default)]
    pub amenities: Vec<Amenity>,
    #[serde(default)]
    pub food: Option<FoodSituation>,
    #[serde(default)]
    pub dietary: Vec<DietaryOption>,
    #[serde(default)]
    pub electronics: Option<ElectronicsPolicy>,
    #[serde(default)]
    pub smoking: Option<SmokingPolicy>,
    #[serde(default)]
    pub vaping: Option<VapingPolicy>,
    #[serde(default)]
    pub staff_licenses: Vec<StaffLicense>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub evidence_urls: Vec<String>,
}

impl LlmExtraction {
    pub fn is_empty(&self) -> bool {
        self.amenities.is_empty()
            && self.food.is_none()
            && self.dietary.is_empty()
            && self.electronics.is_none()
            && self.smoking.is_none()
            && self.vaping.is_none()
            && self.staff_licenses.is_empty()
            && self
                .notes
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
    }
}

/// Raw JSON shape parsed from the model before filtering through exact domain enums, so an
/// unexpected string from a smaller/local model never crashes the whole batch.
#[derive(Debug, Deserialize)]
struct RawLlmPayload {
    #[serde(default)]
    amenities: Vec<String>,
    #[serde(default)]
    food: Option<String>,
    #[serde(default)]
    dietary: Vec<String>,
    #[serde(default)]
    electronics: Option<String>,
    #[serde(default)]
    smoking: Option<String>,
    #[serde(default)]
    vaping: Option<String>,
    #[serde(default)]
    staff_licenses: Vec<String>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("failed to write temporary schema/output file for {backend}")]
    TempIo {
        backend: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("could not spawn `{program}` CLI (is it installed on PATH?)")]
    Spawn {
        program: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("`{program}` timed out after {secs}s")]
    Timeout { program: &'static str, secs: u64 },
    #[error("`{program}` exited with status {code}: {stderr}")]
    CliExit {
        program: &'static str,
        code: i32,
        stderr: String,
    },
    #[error("HTTP request to LLM endpoint failed")]
    Http(#[source] reqwest::Error),
    #[error("missing API key for OpenAI-compatible backend (set OPENAI_API_KEY)")]
    MissingApiKey,
    #[error("LLM output could not be parsed as structured JSON: {preview}")]
    Parse {
        preview: String,
        #[source]
        source: serde_json::Error,
    },
}

pub enum LlmOutcome {
    CacheHit(LlmExtraction),
    Extracted(LlmExtraction),
    Skipped,
}

/// Extracts structured amenities, food, electronic device policy, smoking/vaping, and exact staff
/// license types from a facility's already-cached website pages, backed by a content-addressed
/// disk cache under `.cache/recovery-index/llm/`.
pub async fn extract_from_pages_cached(
    client: &reqwest::Client,
    cache: &DiskCache,
    cfg: &LlmConfig,
    pages: &[CachedWebPage],
) -> Result<LlmOutcome, LlmError> {
    let Some((excerpt, urls)) = build_site_excerpt(pages) else {
        return Ok(LlmOutcome::Skipped);
    };

    let content_hash = sha256_hex(excerpt.as_bytes());
    let model_label = cfg.effective_model_label();
    let cache_key = format!(
        "{PROMPT_VERSION}|{}|{model_label}|{content_hash}",
        cfg.backend.as_str()
    );

    if let Some(hit) = cache.get::<LlmExtraction>("llm", &cache_key, LLM_CACHE_TTL) {
        return Ok(LlmOutcome::CacheHit(hit.value));
    }
    if cache.mode() == CacheMode::Offline {
        return Ok(LlmOutcome::Skipped);
    }

    let prompt = build_extraction_prompt(&excerpt);
    let raw_json = match cfg.backend {
        LlmBackend::Codex => invoke_codex(cfg, &prompt).await?,
        LlmBackend::Claude => invoke_claude(cfg, &prompt).await?,
        LlmBackend::OpenAi | LlmBackend::Ollama => {
            invoke_openai_compat(client, cfg, &prompt).await?
        }
    };

    let mut parsed = parse_llm_json(&raw_json)?;
    parsed.evidence_urls = urls;
    let _ = cache.put("llm", &cache_key, &parsed, None, None);
    Ok(LlmOutcome::Extracted(parsed))
}

/// Merges an [`LlmExtraction`] onto `facility`, returning `true` if any data was applied.
pub fn apply_llm_extraction(facility: &mut Facility, ext: &LlmExtraction) -> bool {
    if ext.is_empty() {
        return false;
    }

    for &a in &ext.amenities {
        if !facility.lifestyle.amenities.contains(&a) {
            facility.lifestyle.amenities.push(a);
        }
    }
    if let Some(food) = ext.food {
        if facility.lifestyle.food.is_none() {
            facility.lifestyle.food = Some(food);
        }
    }
    for &d in &ext.dietary {
        if !facility.lifestyle.dietary.contains(&d) {
            facility.lifestyle.dietary.push(d);
        }
    }
    if let Some(elec) = ext.electronics {
        if facility.lifestyle.electronics.is_none() {
            facility.lifestyle.electronics = Some(elec);
        }
    }
    if let Some(smoking) = ext.smoking {
        if facility.lifestyle.smoking.is_none() {
            facility.lifestyle.smoking = Some(smoking);
        }
    }
    if let Some(vaping) = ext.vaping {
        if facility.lifestyle.vaping.is_none() {
            facility.lifestyle.vaping = Some(vaping);
        }
    }
    if let Some(notes) = &ext.notes {
        let trimmed = notes.trim();
        if !trimmed.is_empty() && facility.lifestyle.notes.is_none() {
            facility.lifestyle.notes = Some(trimmed.to_string());
        }
    }
    facility.lifestyle.normalize();

    for &lic in &ext.staff_licenses {
        if !facility.staff_licenses.contains(&lic) {
            facility.staff_licenses.push(lic);
        }
    }
    facility.staff_licenses.sort_unstable();
    facility.staff_licenses.dedup();

    let meta = facility
        .enrichment
        .get_or_insert_with(EnrichmentMeta::default);
    meta.add_source(DataSource::Llm);
    meta.website_checked = Some(today_utc_iso());
    for url in &ext.evidence_urls {
        meta.add_evidence_url(url.clone());
    }
    true
}

/// Combines cached pages into a compact bounded excerpt, prioritizing subpages (amenities, FAQ,
/// what-to-bring, dining, staff) before the homepage.
pub fn build_site_excerpt(pages: &[CachedWebPage]) -> Option<(String, Vec<String>)> {
    let valid: Vec<&CachedWebPage> = pages
        .iter()
        .filter(|p| p.status >= 200 && p.status < 400 && !p.text.trim().is_empty())
        .collect();
    if valid.is_empty() {
        return None;
    }

    // Put subpages first (index 1..), then homepage (index 0) so amenities/FAQ/staff pages never
    // get truncated behind long homepage marketing copy.
    let mut ordered: Vec<&CachedWebPage> = Vec::with_capacity(valid.len());
    ordered.extend(valid.iter().skip(1).copied());
    ordered.push(valid[0]);

    let mut out = String::new();
    let mut urls = Vec::new();
    for page in ordered {
        let header = format!("\n--- PAGE: {} ---\n", page.url);
        if out.len() + header.len() >= MAX_PROMPT_CHARS {
            break;
        }
        out.push_str(&header);
        urls.push(page.url.clone());
        let budget = MAX_PROMPT_CHARS.saturating_sub(out.len());
        let chunk = truncate_utf8(&page.text, budget);
        out.push_str(chunk);
        if out.len() >= MAX_PROMPT_CHARS {
            break;
        }
    }

    if out.trim().is_empty() {
        None
    } else {
        Some((out, urls))
    }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// JSON Schema enforced by `codex exec --output-schema` and `claude --json-schema`.
pub fn extraction_json_schema() -> serde_json::Value {
    let amenity_codes: Vec<&str> = Amenity::ALL.iter().map(|v| v.code()).collect();
    let food_codes: Vec<&str> = FoodSituation::ALL.iter().map(|v| v.code()).collect();
    let dietary_codes: Vec<&str> = DietaryOption::ALL.iter().map(|v| v.code()).collect();
    let electronics_codes: Vec<&str> = ElectronicsPolicy::ALL.iter().map(|v| v.code()).collect();
    let smoking_codes: Vec<&str> = SmokingPolicy::ALL.iter().map(|v| v.code()).collect();
    let vaping_codes: Vec<&str> = VapingPolicy::ALL.iter().map(|v| v.code()).collect();
    let staff_codes: Vec<&str> = StaffLicense::ALL.iter().map(|v| v.code()).collect();

    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "amenities",
            "food",
            "dietary",
            "electronics",
            "smoking",
            "vaping",
            "staff_licenses",
            "notes"
        ],
        "properties": {
            "amenities": {
                "type": "array",
                "items": { "type": "string", "enum": amenity_codes }
            },
            "food": {
                "type": ["string", "null"],
                "enum": food_codes.into_iter().map(Some).chain(std::iter::once(None)).collect::<Vec<_>>()
            },
            "dietary": {
                "type": "array",
                "items": { "type": "string", "enum": dietary_codes }
            },
            "electronics": {
                "type": ["string", "null"],
                "enum": electronics_codes.into_iter().map(Some).chain(std::iter::once(None)).collect::<Vec<_>>()
            },
            "smoking": {
                "type": ["string", "null"],
                "enum": smoking_codes.into_iter().map(Some).chain(std::iter::once(None)).collect::<Vec<_>>()
            },
            "vaping": {
                "type": ["string", "null"],
                "enum": vaping_codes.into_iter().map(Some).chain(std::iter::once(None)).collect::<Vec<_>>()
            },
            "staff_licenses": {
                "type": "array",
                "items": { "type": "string", "enum": staff_codes }
            },
            "notes": {
                "type": ["string", "null"],
                "description": "Concise 1-2 sentence factual note on amenities, food, or phone/laptop rules explicitly stated on the website (or null if none)."
            }
        }
    })
}

pub fn build_extraction_prompt(site_excerpt: &str) -> String {
    let amenity_list = Amenity::ALL
        .iter()
        .map(|v| format!("`{}` ({})", v.code(), v.label()))
        .collect::<Vec<_>>()
        .join(", ");
    let food_list = FoodSituation::ALL
        .iter()
        .map(|v| format!("`{}` ({})", v.code(), v.label()))
        .collect::<Vec<_>>()
        .join(", ");
    let dietary_list = DietaryOption::ALL
        .iter()
        .map(|v| format!("`{}` ({})", v.code(), v.label()))
        .collect::<Vec<_>>()
        .join(", ");
    let electronics_list = ElectronicsPolicy::ALL
        .iter()
        .map(|v| format!("`{}` ({})", v.code(), v.label()))
        .collect::<Vec<_>>()
        .join(", ");
    let staff_list = StaffLicense::ALL
        .iter()
        .map(|v| format!("`{}` ({})", v.code(), v.label()))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "You are extracting factual, non-care facility amenities and exact clinical staff license \
         types from public text scraped from a treatment facility's website.\n\n\
         STRICT RULES:\n\
         1. Extract ONLY facts explicitly stated in the provided text. NEVER guess, infer, or assume.\n\
         2. If a field is not explicitly mentioned, return `null` or `[]`.\n\
         3. Pay strict attention to negations (e.g. 'no pets', 'cell phones are not allowed').\n\
         4. NEVER conflate clinical staff credentials:\n\
            - A DO is NOT an MD (`do-psychiatrist` != `md-psychiatrist`). Only emit MD or DO when that exact degree appears.\n\
            - A Nurse Practitioner (`pmhnp` / `np`) or Physician Assistant (`pa`) is NOT a Psychiatrist.\n\
            - Distinguish `phd-psychologist` vs `psyd-psychologist`, and `lcsw` vs `lpc-lmhc` vs `lmft`.\n\
         5. Return ONLY a single JSON object matching the required schema (no markdown fences, no commentary).\n\n\
         ALLOWED ENUM CODES:\n\
         - amenities: {amenity_list}\n\
         - food: {food_list} (or null)\n\
         - dietary: {dietary_list}\n\
         - electronics: {electronics_list} (or null)\n\
         - smoking: `not-permitted`, `designated-area`, `permitted` (or null)\n\
         - vaping: `not-permitted`, `designated-area`, `permitted` (or null)\n\
         - staff_licenses: {staff_list}\n\
         - notes: 1-2 short factual sentences summarizing explicit amenities/food/device rules (max 240 chars), or null.\n\n\
         WEBSITE EXCERPTS:\n{site_excerpt}\n"
    )
}

/// Parses structured JSON (including tolerating markdown code fences or Claude CLI wrapper objects)
/// into a validated [`LlmExtraction`].
pub fn parse_llm_json(raw: &str) -> Result<LlmExtraction, LlmError> {
    let trimmed = strip_json_fences(raw.trim());

    // Claude `--output-format json` wraps structured output in `{ "structured_output": { ... } }`
    // or `{ "result": "..." }`. Unwrap cleanly when present.
    let val: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|source| LlmError::Parse {
            preview: truncate_utf8(trimmed, 240).to_string(),
            source,
        })?;

    let payload_val = if let Some(inner) = val.get("structured_output") {
        inner.clone()
    } else if let Some(result_str) = val.get("result").and_then(|v| v.as_str()) {
        let inner_trimmed = strip_json_fences(result_str.trim());
        serde_json::from_str(inner_trimmed).map_err(|source| LlmError::Parse {
            preview: truncate_utf8(inner_trimmed, 240).to_string(),
            source,
        })?
    } else {
        val
    };

    let raw_payload: RawLlmPayload =
        serde_json::from_value(payload_val).map_err(|source| LlmError::Parse {
            preview: truncate_utf8(trimmed, 240).to_string(),
            source,
        })?;

    let mut amenities: Vec<Amenity> = raw_payload
        .amenities
        .iter()
        .filter_map(|c| Amenity::from_code(c))
        .collect();
    amenities.sort_unstable();
    amenities.dedup();

    let mut dietary: Vec<DietaryOption> = raw_payload
        .dietary
        .iter()
        .filter_map(|c| DietaryOption::from_code(c))
        .collect();
    dietary.sort_unstable();
    dietary.dedup();

    let mut staff_licenses: Vec<StaffLicense> = raw_payload
        .staff_licenses
        .iter()
        .filter_map(|c| StaffLicense::from_code(c))
        .collect();
    staff_licenses.sort_unstable();
    staff_licenses.dedup();

    let notes = raw_payload
        .notes
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| truncate_utf8(&s, 280).to_string());

    Ok(LlmExtraction {
        amenities,
        food: raw_payload
            .food
            .as_deref()
            .and_then(FoodSituation::from_code),
        dietary,
        electronics: raw_payload
            .electronics
            .as_deref()
            .and_then(ElectronicsPolicy::from_code),
        smoking: raw_payload
            .smoking
            .as_deref()
            .and_then(SmokingPolicy::from_code),
        vaping: raw_payload
            .vaping
            .as_deref()
            .and_then(VapingPolicy::from_code),
        staff_licenses,
        notes,
        evidence_urls: Vec::new(),
    })
}

fn strip_json_fences(input: &str) -> &str {
    let s = input.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }
    if let Some(rest) = s.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }
    s
}

async fn invoke_codex(cfg: &LlmConfig, prompt: &str) -> Result<String, LlmError> {
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        &sha256_hex(prompt.as_bytes())[..12]
    );
    let tmp_dir = std::env::temp_dir().join(format!("recovery-index-codex-{nonce}"));
    fs::create_dir_all(&tmp_dir).map_err(|source| LlmError::TempIo {
        backend: "codex",
        source,
    })?;
    let schema_path = tmp_dir.join("schema.json");
    let out_path = tmp_dir.join("out.json");
    let schema_bytes = serde_json::to_vec(&extraction_json_schema()).expect("valid JSON schema");
    fs::write(&schema_path, schema_bytes).map_err(|source| LlmError::TempIo {
        backend: "codex",
        source,
    })?;

    let mut cmd = Command::new("codex");
    cmd.arg("exec")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--ephemeral")
        .arg("--skip-git-repo-check")
        .arg("--color")
        .arg("never")
        .arg("--output-schema")
        .arg(&schema_path)
        .arg("-o")
        .arg(&out_path);
    if let Some(model) = &cfg.model {
        cmd.arg("-m").arg(model);
    }
    cmd.arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let run_result = run_cli_with_stdin(cmd, "codex", prompt).await;
    let out_text = fs::read_to_string(&out_path).ok();
    let _ = fs::remove_dir_all(&tmp_dir);

    let stdout = run_result?;
    Ok(out_text.filter(|s| !s.trim().is_empty()).unwrap_or(stdout))
}

async fn invoke_claude(cfg: &LlmConfig, prompt: &str) -> Result<String, LlmError> {
    let schema_str = serde_json::to_string(&extraction_json_schema()).expect("valid JSON schema");
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--json-schema")
        .arg(&schema_str)
        .arg("--no-session-persistence");
    if let Some(model) = &cfg.model {
        cmd.arg("--model").arg(model);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    run_cli_with_stdin(cmd, "claude", prompt).await
}

async fn run_cli_with_stdin(
    mut cmd: Command,
    program: &'static str,
    prompt: &str,
) -> Result<String, LlmError> {
    let mut child = cmd
        .spawn()
        .map_err(|source| LlmError::Spawn { program, source })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let output = tokio::time::timeout(CLI_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| LlmError::Timeout {
            program,
            secs: CLI_TIMEOUT.as_secs(),
        })?
        .map_err(|source| LlmError::Spawn { program, source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(LlmError::CliExit {
            program,
            code: output.status.code().unwrap_or(-1),
            stderr: truncate_utf8(&stderr, 400).to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn invoke_openai_compat(
    client: &reqwest::Client,
    cfg: &LlmConfig,
    prompt: &str,
) -> Result<String, LlmError> {
    let base_url = cfg.base_url.clone().unwrap_or_else(|| match cfg.backend {
        LlmBackend::Ollama => std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_string()),
        _ => std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
    });
    let model = cfg.effective_model_label();
    let api_key = cfg
        .api_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());
    if cfg.backend == LlmBackend::OpenAi && api_key.is_none() {
        return Err(LlmError::MissingApiKey);
    }

    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.0,
        "response_format": { "type": "json_object" },
        "messages": [
            {
                "role": "system",
                "content": "Return strictly valid JSON matching the requested schema. Extract only explicitly stated facts."
            },
            {
                "role": "user",
                "content": prompt
            }
        ]
    });

    let mut req = client
        .post(&endpoint)
        .timeout(CLI_TIMEOUT)
        .header("Content-Type", "application/json")
        .body(serde_json::to_vec(&body).expect("serializes"));
    if let Some(key) = api_key {
        req = req.header("Authorization", format!("Bearer {key}"));
    }

    let resp = req
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(LlmError::Http)?;
    let bytes = resp.bytes().await.map_err(LlmError::Http)?;
    let envelope: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|source| LlmError::Parse {
            preview: truncate_utf8(&String::from_utf8_lossy(&bytes), 240).to_string(),
            source,
        })?;
    let content = envelope
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    Ok(content.to_string())
}
