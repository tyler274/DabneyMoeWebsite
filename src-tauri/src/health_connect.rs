//! Clean-room Tauri v2 command & mobile plugin bridge for Android Google Health Connect
//! (`moe.dabney.personal` personal build only; compiled out of the zero-PHI `moe.dabney.demo` app).

use serde::{Deserialize, Serialize};
#[cfg(target_os = "android")]
use tauri::{plugin::PluginHandle, Manager, Runtime};

#[cfg(target_os = "android")]
const ANDROID_PLUGIN_PACKAGE: &str = "moe.dabney.personal";
#[cfg(target_os = "android")]
const ANDROID_PLUGIN_CLASS: &str = "HealthConnectPlugin";

const DEFAULT_SYNC_DAYS: u32 = 14;
const MIN_SYNC_DAYS: u32 = 1;
const MAX_SYNC_DAYS: u32 = 180;

/// Health Connect availability and permission status returned to the WebView UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthConnectStatus {
    pub enabled: bool,
    pub platform: String,
    pub sdk_status: String,
    pub has_glucose_permission: bool,
    pub package_id: String,
}

/// Outcome of the Android Health Connect runtime permission sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthConnectPermissionResult {
    pub granted: bool,
    pub granted_permissions: Vec<String>,
}

/// Canonical `dabney.health_bridge.v1` JSON payload returned by Health Connect reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthConnectReadResponse {
    pub bridge_json: String,
    pub record_count: usize,
    pub sources: Vec<String>,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct ReadGlucosePayload {
    days: u32,
}

#[cfg(target_os = "android")]
struct HealthConnectPluginState<R: Runtime>(PluginHandle<R>);

/// Clamp requested Health Connect sync window to a safe bounded day span (`1..=180`).
pub fn clamp_sync_days(days: Option<u32>) -> u32 {
    days.unwrap_or(DEFAULT_SYNC_DAYS)
        .clamp(MIN_SYNC_DAYS, MAX_SYNC_DAYS)
}

/// Register the Android `HealthConnectPlugin` Tauri plugin when running on Android.
#[cfg(target_os = "android")]
pub fn init_plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("dabney-health-connect")
        .setup(|app, api| {
            let handle =
                api.register_android_plugin(ANDROID_PLUGIN_PACKAGE, ANDROID_PLUGIN_CLASS)?;
            app.manage(HealthConnectPluginState(handle));
            Ok(())
        })
        .build()
}

/// Query Health Connect SDK availability and blood-glucose read permission state.
#[tauri::command]
pub async fn health_connect_status(app: tauri::AppHandle) -> Result<HealthConnectStatus, String> {
    #[cfg(target_os = "android")]
    {
        let state = app.state::<HealthConnectPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<HealthConnectStatus>("checkStatus", serde_json::json!({}))
            .await
            .map_err(|err| format!("Health Connect status failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(HealthConnectStatus {
            enabled: false,
            platform: "desktop".to_string(),
            sdk_status: "android_only".to_string(),
            has_glucose_permission: false,
            package_id: "moe.dabney.desktop".to_string(),
        })
    }
}

/// Launch the Android Health Connect `READ_BLOOD_GLUCOSE` permission sheet.
#[tauri::command]
pub async fn health_connect_request_permission(
    app: tauri::AppHandle,
) -> Result<HealthConnectPermissionResult, String> {
    #[cfg(target_os = "android")]
    {
        let state = app.state::<HealthConnectPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<HealthConnectPermissionResult>(
                "requestGlucosePermission",
                serde_json::json!({}),
            )
            .await
            .map_err(|err| format!("Health Connect permission request failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("Google Health Connect is available only in the Android personal build (moe.dabney.personal).".to_string())
    }
}

/// Read personal `BloodGlucoseRecord` entries from Android Health Connect and return them
/// serialized as canonical `dabney.health_bridge.v1` JSON.
#[tauri::command]
pub async fn health_connect_read_glucose(
    app: tauri::AppHandle,
    days: Option<u32>,
) -> Result<HealthConnectReadResponse, String> {
    let clamped = clamp_sync_days(days);
    #[cfg(target_os = "android")]
    {
        let state = app.state::<HealthConnectPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<HealthConnectReadResponse>(
                "readBloodGlucose",
                ReadGlucosePayload { days: clamped },
            )
            .await
            .map_err(|err| format!("Health Connect glucose read failed: {err}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, clamped);
        Err("Google Health Connect is available only in the Android personal build (moe.dabney.personal).".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_requested_sync_days_to_safe_bounds() {
        assert_eq!(clamp_sync_days(None), 14);
        assert_eq!(clamp_sync_days(Some(0)), 1);
        assert_eq!(clamp_sync_days(Some(30)), 30);
        assert_eq!(clamp_sync_days(Some(999)), 180);
    }

    #[test]
    fn bridge_response_roundtrips_json() {
        let payload = HealthConnectReadResponse {
            bridge_json: r#"{"schema":"dabney.health_bridge.v1","platform":"android_health_connect","records":[]}"#.to_string(),
            record_count: 0,
            sources: vec!["com.abbott.lingo".to_string()],
        };
        let encoded = serde_json::to_string(&payload).expect("serialize");
        let decoded: HealthConnectReadResponse =
            serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, payload);
    }
}
