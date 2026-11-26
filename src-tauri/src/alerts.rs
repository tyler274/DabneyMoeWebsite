//! Clean-room Tauri v2 command & mobile plugin bridge for opt-in personal glucose
//! range alert notifications (`moe.dabney.personal` only; compiled out of the
//! zero-PHI `moe.dabney.demo` app).

use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "android", target_os = "ios"))]
use tauri::{plugin::PluginHandle, Manager, Runtime};

#[cfg(target_os = "android")]
const ANDROID_PLUGIN_PACKAGE: &str = "moe.dabney.personal";
#[cfg(target_os = "android")]
const ANDROID_PLUGIN_CLASS: &str = "GlucoseAlertPlugin";

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_dabney_glucose_alerts);

/// Maximum accepted character length for a notification title.
pub const MAX_ALERT_TITLE_CHARS: usize = 160;
/// Maximum accepted character length for a notification body.
pub const MAX_ALERT_BODY_CHARS: usize = 512;
/// Maximum accepted character length for a notification deduplication tag.
pub const MAX_ALERT_TAG_CHARS: usize = 96;

/// Native mobile notification availability and permission status returned to the WebView UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAlertStatus {
    pub enabled: bool,
    pub platform: String,
    pub permission_granted: bool,
    pub permission_status: String,
    pub channel_id: String,
}

/// Result of requesting native mobile notification permissions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAlertPermissionResult {
    pub granted: bool,
    pub status: String,
}

/// Request payload for dispatching an opt-in local glucose range alert notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAlertNotificationRequest {
    pub title: String,
    pub body: String,
    pub severity: String,
    pub tag: String,
}

impl AppAlertNotificationRequest {
    /// Validate non-empty content and safe character length bounds before bridging to mobile.
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("Alert notification title cannot be empty.".to_string());
        }
        if self.title.chars().count() > MAX_ALERT_TITLE_CHARS {
            return Err(format!(
                "Alert notification title exceeds {MAX_ALERT_TITLE_CHARS} characters."
            ));
        }
        if self.body.trim().is_empty() {
            return Err("Alert notification body cannot be empty.".to_string());
        }
        if self.body.chars().count() > MAX_ALERT_BODY_CHARS {
            return Err(format!(
                "Alert notification body exceeds {MAX_ALERT_BODY_CHARS} characters."
            ));
        }
        if self.tag.chars().count() > MAX_ALERT_TAG_CHARS {
            return Err(format!(
                "Alert notification tag exceeds {MAX_ALERT_TAG_CHARS} characters."
            ));
        }
        Ok(())
    }
}

/// Result of dispatching a local glucose range alert notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAlertDispatchResult {
    pub delivered: bool,
    pub notification_id: i32,
    pub detail: String,
}

#[cfg(any(target_os = "android", target_os = "ios"))]
struct GlucoseAlertPluginState<R: Runtime>(PluginHandle<R>);

/// Register the native Android `GlucoseAlertPlugin` or iOS `GlucoseAlertPlugin` Tauri mobile plugin.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn init_plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("dabney-glucose-alerts")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let handle =
                api.register_android_plugin(ANDROID_PLUGIN_PACKAGE, ANDROID_PLUGIN_CLASS)?;
            #[cfg(target_os = "ios")]
            let handle = api.register_ios_plugin(init_plugin_dabney_glucose_alerts)?;
            app.manage(GlucoseAlertPluginState(handle));
            Ok(())
        })
        .build()
}

/// Query native notification bridge availability and permission state.
/// Desktop shells return `enabled: false` with `permission_status: "webview_fallback"`
/// so desktop WebViews fall back cleanly to the browser Web Notification API.
#[tauri::command]
pub async fn app_alerts_status(app: tauri::AppHandle) -> Result<AppAlertStatus, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let state = app.state::<GlucoseAlertPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<AppAlertStatus>("checkStatus", serde_json::json!({}))
            .await
            .map_err(|err| format!("Native alert status failed: {err}"))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = app;
        Ok(AppAlertStatus {
            enabled: false,
            platform: "desktop".to_string(),
            permission_granted: false,
            permission_status: "webview_fallback".to_string(),
            channel_id: String::new(),
        })
    }
}

/// Request runtime notification permission on mobile personal builds.
#[tauri::command]
pub async fn app_alerts_request_permission(
    app: tauri::AppHandle,
) -> Result<AppAlertPermissionResult, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let state = app.state::<GlucoseAlertPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<AppAlertPermissionResult>(
                "requestPermission",
                serde_json::json!({}),
            )
            .await
            .map_err(|err| format!("Native alert permission request failed: {err}"))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = app;
        Ok(AppAlertPermissionResult {
            granted: false,
            status: "webview_fallback".to_string(),
        })
    }
}

/// Validate and dispatch a local privacy-preserving notification on mobile personal builds.
#[tauri::command]
pub async fn app_alerts_send_notification(
    app: tauri::AppHandle,
    payload: AppAlertNotificationRequest,
) -> Result<AppAlertDispatchResult, String> {
    payload.validate()?;
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let state = app.state::<GlucoseAlertPluginState<tauri::Wry>>();
        state
            .0
            .run_mobile_plugin_async::<AppAlertDispatchResult>("sendNotification", payload)
            .await
            .map_err(|err| format!("Native alert notification failed: {err}"))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = (app, payload);
        Err("Native mobile glucose notifications are available only in the mobile personal build (moe.dabney.personal); use Web Notifications on desktop.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> AppAlertNotificationRequest {
        AppAlertNotificationRequest {
            title: "Glucose range alert".to_string(),
            body: "Recent sensor reading crossed your configured target band.".to_string(),
            severity: "warning".to_string(),
            tag: "glucose-range-low".to_string(),
        }
    }

    #[test]
    fn notification_request_validation_enforces_non_empty_and_length_bounds() {
        assert!(sample_request().validate().is_ok());

        let mut empty_title = sample_request();
        empty_title.title = "   ".to_string();
        assert!(empty_title.validate().is_err());

        let mut empty_body = sample_request();
        empty_body.body = "\t\n".to_string();
        assert!(empty_body.validate().is_err());

        let mut empty_tag = sample_request();
        empty_tag.tag = String::new();
        assert!(empty_tag.validate().is_ok());

        let mut exact_bounds = sample_request();
        exact_bounds.title = "é".repeat(MAX_ALERT_TITLE_CHARS);
        exact_bounds.body = "≤".repeat(MAX_ALERT_BODY_CHARS);
        exact_bounds.tag = "a".repeat(MAX_ALERT_TAG_CHARS);
        assert!(exact_bounds.validate().is_ok());

        let mut long_title = sample_request();
        long_title.title = "a".repeat(MAX_ALERT_TITLE_CHARS + 1);
        assert!(long_title.validate().is_err());

        let mut long_body = sample_request();
        long_body.body = "b".repeat(MAX_ALERT_BODY_CHARS + 1);
        assert!(long_body.validate().is_err());

        let mut long_tag = sample_request();
        long_tag.tag = "t".repeat(MAX_ALERT_TAG_CHARS + 1);
        assert!(long_tag.validate().is_err());
    }

    #[test]
    fn alert_wire_types_roundtrip_json() {
        let status = AppAlertStatus {
            enabled: true,
            platform: "android_notification_manager".to_string(),
            permission_granted: true,
            permission_status: "granted".to_string(),
            channel_id: "dabney_glucose_alerts".to_string(),
        };
        let encoded = serde_json::to_string(&status).expect("serialize status");
        let decoded: AppAlertStatus = serde_json::from_str(&encoded).expect("deserialize status");
        assert_eq!(decoded, status);

        let perm = AppAlertPermissionResult {
            granted: false,
            status: "prompt".to_string(),
        };
        let encoded_perm = serde_json::to_string(&perm).expect("serialize permission");
        let decoded_perm: AppAlertPermissionResult =
            serde_json::from_str(&encoded_perm).expect("deserialize permission");
        assert_eq!(decoded_perm, perm);

        let req = sample_request();
        let encoded_req = serde_json::to_string(&req).expect("serialize request");
        let decoded_req: AppAlertNotificationRequest =
            serde_json::from_str(&encoded_req).expect("deserialize request");
        assert_eq!(decoded_req, req);

        let dispatch = AppAlertDispatchResult {
            delivered: true,
            notification_id: 4242,
            detail: "Posted to dabney_glucose_alerts".to_string(),
        };
        let encoded_dispatch = serde_json::to_string(&dispatch).expect("serialize dispatch");
        let decoded_dispatch: AppAlertDispatchResult =
            serde_json::from_str(&encoded_dispatch).expect("deserialize dispatch");
        assert_eq!(decoded_dispatch, dispatch);
    }
}
