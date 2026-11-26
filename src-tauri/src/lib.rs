//! Tauri application entry point shared by desktop and mobile targets.

#[cfg(feature = "health-connect")]
pub mod alerts;
#[cfg(feature = "health-connect")]
pub mod health_connect;
#[cfg(feature = "health-connect")]
pub mod local_llm;
#[cfg(feature = "health-connect")]
pub mod personal_vault;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());

    #[cfg(all(
        feature = "health-connect",
        any(target_os = "android", target_os = "ios")
    ))]
    let builder = builder
        .plugin(health_connect::init_plugin())
        .plugin(alerts::init_plugin());

    #[cfg(all(feature = "health-connect", target_os = "android"))]
    let builder = builder
        .plugin(personal_vault::init_plugin())
        .plugin(local_llm::init_plugin());

    #[cfg(feature = "health-connect")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        alerts::app_alerts_status,
        alerts::app_alerts_request_permission,
        alerts::app_alerts_send_notification,
        health_connect::health_connect_status,
        health_connect::health_connect_request_permission,
        health_connect::health_connect_read_glucose,
        personal_vault::personal_vault_status,
        personal_vault::personal_vault_load,
        personal_vault::personal_vault_save,
        personal_vault::personal_vault_wipe,
        local_llm::local_llm_status,
        local_llm::local_llm_prepare,
        local_llm::local_llm_generate,
    ]);

    builder
        .run(tauri::generate_context!())
        .expect("error while running the Dabney Tauri application");
}

#[cfg(test)]
mod tests {
    /// Smoke test: the embedded Tauri context (identifier, icons, config)
    /// parses without panicking. Catches malformed `tauri.conf.json` at
    /// test time rather than at app startup.
    #[test]
    fn tauri_context_is_valid() {
        let _ctx: tauri::Context<tauri::Wry> = tauri::generate_context!();
    }
}
