//! Shared client-side Tauri v2 IPC bridge for opt-in personal mobile health sync
//! (Android Google Health Connect and iOS Apple HealthKit inside `moe.dabney.personal`).
//!
//! On web browsers and inside the public zero-PHI Google Play demo (`moe.dabney.demo`),
//! `health_connect_status` is absent or returns `enabled: false`, so all personal UI
//! stays hidden and zero health permissions exist in the demo app.

use serde::Deserialize;

/// Native health bridge platform reported by `health_connect_status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthBridgePlatform {
    AndroidHealthConnect,
    IosHealthKit,
    Other,
}

impl HealthBridgePlatform {
    pub fn from_wire(platform: &str) -> Self {
        match platform {
            "android_health_connect" => Self::AndroidHealthConnect,
            "ios_healthkit" => Self::IosHealthKit,
            _ => Self::Other,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::AndroidHealthConnect => "Android Health Connect",
            Self::IosHealthKit => "Apple HealthKit",
            Self::Other => "On-Device Health Bridge",
        }
    }

    pub fn permission_summary(self) -> &'static str {
        match self {
            Self::AndroidHealthConnect => "READ_BLOOD_GLUCOSE only · stays in device RAM",
            Self::IosHealthKit => {
                "HKQuantityTypeIdentifierBloodGlucose read-only · stays in device RAM"
            }
            Self::Other => "Read-only blood glucose · stays in device RAM",
        }
    }

    pub fn unavailable_hint(self) -> &'static str {
        match self {
            Self::AndroidHealthConnect => "Android Health Connect is not available on this device (Android 9+ with Health Connect required). You can still import Abbott Lingo CSV files below.",
            Self::IosHealthKit => "Apple HealthKit is not available on this device. You can still import an Apple Health export.xml or Abbott Lingo CSV file below.",
            Self::Other => "On-device health sync is not available on this device. You can still import CGM exports below.",
        }
    }

    pub fn empty_sync_hint(self, days: u32) -> String {
        match self {
            Self::AndroidHealthConnect => format!(
                "No Blood Glucose records found in Android Health Connect over the last {days} days. Confirm Abbott Lingo Health Connect sync is enabled."
            ),
            Self::IosHealthKit => format!(
                "No Blood Glucose samples found in Apple Health over the last {days} days. Confirm Abbott Lingo → Apple Health sync is enabled and Blood Glucose read access is granted in iOS Health Settings."
            ),
            Self::Other => format!("No Blood Glucose records found over the last {days} days."),
        }
    }
}

/// Runtime status returned by `health_connect_status` in `moe.dabney.personal`
/// (Cargo feature `health-connect`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HealthBridgeStatusUi {
    pub enabled: bool,
    pub platform: String,
    pub sdk_status: String,
    pub has_glucose_permission: bool,
    pub package_id: String,
}

impl HealthBridgeStatusUi {
    pub fn platform_kind(&self) -> HealthBridgePlatform {
        HealthBridgePlatform::from_wire(&self.platform)
    }
}

/// Outcome returned by `health_connect_request_permission`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HealthBridgePermissionUi {
    pub granted: bool,
    #[serde(default)]
    pub granted_permissions: Vec<String>,
}

/// Canonical `dabney.health_bridge.v1` payload returned by `health_connect_read_glucose`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HealthBridgeReadUi {
    pub bridge_json: String,
    pub record_count: usize,
    #[serde(default)]
    pub sources: Vec<String>,
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn js_error_to_string(err: wasm_bindgen::JsValue) -> String {
    use js_sys::{Reflect, JSON};
    use wasm_bindgen::JsValue;

    if let Some(s) = err.as_string() {
        return s;
    }
    if let Ok(msg) = Reflect::get(&err, &JsValue::from_str("message")) {
        if let Some(s) = msg.as_string() {
            return s;
        }
    }
    JSON::stringify(&err)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_else(|| "Tauri command failed".to_string())
}

/// Invoke an app-registered Tauri v2 command via `window.__TAURI__.core.invoke`
/// without extra JS-bridge crate dependencies. Calls `on_done(Err("not_tauri"))`
/// immediately when running in a plain browser.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn invoke_tauri_command<T, F>(cmd: &str, args: Option<js_sys::Object>, on_done: F)
where
    T: serde::de::DeserializeOwned + 'static,
    F: FnOnce(Result<T, String>) + 'static,
{
    use js_sys::{Function, Object, Reflect, JSON};
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::{closure::Closure, JsCast, JsValue};

    let Some(win) = web_sys::window() else {
        on_done(Err("not_tauri".to_string()));
        return;
    };
    let tauri =
        Reflect::get(win.as_ref(), &JsValue::from_str("__TAURI__")).unwrap_or(JsValue::UNDEFINED);
    if tauri.is_undefined() || tauri.is_null() {
        on_done(Err("not_tauri".to_string()));
        return;
    }
    let core = Reflect::get(&tauri, &JsValue::from_str("core")).unwrap_or(JsValue::UNDEFINED);
    if core.is_undefined() || core.is_null() {
        on_done(Err("not_tauri".to_string()));
        return;
    }
    let invoke = Reflect::get(&core, &JsValue::from_str("invoke")).unwrap_or(JsValue::UNDEFINED);
    let Some(invoke_fn) = invoke.dyn_ref::<Function>() else {
        on_done(Err("not_tauri".to_string()));
        return;
    };

    let args_obj = args.unwrap_or_else(Object::new);
    let Ok(promise_val) = invoke_fn.call2(&core, &JsValue::from_str(cmd), args_obj.as_ref()) else {
        on_done(Err(format!("Failed to invoke {cmd}")));
        return;
    };
    let then_val =
        Reflect::get(&promise_val, &JsValue::from_str("then")).unwrap_or(JsValue::UNDEFINED);
    let Some(then_fn) = then_val.dyn_ref::<Function>() else {
        on_done(Err(format!("Command {cmd} did not return a Promise")));
        return;
    };

    let cb_ok = Rc::new(RefCell::new(Some(on_done)));
    let cb_err = Rc::clone(&cb_ok);

    let on_ok = Closure::once_into_js(move |val: JsValue| {
        if let Some(done) = cb_ok.borrow_mut().take() {
            let parsed = JSON::stringify(&val)
                .ok()
                .and_then(|s| s.as_string())
                .ok_or_else(|| "Could not serialize Tauri response to JSON".to_string())
                .and_then(|raw| {
                    serde_json::from_str::<T>(&raw)
                        .map_err(|err| format!("Unexpected Tauri response shape: {err}"))
                });
            done(parsed);
        }
    });

    let on_err = Closure::once_into_js(move |err: JsValue| {
        if let Some(done) = cb_err.borrow_mut().take() {
            done(Err(js_error_to_string(err)));
        }
    });

    let _ = then_fn.call2(&promise_val, &on_ok, &on_err);
}

/// Probe `health_connect_status` silently; invokes `on_enabled` only when the
/// running shell is an opt-in personal build (`enabled == true`).
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn probe_personal_health_bridge<F>(on_enabled: F)
where
    F: FnOnce(HealthBridgeStatusUi) + 'static,
{
    invoke_tauri_command::<HealthBridgeStatusUi, _>("health_connect_status", None, move |res| {
        if let Ok(status) = res {
            if status.enabled {
                on_enabled(status);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_android_and_ios_health_bridge_platforms() {
        let android: HealthBridgeStatusUi = serde_json::from_str(
            r#"{
                "enabled": true,
                "platform": "android_health_connect",
                "sdk_status": "available",
                "has_glucose_permission": true,
                "package_id": "moe.dabney.personal"
            }"#,
        )
        .unwrap();
        assert_eq!(
            android.platform_kind(),
            HealthBridgePlatform::AndroidHealthConnect
        );
        assert_eq!(
            android.platform_kind().display_name(),
            "Android Health Connect"
        );

        let ios: HealthBridgeStatusUi = serde_json::from_str(
            r#"{
                "enabled": true,
                "platform": "ios_healthkit",
                "sdk_status": "available",
                "has_glucose_permission": false,
                "package_id": "moe.dabney.personal"
            }"#,
        )
        .unwrap();
        assert_eq!(ios.platform_kind(), HealthBridgePlatform::IosHealthKit);
        assert_eq!(ios.platform_kind().display_name(), "Apple HealthKit");
        assert!(ios
            .platform_kind()
            .permission_summary()
            .contains("HKQuantityTypeIdentifierBloodGlucose"));
    }
}
