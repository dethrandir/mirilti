// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

// --- Faz 0: modül iskeleti (§3.3 dizin düzeni) ---
// Aşağıdaki modüller şimdilik yalnızca iskelet (boş mod.rs + alt modül
// dosyaları) olarak bağlanır; amacı `cargo check` ve
// `cargo clippy --all-targets -- -D warnings` temiz geçecek şekilde
// derlenebilir bir dizin ağacı kurmaktır. Gerçek implementasyonlar
// ilerleyen fazlarda (Faz 1+) eklenir.
mod audio;
mod cli;
mod engine;
mod models;
mod output;
mod pipeline;
mod shortcut;
mod store;
mod system;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}