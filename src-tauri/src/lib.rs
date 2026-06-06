//! Tauri application entry point shared by desktop and mobile targets.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
