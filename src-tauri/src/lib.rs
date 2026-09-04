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

use tauri::{AppHandle, Manager, PhysicalPosition, Runtime, WebviewWindow};

// Overlay penceresinin etiketi — tauri.conf.json `app.windows` içinde
// (label: "overlay", url: "src-overlay/overlay.html") bildirilir. Tauri bu
// pencereyi başlangıçta otomatik örnekler (visible:false doğar); buradaki kod
// onu çalışma zamanında bulur ve gösterir/gizler.
const OVERLAY_LABEL: &str = "overlay";

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// Faz 1a stub: overlay penceresini gösterir/gizler (birden fazla pencere
/// yönetimi). Ana pencereden `invoke('set_overlay_visible', { visible })`
/// ile çağrılır. Gerçek durum makinesi (§3.2) Faz 1+ ile bunu sürer.
#[tauri::command]
fn set_overlay_visible<R: Runtime>(app: AppHandle<R>, visible: bool) -> Result<(), String> {
    let window = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| format!("overlay pencere ({OVERLAY_LABEL}) bulunamadı"))?;
    if visible {
        window.show().map_err(|e| e.to_string())
    } else {
        window.hide().map_err(|e| e.to_string())
    }
}

/// Overlay'i sağ-alt köşeye yerleştirmek için en iyi çaba.
///
/// UYARI (KWin/Wayland): Wayland üzerinde istemci mutlak pencere konumu
/// belirleyemez; `set_position` X11'de çalışır ama KWin/Wayland'de yok sayılır.
/// Güvenilir "sağ-alt köşe + üstte sabit" için KWin pencere kuralı gerekir
/// (NOTES.md — 3 satır: KWin Sistemi Ayarları > Pencere Yönetimi > Pencereleri
/// Kuralları'na "mirilti-overlay" sınıfını ekle; "Keep above = Force" +
/// "Position = Force  Bottom Right"; X11'de alwaysOnTop yeterli, Wayland'de kural şart).
/// Faz 1d: uçtan uca kıvılcım — en küçük çalışan zincir.
///
/// Komut, `invoke('toggle_dictation')` ile çağrılırsa (ana pencereden ya da
/// Faz 8'de GlobalShortcuts tetikleyicisinden):
///   1. Overlay'i YAK — `set_overlay_visible(app, true)` (nefes alan halka).
///   2. ~1 sn bekle — kayıt simülasyonu.
///   3. Sahte metni yapıştır — `output::inject::paste` ile gerçek odaktaki
///      uygulamaya (§8.1 zincir: portal → ydotool → X11 → pano).
///   4. Overlay'i SÖNDÜR — paste sonucundan bağımsız olarak her koşulda gizler.
///
/// # Not
/// Tauri command'leri tokio worker'ında async koşar. `tokio` bu crate'te
/// yalnızca dev-dependency'dir (üretim kodu Tauri'nin kendi runtime'ını
/// kullanır) ve `time` feature'ı yoktur; bu yüzden kayıt simülasyonu beklemesi
/// 1 saniyelik blocking `std::thread::sleep` ile yapılır — spike için kabul
/// edilebilir; gerçek akış (Faz 8) bunu tokio-quantumlu beklemeye çevirir.
#[tauri::command]
async fn toggle_dictation<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    // 1) Overlay'i göster (kayıt durumu = nefes alan halka görünür).
    set_overlay_visible(app.clone(), true)?;

    // 2) ~1 sn bekle ("kayıt" simülasyonu).
    std::thread::sleep(std::time::Duration::from_secs(1));

    // 3) Sahte metni gerçek odaktaki uygulamaya yapıştır (§8.1 zincir).
    let result = crate::output::inject::paste("mirilti uçtan uca çalışıyor").await;

    // 4) Overlay'i her koşulda söndür.
    set_overlay_visible(app, false)?;

    Ok(format!(
        "{} — pano yazıldı: {}, Ctrl+V gönderildi: {}",
        result.backend.label(),
        result.clipboard_written,
        result.ctrl_v_sent
    ))
}

fn pin_overlay_bottom_right<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "monitör bulunamadı".to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let mpos = monitor.position();
    let msize = monitor.size();
    let margin = 24i32;
    window
        .set_position(PhysicalPosition::new(
            mpos.x + (msize.width as i32) - (size.width as i32) - margin,
            mpos.y + (msize.height as i32) - (size.height as i32) - margin,
        ))
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, set_overlay_visible, toggle_dictation])
        .setup(|app| {
            // Overlay penceresi tauri.conf.json'dan zaten doğdu; burada yalnızca
            // konum denemesi yapıyoruz (KWin/Wayland'de kural şart, yukarıya bak).
            if let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) {
                if let Err(e) = pin_overlay_bottom_right(&overlay) {
                    eprintln!("overlay: sağ-alt köşeye konumlanamadı: {e}");
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}