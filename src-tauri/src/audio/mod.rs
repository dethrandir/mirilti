//! Ses yakalama alt sistemi (Faz 2: tam uygulama, §6).
//!
//! Cihaz tespiti (`device`), kayıt (`recorder`), yeniden örnekleme
//! (`resample`) ve WAV çıktısı (`wav`). Hedef net biçim **16 kHz mono i16**;
//! cihaz desteklemiyorsa doğal formatta yakalanıp `rubato` ile yeniden
//! örneklenir ve kanallar birleştirilir (downmix). ffmpeg yok (iki ilke:
//! "tıkla, açılsın" + ses dönüşümü Rust içinde).
//!
//! Bu birim henüz üretim kodundan çağrılmıyor (Tauri state makinesi Faz 8'de
//! bağlanır) — o yüzden modül seviyesinde `dead_code`'a izin verilir; her
//! öğe clippy `-D warnings` altında temiz kalsın diye bilinçlidir.

// Faz 2'ye dek yalnızca iskeletti; artık gerçek cihaz/kayıt/resample/wav var.
// Üretimden çağrılmadığı için (Faz 8) şimdilik ölü kod uyarısını kapat.
#![allow(dead_code)]

pub mod device;
pub mod recorder;
pub mod resample;
pub mod wav;

/// Hedef ses biçimi (PLAN §6): 16 kHz, mono, i16.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const TARGET_CHANNELS: u16 = 1;
pub const TARGET_BITS: u16 = 16;

/// Ses yakalama biriminin ortak hata türü.
///
/// Yazdırılabilir (Display) ve kaynak hatasını taşır; dış katmana
/// (Tauri command → frontend) `String` olarak iletilebilir.
#[derive(Debug)]
pub enum AudioError {
    IteratingDevices(cpal::DevicesError),
    DeviceNotAvailable(String),
    NoInputDevice,
    BadDeviceName(String),
    NoDefaultConfig,
    DefaultConfig(cpal::DefaultStreamConfigError),
    BuildStreamError(cpal::BuildStreamError),
    PlayStreamError(cpal::PlayStreamError),
    UnsupportedSampleFormat(cpal::SampleFormat),
    Recording(String),
    Resample(String),
    Wav(String),
    Io(std::io::Error),
}

impl core::fmt::Display for AudioError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AudioError::IteratingDevices(e) => write!(f, "cihazlar listelenemedi: {e}"),
            AudioError::DeviceNotAvailable(n) => write!(f, "seçili cihaz yok: {n}"),
            AudioError::NoInputDevice => write!(f, "giriş cihazı bulunamadı"),
            AudioError::BadDeviceName(n) => write!(f, "cihaz adı alınamadı: {n}"),
            AudioError::NoDefaultConfig => write!(f, "varsayılan girdi biçimi alınamadı"),
            AudioError::DefaultConfig(e) => write!(f, "varsayılan girdi biçimi hatası: {e}"),
            AudioError::BuildStreamError(e) => write!(f, "ses akışı kurulamadı: {e}"),
            AudioError::PlayStreamError(e) => write!(f, "ses akışı başlatılamadı: {e}"),
            AudioError::UnsupportedSampleFormat(fmt) => {
                write!(f, "desteklenmeyen örnek biçimi: {fmt}")
            }
            AudioError::Recording(e) => write!(f, "kayıt hatası: {e}"),
            AudioError::Resample(e) => write!(f, "yeniden örnekleme hatası: {e}"),
            AudioError::Wav(e) => write!(f, "WAV hatası: {e}"),
            AudioError::Io(e) => write!(f, "io hatası: {e}"),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AudioError::IteratingDevices(e) => Some(e),
            AudioError::DefaultConfig(e) => Some(e),
            AudioError::BuildStreamError(e) => Some(e),
            AudioError::PlayStreamError(e) => Some(e),
            AudioError::Io(e) => Some(e),
            _ => None,
        }
    }
}