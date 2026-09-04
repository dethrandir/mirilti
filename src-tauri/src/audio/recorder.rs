//! Kayıt döngüsü (§6, Faz 2).
//!
//! `cpal` ham (raw) girdi akışını tip-bilinmeyen `Data` ile dinler: gelen
//! örnek biçimi çalışma zamanında çözülür ve `i16`'ye çevrilir, doğal oran/
//! kanalda biriktirilir. `rubato` ile hedef 16 kHz mono i16'ya indirilir.
//!
//! **İş parçacığı modeli:** cpal `Stream`'i `!Send`'dir (Android/AAudio
//! gerekçesiyle `PhantomData<*mut ()>` taşır). Bu yüzden akışı kurmak ve onu
//! hayatta tutmak zorunda olan sürücü döngüsü tek bir iş parçacığında yaşar;
//! cpal'in kendi callback iş parçacığı örnekleri toplar, ayrı bir bekleyen
//! iş parçacığı stop/cancel/max-süreyi yönetir. `start()` tokio'yu bloklamaz,
//! yalnızca akış "hazır" olana kadar (birkaç ms) bekler ve anında döner.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use serde::Serialize;

use crate::audio::device;
use crate::audio::resample;
use crate::audio::{AudioError, TARGET_SAMPLE_RATE};

/// Varsayılan kayıt sınırları (PLAN §6).
pub const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(300); // 5 dk
/// Varsayılan sessizlik eşiği: RMS (0..1 ölçek). Bunun altında kalındıysa
/// kayıt "ses algılanmadı" sayılır. 0.01 ≈ −40 dBFS (sessiz oda).
pub const DEFAULT_SILENCE_THRESHOLD: f32 = 0.01;

/// Kayıt başlatma seçenekleri.
#[derive(Debug, Clone)]
pub struct RecorderOptions {
    /// Kullanılacak cihaz adı; `None` = sistem varsayılanını kullan.
    pub device_name: Option<String>,
    /// En fazla kayıt süresi; aşılınca otomatik durdurulur (normal akışa dönüş).
    pub max_duration: Duration,
    /// Sessizlik eşiği (RMS, 0..1). Kayıt boyunca bu aşılmadıysa sessiz görülür.
    pub silence_threshold: f32,
}

impl Default for RecorderOptions {
    fn default() -> Self {
        Self {
            device_name: None,
            max_duration: DEFAULT_MAX_DURATION,
            silence_threshold: DEFAULT_SILENCE_THRESHOLD,
        }
    }
}

/// Canlı seviye göstergesi (overlay'e gönderilir).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RmsLevel {
    /// En son callback diliminin RMS'i, 0..1 ölçek.
    pub level: f32,
    /// Kayıt boyunca görülen en yüksek RMS.
    pub peak: f32,
}

/// Kayıt sürücüsü; `start` → `stop`/`cancel` yaşam döngüsü.
///
/// Akış sürücü iş parçacığında yaşar (bkz. modül dokümanı); bu yapı yalnızca
/// komut/sonuç kanallarını ve canlı seviye için paylaşılan durumu tutar.
pub struct Recorder {
    cmd_tx: Option<Sender<Command>>,
    out_rx: Option<Receiver<Outcome>>,
    capture: Arc<Mutex<Capture>>,
    active: bool,
}

/// Sürücüye gönderilen komutlar.
enum Command {
    Stop,
    Cancel,
}

/// Sürücüden dönen sonuç.
enum Outcome {
    Recording(Box<Recording>),
    Canceled,
}

/// Callback ve sürücünün paylaştığı birikim alanı.
struct Capture {
    /// Doğal oran/kanalda biriktirilmiş interleaved i16 örnekler.
    native: Vec<i16>,
    channels: u16,
    sample_rate: u32,
    /// Son callback RMS'i (0..1).
    level: f32,
    /// Görülen en yüksek RMS.
    peak_rms: f32,
    /// RMS kareler toplamı (ort. için).
    sum_sq: f64,
    frame_count: u64,
}

impl Capture {
    fn new(channels: u16, sample_rate: u32) -> Self {
        Self {
            native: Vec::new(),
            channels,
            sample_rate,
            level: 0.0,
            peak_rms: 0.0,
            sum_sq: 0.0,
            frame_count: 0,
        }
    }

    /// Kayıt geneline ait ortalama RMS (0..1).
    fn average_rms(&self) -> f32 {
        if self.frame_count == 0 {
            return 0.0;
        }
        (self.sum_sq / self.frame_count as f64).sqrt() as f32
    }
}

/// Tamamlanmış bir kaydın sonucu.
#[derive(Debug, Clone, Serialize)]
pub struct Recording {
    /// 16 kHz mono i16 örnekler (whisper'a doğrudan verilir).
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub duration: Duration,
    /// Kayıt boyunca görülen en yüksek RMS (0..1).
    pub peak_rms: f32,
    pub average_rms: f32,
    /// `true` → eşik hiç aşılmadı, "ses algılanmadı".
    pub silence_detected: bool,
    /// `true` → maksimum süre aşılınca otomatik durduruldu.
    pub auto_stopped: bool,
}

impl Recorder {
    /// Kaydı başlatır; cihaz seçimi + akış kurulumu + sürücü iş parçacığı.
    ///
    /// `start` tokio'yu bloklamaz: akış hazır olana kadar (~birkaç ms)
    /// bekleyip anında döner. Sonuç `stop`/`cancel` ile alınır.
    pub fn start(opts: RecorderOptions) -> Result<Self, AudioError> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let (out_tx, out_rx) = mpsc::channel::<Outcome>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), AudioError>>();
        let capture = Arc::new(Mutex::new(Capture::new(1, TARGET_SAMPLE_RATE)));
        let cap_worker = Arc::clone(&capture);

        std::thread::Builder::new()
            .name("mirilti-audio".into())
            .spawn(move || {
                worker_main(opts, cap_worker, cmd_rx, &out_tx, &ready_tx);
            })
            .map_err(|e| AudioError::Recording(format!("sürücü iş parçacığı: {e}")))?;

        // Akışın kurulup `play()` edildiğini (veya hatayı) bekleriz.
        ready_rx
            .recv()
            .map_err(|_| AudioError::Recording("sürücü başlamadı".into()))??;

        Ok(Self {
            cmd_tx: Some(cmd_tx),
            out_rx: Some(out_rx),
            capture,
            active: true,
        })
    }

    /// Kaydı durdurur ve işlenmiş 16 kHz mono sonucu döndürür (bloklayıcı).
    pub fn stop(&mut self) -> Result<Recording, AudioError> {
        if !self.active {
            return Err(AudioError::Recording("kayıt aktif değil".into()));
        }
        let Some(tx) = self.cmd_tx.take() else {
            return Err(AudioError::Recording("komut kanalı eksik".into()));
        };
        let _ = tx.send(Command::Stop);
        self.active = false;
        match self
            .out_rx
            .take()
            .ok_or_else(|| AudioError::Recording("sonuç kanalı eksik".into()))?
            .recv()
        {
            Ok(Outcome::Recording(r)) => Ok(*r),
            Ok(Outcome::Canceled) => Err(AudioError::Recording("kayıt iptal edildi".into())),
            Err(_) => Err(AudioError::Recording("sürücü sonlandı".into())),
        }
    }

    /// Kaydı iptal eder; toplanan ses atılır (Esc akışı, PLAN §3.2).
    pub fn cancel(&mut self) -> Result<(), AudioError> {
        if !self.active {
            return Err(AudioError::Recording("kayıt aktif değil".into()));
        }
        let Some(tx) = self.cmd_tx.take() else {
            return Err(AudioError::Recording("komut kanalı eksik".into()));
        };
        let _ = tx.send(Command::Cancel);
        self.active = false;
        let _ = self
            .out_rx
            .take()
            .ok_or_else(|| AudioError::Recording("sonuç kanalı eksik".into()))?
            .recv();
        Ok(())
    }

    /// Anlık seviye (overlay / canlı gösterge, bloklamaz).
    pub fn current_level(&self) -> RmsLevel {
        let c = self
            .capture
            .lock()
            .map(|g| (g.level, g.peak_rms))
            .unwrap_or((0.0, 0.0));
        RmsLevel {
            level: c.0,
            peak: c.1,
        }
    }
}

/// Sürücü ana işlevi (iş parçacığında çalışır, akışı kurar ve yönetir).
///
/// 1. Cihaz/biçim çözülür, akış kurulur ve `play()` edilir.
/// 2. `ready` kanalına `Ok(())` (ya da kurulum hatası `Err`) gönderilir.
/// 3. Stop / Cancel / max-süre beklenir; sonuca göre kayıt finalize edilir.
/// 4. Sonuç `out` kanalına gönderilir.
///
/// `stream` `!Send` olduğu için bu fonksiyonun dışına asla taşınmaz.
fn worker_main(
    opts: RecorderOptions,
    capture: Arc<Mutex<Capture>>,
    cmd_rx: Receiver<Command>,
    out_tx: &Sender<Outcome>,
    ready_tx: &Sender<Result<(), AudioError>>,
) {
    let stream = match build_stream(&opts, &capture) {
        Ok(s) => s,
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));

    let outcome = match cmd_rx.recv_timeout(opts.max_duration) {
        Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = stream.pause();
            build_outcome(&capture, opts.silence_threshold, false)
        }
        Ok(Command::Cancel) => {
            let _ = stream.pause();
            Outcome::Canceled
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Maksimum süre aşıldı → otomatik durdur (normal akışa dönüş).
            let _ = stream.pause();
            build_outcome(&capture, opts.silence_threshold, true)
        }
    };
    let _ = out_tx.send(outcome);
}

/// Akışı kurar ve başlatır (cihaz çözme + raw akış + `play`).
fn build_stream(
    opts: &RecorderOptions,
    capture: &Arc<Mutex<Capture>>,
) -> Result<cpal::Stream, AudioError> {
    let (device, _fmt) = device::select_input_device(opts.device_name.as_deref())?;
    let config = device
        .default_input_config()
        .map_err(AudioError::DefaultConfig)?;

    let channels = config.channels();
    let sample_rate = config.sample_rate().0;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.config();

    {
        let mut c = capture.lock().unwrap();
        c.channels = channels;
        c.sample_rate = sample_rate;
    }

    let cap_cb = Arc::clone(capture);
    let data_cb = move |data: &cpal::Data, _info: &cpal::InputCallbackInfo| {
        let Some(buf) = native_to_i16(data) else {
            return;
        };
        let mut c = match cap_cb.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let rms = mono_rms_i16(&buf, channels);
        c.level = rms;
        if rms > c.peak_rms {
            c.peak_rms = rms;
        }
        let frames = (buf.len() / channels.max(1) as usize).max(1);
        c.sum_sq += (rms as f64) * (rms as f64) * frames as f64;
        c.frame_count += frames as u64;
        c.native.extend_from_slice(&buf);
    };
    let err_cb = |e: cpal::StreamError| eprintln!("[audio] akış hatası: {e}");

    let stream = device
        .build_input_stream_raw(&stream_config, sample_format, data_cb, err_cb, None)
        .map_err(AudioError::BuildStreamError)?;
    stream.play().map_err(AudioError::PlayStreamError)?;
    Ok(stream)
}

/// Birikmiş ham örnekleri 16 kHz mono i16'ya indirip bir [`Recording`] üretir.
fn build_outcome(
    capture: &Arc<Mutex<Capture>>,
    threshold: f32,
    auto_stopped: bool,
) -> Outcome {
    let c = capture.lock().unwrap();
    let native = c.native.clone();
    let channels = c.channels;
    let rate = c.sample_rate;
    let peak = c.peak_rms;
    let avg = c.average_rms();
    drop(c);

    let samples = match resample::to_mono_16k(&native, channels, rate) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[audio] to_mono_16k hatası: {e}");
            Vec::new()
        }
    };
    let duration = Duration::from_secs_f64(samples.len() as f64 / TARGET_SAMPLE_RATE as f64);
    let silence_detected = peak < threshold;
    Outcome::Recording(Box::new(Recording {
        samples,
        sample_rate: TARGET_SAMPLE_RATE,
        duration,
        peak_rms: peak,
        average_rms: avg,
        silence_detected,
        auto_stopped,
    }))
}

/// Gelen tip-bilinmeyen `Data`'yı interleaved `i16`'ye çevirir.
///
/// Cpal doğal biçimi çalışma zamanında gelir ([`cpal::SampleFormat`]); her
/// varyant için `as_slice::<T>()` ile güvenli erişip normalize `i16`'ye çeviririz.
fn native_to_i16(data: &cpal::Data) -> Option<Vec<i16>> {
    use cpal::SampleFormat;
    match data.sample_format() {
        SampleFormat::F32 => {
            let s = data.as_slice::<f32>()?;
            Some(s.iter().map(|&v| resample::f32_to_i16(v)).collect())
        }
        SampleFormat::F64 => {
            let s = data.as_slice::<f64>()?;
            Some(s.iter().map(|&v| resample::f32_to_i16(v as f32)).collect())
        }
        SampleFormat::I16 => Some(data.as_slice::<i16>()?.to_vec()),
        SampleFormat::U16 => {
            let s = data.as_slice::<u16>()?;
            Some(s.iter().map(|&v| (v as i32 - 32768) as i16).collect())
        }
        SampleFormat::I32 => {
            let s = data.as_slice::<i32>()?;
            Some(s.iter().map(|&v| (v >> 16) as i16).collect())
        }
        SampleFormat::U32 => {
            let s = data.as_slice::<u32>()?;
            Some(
                s.iter()
                    .map(|&v| ((v as i64 >> 16) - 32768).clamp(-32768, 32767) as i16)
                    .collect(),
            )
        }
        SampleFormat::I64 => {
            let s = data.as_slice::<i64>()?;
            Some(s.iter().map(|&v| (v >> 48) as i16).collect())
        }
        SampleFormat::U64 => {
            let s = data.as_slice::<u64>()?;
            Some(
                s.iter()
                    .map(|&v| ((v as i128 >> 48) - 32768).clamp(-32768, 32767) as i16)
                    .collect(),
            )
        }
        SampleFormat::I8 => {
            let s = data.as_slice::<i8>()?;
            Some(s.iter().map(|&v| (v as i16) * 256).collect())
        }
        SampleFormat::U8 => {
            let s = data.as_slice::<u8>()?;
            Some(s.iter().map(|&v| ((v as i16 - 128) as i32 * 256) as i16).collect())
        }
        // cpal `SampleFormat` non-exhaustive; gelecekteki biçimler desteklenmezse
        // bu dilim yok sayılır (hata değil, zarif düşme).
        _ => None,
    }
}

/// Interleaved `i16` diliminin mono-ortalanmış RMS'ini (0..1) hesaplar.
///
/// Her çerçevede kanallar ortalanır, karesel ortalama alınır, `32768`'e
/// bölünür. Hem canlı seviye (callback) hem sessizlik eşiği için kullanılır.
fn mono_rms_i16(samples: &[i16], channels: u16) -> f32 {
    let channels = channels.max(1) as usize;
    let frames = samples.len() / channels;
    if frames == 0 {
        return 0.0;
    }
    let mut sum_sq = 0f64;
    for f in 0..frames {
        let mut acc: i64 = 0;
        for ch in 0..channels {
            acc += samples[f * channels + ch] as i64;
        }
        let mono = acc as f64 / channels as f64;
        let norm = mono / 32768.0;
        sum_sq += norm * norm;
    }
    (sum_sq / frames as f64).sqrt() as f32
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(mono_rms_i16(&[0i16; 100], 1), 0.0);
    }

    #[test]
    fn rms_of_full_scale_is_one() {
        // Tam genlik kare dalga → RMS ≈ 1.0.
        let mut s = Vec::new();
        for i in 0..1000 {
            s.push(if i % 2 == 0 { 32767 } else { -32768 });
        }
        let rms = mono_rms_i16(&s, 1);
        assert!((rms - 1.0).abs() < 0.02, "rms={rms}");
    }

    #[test]
    fn rms_stereo_averages_channels() {
        // L tam genlik, R ters faza tam genlik → mono ortalaması ≈ 0 → rms ≈ 0.
        let mut s = Vec::new();
        for i in 0..2000 {
            let l = if i % 2 == 0 { 32767 } else { -32768 };
            let r = if i % 2 == 0 { 32767 } else { -32768 }; // aynı faz → güçlenir
            s.push(l);
            s.push(r);
        }
        let rms = mono_rms_i16(&s, 2);
        assert!((rms - 1.0).abs() < 0.02, "rms={rms}");
    }

    #[test]
    fn level_below_threshold_reports_silence() {
        let quiet = vec![100i16; 500]; // 100/32768 ≈ 0.003 < 0.01
        let rms = mono_rms_i16(&quiet, 1);
        assert!(rms < DEFAULT_SILENCE_THRESHOLD);
        assert!(rms > 0.0);
    }

    #[test]
    fn level_above_threshold_is_loud() {
        let loud = vec![12000i16; 500]; // ≈ 0.37 > 0.01
        let rms = mono_rms_i16(&loud, 1);
        assert!(rms > DEFAULT_SILENCE_THRESHOLD);
    }
}

/// Gerçek donanım kayıt testi — varsayılan cihazla kısa bir kayıt alır.
///
/// Ayrıca akışın açılıp kapanabildiğini doğrular. `#[ignore]` (donanım gerekir).
#[cfg(test)]
mod hardware {
    use super::*;

    #[test]
    #[ignore]
    fn start_stop_short_capture() {
        let mut rec = Recorder::start(RecorderOptions {
            device_name: None,
            max_duration: Duration::from_secs(2),
            ..Default::default()
        })
        .expect("kayıt başlamalı");

        std::thread::sleep(Duration::from_millis(300));
        let r = rec.stop().expect("kayıt durmalı");
        eprintln!(
            "kayıt: durum={} sure_ms={:?} peak_rms={} avg_rms={} ornek={}",
            if r.silence_detected {
                "ses yok"
            } else {
                "ses var"
            },
            r.duration,
            r.peak_rms,
            r.average_rms,
            r.samples.len()
        );
    }
}