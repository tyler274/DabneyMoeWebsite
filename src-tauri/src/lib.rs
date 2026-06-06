//! Tauri application entry point shared by desktop and mobile targets.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the Dabney Tauri application");
}
