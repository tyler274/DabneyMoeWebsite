//! Clean-room Tauri v2 bridge for the encrypted on-device personal vault
//! (`moe.dabney.personal` only; compiled out of the zero-PHI `moe.dabney.demo` app).

use serde::{Deserialize, Serialize};
#[cfg(target_os = "android")]
use tauri::{plugin::PluginHandle, Manager, Runtime};

#[cfg(target_os = "android")]
const ANDROID_PLUGIN_PACKAGE: &str = "moe.dabney.personal";
#[cfg(target_os = "android")]
const ANDROID_PLUGIN_CLASS: &str = "PersonalVaultPlugin";

/// Maximum accepted JSON payload size per field (`16 MiB` bounded cap).
pub const MAX_VAULT_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Summary of the encrypted on-device personal vault.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalVaultStatus {
    pub enabled: bool,
    pub encrypted: bool,
    pub cipher: String,
    pub storage_backend: String,
    pub has_cgm_snapshot: bool,
    pub cgm_reading_count: usize,
    pub cgm_saved_at: Option<String>,
    pub checkin_count: usize,
    pub note_count: usize,
    pub chat_turn_count: usize,
    pub total_bytes: u64,
}

/// Decrypted snapshot restored from the encrypted personal vault on app start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalVaultSnapshot {
    pub found: bool,
    pub cgm_bridge_json: Option<String>,
    pub cgm_source_label: Option<String>,
    pub cgm_saved_at: Option<String>,
    pub cgm_reading_count: usize,
    pub checkins_json: Option<String>,
    pub notes_json: Option<String>,
    pub chat_history_json: Option<String>,
}

/// Partial update payload merged into the encrypted personal vault.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalVaultSaveRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgm_bridge_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgm_source_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgm_reading_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkins_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_history_json: Option<String>,
}

impl PersonalVaultSaveRequest {
    pub fn total_bytes(&self) -> usize {
        self.cgm_bridge_json.as_deref().map_or(0, str::len)
            + self.cgm_source_label.as_deref().map_or(0, str::len)
            + self.checkins_json.as_deref().map_or(0, str::len)
            + self.notes_json.as_deref().map_or(0, str::len)
            + self.chat_history_json.as_deref().map_or(0, str::len)
    }

    pub fn validate(&self) -> Result<(), String> {
        let bytes = self.total_bytes();
        if bytes > MAX_VAULT_PAYLOAD_BYTES {
            return Err(format!(
                "Personal vault save payload ({bytes} bytes) exceeds 16 MiB cap."
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "android")]
struct PersonalVaultPluginState<R: Runtime>(PluginHandle<R>);

/// Register the Android `PersonalVaultPlugin` Tauri plugin on Android personal builds.
#[cfg(target_os = "android")]
pub fn init_plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("dabney-personal-vault")
        .setup(|app, api| {
            let handle =
                api.register_android_plugin(ANDROID_PLUGIN_PACKAGE, ANDROID_PLUGIN_CLASS)?;
            app.manage(PersonalVaultPluginState(handle));
            Ok(())
        })
        .build()
}

/// Query encrypted personal vault availability and record counts.
#[tauri::command]
pub async fn personal_vault_status(app: tauri::AppHandle) -> Result<PersonalVaultStatus, String> {
    #[cfg(target_os = "android")]
    {
        let state = app.state::<PersonalVaultPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<PersonalVaultStatus>("getStatus", serde_json::json!({}))
            .await
            .map_err(|err| format!("Personal vault status failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(PersonalVaultStatus {
            enabled: false,
            encrypted: false,
            cipher: "none".to_string(),
            storage_backend: "android_only".to_string(),
            has_cgm_snapshot: false,
            cgm_reading_count: 0,
            cgm_saved_at: None,
            checkin_count: 0,
            note_count: 0,
            chat_turn_count: 0,
            total_bytes: 0,
        })
    }
}

/// Load and decrypt the personal vault snapshot into local app RAM.
#[tauri::command]
pub async fn personal_vault_load(app: tauri::AppHandle) -> Result<PersonalVaultSnapshot, String> {
    #[cfg(target_os = "android")]
    {
        let state = app.state::<PersonalVaultPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<PersonalVaultSnapshot>("loadSnapshot", serde_json::json!({}))
            .await
            .map_err(|err| format!("Personal vault load failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(PersonalVaultSnapshot {
            found: false,
            cgm_bridge_json: None,
            cgm_source_label: None,
            cgm_saved_at: None,
            cgm_reading_count: 0,
            checkins_json: None,
            notes_json: None,
            chat_history_json: None,
        })
    }
}

/// Merge and encrypt updated personal sections atomically inside the local vault.
#[tauri::command]
pub async fn personal_vault_save(
    app: tauri::AppHandle,
    payload: PersonalVaultSaveRequest,
) -> Result<PersonalVaultStatus, String> {
    payload.validate()?;
    #[cfg(target_os = "android")]
    {
        let state = app.state::<PersonalVaultPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<PersonalVaultStatus>("saveSnapshot", payload)
            .await
            .map_err(|err| format!("Personal vault save failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, payload);
        Err("Encrypted personal vault is available only in the Android personal build (moe.dabney.personal).".to_string())
    }
}

/// Cryptographically wipe encrypted vault files and destroy the Android Keystore key alias.
#[tauri::command]
pub async fn personal_vault_wipe(app: tauri::AppHandle) -> Result<PersonalVaultStatus, String> {
    #[cfg(target_os = "android")]
    {
        let state = app.state::<PersonalVaultPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<PersonalVaultStatus>("wipeVault", serde_json::json!({}))
            .await
            .map_err(|err| format!("Personal vault wipe failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("Encrypted personal vault is available only in the Android personal build (moe.dabney.personal).".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_save_request_enforces_size_cap() {
        let small = PersonalVaultSaveRequest {
            cgm_bridge_json: Some("{\"records\":[]}".to_string()),
            cgm_reading_count: Some(0),
            ..Default::default()
        };
        assert!(small.validate().is_ok());

        let oversized = PersonalVaultSaveRequest {
            cgm_bridge_json: Some("x".repeat(MAX_VAULT_PAYLOAD_BYTES + 1)),
            ..Default::default()
        };
        assert!(oversized.validate().is_err());
    }

    #[test]
    fn vault_status_roundtrips_json() {
        let status = PersonalVaultStatus {
            enabled: true,
            encrypted: true,
            cipher: "AES-256-GCM (Android Keystore)".to_string(),
            storage_backend: "android_keystore_files".to_string(),
            has_cgm_snapshot: true,
            cgm_reading_count: 144,
            cgm_saved_at: Some("2026-11-25T19:00:00Z".to_string()),
            checkin_count: 3,
            note_count: 2,
            chat_turn_count: 4,
            total_bytes: 8192,
        };
        let encoded = serde_json::to_string(&status).unwrap();
        let decoded: PersonalVaultStatus = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, status);
    }
}
