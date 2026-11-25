//! Tauri application entry point shared by desktop and mobile targets.

#[cfg(feature = "health-connect")]
pub mod health_connect;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());

    #[cfg(all(feature = "health-connect", target_os = "android"))]
    let builder = builder.plugin(health_connect::init_plugin());

    #[cfg(feature = "health-connect")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        health_connect::health_connect_status,
        health_connect::health_connect_request_permission,
        health_connect::health_connect_read_glucose,
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
