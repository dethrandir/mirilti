//! Ses giriş cihazı listesi ve özellik tespiti (§6, Faz 2).
//!
//! `cpal` PipeWire/ALSA üzerinden çalışır: her giriş cihazının adını,
//! varsayılan olup olmadığını ve cihazın doğal yakalama biçimini raporlar.
//! Doğal biçimin 16 kHz mono i16 olup olmadığı "doğrudan kayıt" kararını
//! besler (bkz. [`crate::audio::recorder`]) — V8 varsayımı buradan test edilir.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

use crate::audio::{AudioError, TARGET_CHANNELS, TARGET_SAMPLE_RATE};

/// Bir cihazın doğal (native) yakalama biçimi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureFormat {
    /// Kanal sayısı (ör. 1 mono, 2 stereo, 6 5.1).
    pub channels: u16,
    /// Örnek oranı (ör. 48_000).
    pub sample_rate: u32,
    /// Örnek biçimi adı: "i16", "u16", "f32", "i32", ...
    pub sample_format: &'static str,
}

impl CaptureFormat {
    /// [`cpal::SupportedStreamConfig`]'ten özellikleri çıkarır.
    pub fn from_supported(cfg: &cpal::SupportedStreamConfig) -> Self {
        let sample_format = match cfg.sample_format() {
            cpal::SampleFormat::I8 => "i8",
            cpal::SampleFormat::I16 => "i16",
            cpal::SampleFormat::I32 => "i32",
            cpal::SampleFormat::I64 => "i64",
            cpal::SampleFormat::U8 => "u8",
            cpal::SampleFormat::U16 => "u16",
            cpal::SampleFormat::U32 => "u32",
            cpal::SampleFormat::U64 => "u64",
            cpal::SampleFormat::F32 => "f32",
            cpal::SampleFormat::F64 => "f64",
            // cpal `SampleFormat` non-exhaustive; gelecekteki biçimler.
            _ => "bilinmeyen",
        };
        Self {
            channels: cfg.channels(),
            sample_rate: cfg.sample_rate().0,
            sample_format,
        }
    }

    /// Biçim doğrudan hedefe (16 kHz mono) uyuyor mu? Uyuyorsa kayıt
    /// yeniden örneklemesiz yapılabilir (V8 kapalıysa rubato yolu gerekir).
    pub fn is_direct_16k_mono(&self) -> bool {
        self.channels == TARGET_CHANNELS && self.sample_rate == TARGET_SAMPLE_RATE
    }
}

/// Listelenebilir bir ses giriş cihazı.
#[derive(Debug, Clone, Serialize)]
pub struct InputDevice {
    pub name: String,
    pub is_default: bool,
    /// Cihazın doğal yakalama biçimi; alınamazsa `None`.
    pub native: Option<CaptureFormat>,
}

impl InputDevice {
    /// Cihaz doğrudan 16 kHz mono i16 veriyor mu?
    pub fn direct_16k_mono(&self) -> bool {
        self.native.as_ref().is_some_and(CaptureFormat::is_direct_16k_mono)
    }
}

/// Varsayılan girdi cihazının doğal biçimini döndürür (V8 doğrulaması).
///
/// Tespit böyle yapılır: `host.default_input_device()` → `default_input_config()`
/// → `SupportedStreamConfig` → [`CaptureFormat`].
pub fn probe_default_input_config() -> Result<Option<(String, CaptureFormat)>, AudioError> {
    let host = cpal::default_host();
    let Some(dev) = host.default_input_device() else {
        return Ok(None);
    };
    let name = dev.name().map_err(|e| AudioError::BadDeviceName(e.to_string()))?;
    let cfg = dev
        .default_input_config()
        .map_err(AudioError::DefaultConfig)?;
    Ok(Some((name, CaptureFormat::from_supported(&cfg))))
}

/// Tüm giriş cihazlarını adlarıyla listeler.
///
/// Varsayılan cihaz adından karşılaştırılarak `is_default` işaretlenir
/// (cpal `Device`'i `PartialEq` uygulamaz, isimle karşılaştırırız).
pub fn list_input_devices() -> Result<Vec<InputDevice>, AudioError> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());
    let devices = host
        .input_devices()
        .map_err(AudioError::IteratingDevices)?;

    let mut out = Vec::new();
    for device in devices {
        let name = device.name().unwrap_or_else(|_| "(adsız cihaz)".to_string());
        let native = device
            .default_input_config()
            .ok()
            .as_ref()
            .map(CaptureFormat::from_supported);
        let is_default = default_name.as_deref() == Some(name.as_str());
        out.push(InputDevice {
            name,
            is_default,
            native,
        });
    }
    Ok(out)
}

/// Adıyla bir girdi cihazını bulur; `None` bir cihaz yok demek değildir
/// (ad bulunamadı → `Err` değil, `Ok(None)`).
pub fn input_device_by_name(name: &str) -> Result<Option<cpal::Device>, AudioError> {
    let host = cpal::default_host();
    for device in host.input_devices().map_err(AudioError::IteratingDevices)? {
        if device.name().map(|n| n == name).unwrap_or(false) {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

/// Kayıt için kullanılacak cihazı seçer ve doğal biçimini döndürür.
///
/// `device_name` `None` ise sisteme ait varsayılan girdi cihazı kullanılır.
pub fn select_input_device(
    device_name: Option<&str>,
) -> Result<(cpal::Device, CaptureFormat), AudioError> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => input_device_by_name(name)?
            .ok_or_else(|| AudioError::DeviceNotAvailable(name.to_string()))?,
        None => host
            .default_input_device()
            .ok_or(AudioError::NoInputDevice)?,
    };
    let cfg = device
        .default_input_config()
        .map_err(AudioError::DefaultConfig)?;
    Ok((device, CaptureFormat::from_supported(&cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Varsayılan girdi biçimini gerçek donanımla raporlar (V8 doğrulaması).
    ///
    /// Donanım gerekir (mikrofon/girdi yolu) — bu yüzden `#[ignore]`. CI'da ve
    /// headless makinede cihaz yoksa zarifçe `Ok(None)` döner, hata olmaz.
    #[test]
    #[ignore]
    fn probe_default_input_config_lists_native_format() {
        match probe_default_input_config() {
            Ok(Some((name, fmt))) => {
                eprintln!("default girdi: name={name:?} fmt={fmt:?} direct_16k={}", fmt.is_direct_16k_mono());
                // 16k mono i16 doğrudan destekleniyorsa V8 kapandı demektir.
                eprintln!("V8 (direct 16k mono i16) = {}", fmt.is_direct_16k_mono());
            }
            Ok(None) => eprintln!("girdi cihazı yok (zarif yol)"),
            Err(e) => panic!("sondaj başarısız: {e}"),
        }
    }

    /// Tüm girdi cihazlarını gerçek donanımla listeler.
    #[test]
    #[ignore]
    fn list_input_devices_reports_formats() {
        let devices = list_input_devices().expect("cihaz listesi okunmalı");
        for d in &devices {
            eprintln!(
                "cihaz: name={:?} default={} direct_16k={} native={:?}",
                d.name,
                d.is_default,
                d.direct_16k_mono(),
                d.native
            );
        }
    }
}