//! Clean-room Tauri v2 bridge for on-device LLM inference on supported hardware
//! (Android AICore / Gemini Nano via ML Kit GenAI Prompt API inside `moe.dabney.personal`;
//! compiled out of the zero-PHI `moe.dabney.demo` app).

use serde::{Deserialize, Serialize};
#[cfg(target_os = "android")]
use tauri::{plugin::PluginHandle, Manager, Runtime};

#[cfg(target_os = "android")]
const ANDROID_PLUGIN_PACKAGE: &str = "moe.dabney.personal";
#[cfg(target_os = "android")]
const ANDROID_PLUGIN_CLASS: &str = "LocalLlmPlugin";

const MIN_OUTPUT_TOKENS: u32 = 64;
const DEFAULT_OUTPUT_TOKENS: u32 = 512;
const MAX_OUTPUT_TOKENS: u32 = 1024;
const MAX_PROMPT_CHARS: usize = 12_000;

/// Device capability and on-device LLM availability status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLlmStatus {
    pub enabled: bool,
    pub backend: String,
    pub model_name: String,
    pub availability: String,
    pub device_summary: String,
    pub total_ram_mb: u64,
    pub aicore_installed: bool,
    pub detail: String,
}

/// Result of preparing/downloading the on-device model via Android AICore.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLlmPrepareResult {
    pub ready: bool,
    pub status: String,
    pub detail: String,
}

/// Output from a local on-device LLM generation turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLlmGenerateResponse {
    pub text: String,
    pub backend: String,
    pub model_name: String,
    pub latency_ms: u64,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct GeneratePayload {
    prompt: String,
    max_tokens: u32,
}

#[cfg(target_os = "android")]
struct LocalLlmPluginState<R: Runtime>(PluginHandle<R>);

/// Clamp requested output token count to safe on-device bounds (`64..=1024`).
pub fn clamp_max_tokens(tokens: Option<u32>) -> u32 {
    tokens
        .unwrap_or(DEFAULT_OUTPUT_TOKENS)
        .clamp(MIN_OUTPUT_TOKENS, MAX_OUTPUT_TOKENS)
}

/// Sanitize and bound prompt length for on-device NPU context windows.
pub fn bound_prompt(prompt: &str) -> Result<String, String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }
    let bounded: String = trimmed.chars().take(MAX_PROMPT_CHARS).collect();
    Ok(bounded)
}

/// Register the Android `LocalLlmPlugin` Tauri plugin on Android personal builds.
#[cfg(target_os = "android")]
pub fn init_plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("dabney-local-llm")
        .setup(|app, api| {
            let handle =
                api.register_android_plugin(ANDROID_PLUGIN_PACKAGE, ANDROID_PLUGIN_CLASS)?;
            app.manage(LocalLlmPluginState(handle));
            Ok(())
        })
        .build()
}

/// Probe hardware capability and Android AICore / Gemini Nano availability.
#[tauri::command]
pub async fn local_llm_status(app: tauri::AppHandle) -> Result<LocalLlmStatus, String> {
    #[cfg(target_os = "android")]
    {
        let state = app.state::<LocalLlmPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<LocalLlmStatus>("checkStatus", serde_json::json!({}))
            .await
            .map_err(|err| format!("Local LLM status failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(LocalLlmStatus {
            enabled: false,
            backend: "grounded_rust".to_string(),
            model_name: "Grounded Rust Synthesis".to_string(),
            availability: "unavailable".to_string(),
            device_summary: "Desktop shell".to_string(),
            total_ram_mb: 0,
            aicore_installed: false,
            detail: "Android AICore / Gemini Nano runs on supported Android devices; grounded Rust synthesis is active.".to_string(),
        })
    }
}

/// Trigger on-device Gemini Nano download via Android AICore if status is `downloadable`.
#[tauri::command]
pub async fn local_llm_prepare(app: tauri::AppHandle) -> Result<LocalLlmPrepareResult, String> {
    #[cfg(target_os = "android")]
    {
        let state = app.state::<LocalLlmPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<LocalLlmPrepareResult>("prepareModel", serde_json::json!({}))
            .await
            .map_err(|err| format!("Local LLM prepare failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(LocalLlmPrepareResult {
            ready: false,
            status: "unavailable".to_string(),
            detail: "Android AICore model download is available only on supported Android devices."
                .to_string(),
        })
    }
}

/// Generate a grounded response strictly on-device via Android AICore / Gemini Nano.
#[tauri::command]
pub async fn local_llm_generate(
    app: tauri::AppHandle,
    prompt: String,
    max_tokens: Option<u32>,
) -> Result<LocalLlmGenerateResponse, String> {
    let bounded = bound_prompt(&prompt)?;
    let clamped_tokens = clamp_max_tokens(max_tokens);
    #[cfg(target_os = "android")]
    {
        let state = app.state::<LocalLlmPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<LocalLlmGenerateResponse>(
                "generate",
                GeneratePayload {
                    prompt: bounded,
                    max_tokens: clamped_tokens,
                },
            )
            .await
            .map_err(|err| format!("Local LLM generation failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, bounded, clamped_tokens);
        Err("On-device Gemini Nano generation is available only on supported Android personal builds.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_tokens_and_bounds_prompt() {
        assert_eq!(clamp_max_tokens(None), 512);
        assert_eq!(clamp_max_tokens(Some(10)), 64);
        assert_eq!(clamp_max_tokens(Some(300)), 300);
        assert_eq!(clamp_max_tokens(Some(9999)), 1024);

        assert!(bound_prompt("   ").is_err());
        let long = "a".repeat(MAX_PROMPT_CHARS + 500);
        assert_eq!(
            bound_prompt(&long).unwrap().chars().count(),
            MAX_PROMPT_CHARS
        );
    }

    #[test]
    fn local_llm_status_roundtrips_json() {
        let status = LocalLlmStatus {
            enabled: true,
            backend: "android_aicore_gemini_nano".to_string(),
            model_name: "Gemini Nano (Android AICore)".to_string(),
            availability: "available".to_string(),
            device_summary: "Android Flagship · Snapdragon · 12.0 GB RAM · API 36".to_string(),
            total_ram_mb: 12288,
            aicore_installed: true,
            detail: "Ready".to_string(),
        };
        let encoded = serde_json::to_string(&status).unwrap();
        let decoded: LocalLlmStatus = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, status);
    }
}
