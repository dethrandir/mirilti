//! Yeniden örnekleme ve kanal birleştirme (§6, Faz 2).
//!
//! Hedef biçim **16 kHz mono i16**. Cihaz doğal biçimini verdiğinde
//! doğrudan alırız; değilse `rubato` (`FftFixedIn`) ile yüksek kaliteli
//! yeniden örnekleme yapıp çok kanallıysa kanalları birleştiririz (downmix).
//! ffmpeg kullanılmaz.

use rubato::{FftFixedIn, Resampler};

use crate::audio::{AudioError, TARGET_CHANNELS, TARGET_SAMPLE_RATE};

/// Bir i16 örneği normalize edilmiş `f32`'ye çevirir (-1.0..1.0).
pub fn i16_to_f32(s: i16) -> f32 {
    s as f32 / 32768.0
}

/// Normalize `f32`'yi `i16`'ye çevirir; değer [-1,1]'e kenetlenir.
pub fn f32_to_i16(s: f32) -> i16 {
    let clamped = s.clamp(-1.0, 1.0);
    (clamped * 32767.0).round() as i16
}

/// interleaved `i16`'yi kanal-düzlemi (planar) `f32`'ye açar.
fn to_planar_f32(samples: &[i16], channels: u16) -> Vec<Vec<f32>> {
    let channels = channels as usize;
    let frames = samples.len() / channels.max(1);
    let mut planar = vec![Vec::with_capacity(frames); channels];
    for f in 0..frames {
        for ch in 0..channels {
            planar[ch].push(i16_to_f32(samples[f * channels + ch]));
        }
    }
    planar
}

/// Kanal-düzlemi `f32`'yi (tek kanallı varsayılır) `i16`'ye indirir.
fn from_planar_f32(mono: &[f32]) -> Vec<i16> {
    mono.iter().copied().map(f32_to_i16).collect()
}

/// Çok kanallı interleaved `i16`'yi tek kanala (mono) birleştirir.
/// Her çerçevedeki kanal ortalaması alınır.
pub fn downmix_to_mono(samples: &[i16], channels: u16) -> Vec<i16> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let channels = channels as usize;
    let frames = samples.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut sum: i64 = 0;
        for ch in 0..channels {
            sum += samples[f * channels + ch] as i64;
        }
        mono.push((sum / channels as i64) as i16);
    }
    mono
}

/// Interleaved stereo `i16`'yi mono'ya birleştirir (L+R)/2.
///
/// Birim test hedefi (§6): stereo → mono.
pub fn downmix_stereo_to_mono(samples: &[i16]) -> Vec<i16> {
    downmix_to_mono(samples, 2)
}

/// 48 kHz mono `i16`'yi 16 kHz mono `i16`'ye yeniden örnekler (rubato).
///
/// Birim test hedefi (§6): 48k → 16k.
pub fn resample_48k_to_16k(samples: &[i16]) -> Result<Vec<i16>, AudioError> {
    let planar_in = to_planar_f32(samples, 1);
    let (_, out) = fft_resample_planar(&planar_in, 48_000, TARGET_SAMPLE_RATE)
        .map_err(AudioError::Resample)?;
    Ok(from_planar_f32(&out[0]))
}

/// Girdi `i16`'yi (doğal biçim, interleaved) hedef **16 kHz mono i16**'ye
/// çevirir. Kanal sayısı 1 ve oran 16k ise doğrudan döner; aksi hâlde
/// önce downmix, sonra yeniden örnekleme yapar.
///
/// Kayıt hattının tek giriş noktası: [`crate::audio::recorder`].
pub fn to_mono_16k(
    samples: &[i16],
    channels: u16,
    sample_rate: u32,
) -> Result<Vec<i16>, AudioError> {
    if channels == TARGET_CHANNELS && sample_rate == TARGET_SAMPLE_RATE {
        return Ok(samples.to_vec());
    }
    let mono_i16 = downmix_to_mono(samples, channels);
    if sample_rate == TARGET_SAMPLE_RATE {
        return Ok(mono_i16);
    }
    let planar_in = to_planar_f32(&mono_i16, 1);
    let (_, out) = fft_resample_planar(&planar_in, sample_rate, TARGET_SAMPLE_RATE)
        .map_err(AudioError::Resample)?;
    Ok(from_planar_f32(&out[0]))
}

/// `rubato::FftFixedIn<f32>` ile temel yeniden örnekleme.
///
/// Planar `f32` girişinde her kanalı `in_rate` → `out_rate` ye taşır.
/// Sabit parçalar (chunk) hâlinde beslenir; sondaki eksik parça sıfırla
/// doldurulur ve kalan tampon boşaltılır (flush). Sonuç kanal-düzlemi `f32`.
///
/// Hata türü `String`'dir (kurulum ve işlem hatalarını birleştirir); çağıran
/// `AudioError::Resample`'e sarar.
pub fn fft_resample_planar(
    planar_in: &[Vec<f32>],
    in_rate: u32,
    out_rate: u32,
) -> Result<(usize, Vec<Vec<f32>>), String> {
    let channels = planar_in.len();
    if channels == 0 || planar_in[0].is_empty() {
        return Ok((0, vec![Vec::new(); channels]));
    }

    const CHUNK_IN: usize = 512;
    let mut resampler = FftFixedIn::<f32>::new(in_rate as usize, out_rate as usize, CHUNK_IN, 1, channels)
        .map_err(|e| e.to_string())?;
    let step = resampler.input_frames_next();
    let n_in = planar_in[0].len();

    let mut out: Vec<Vec<f32>> = vec![Vec::new(); channels];
    let mut pos = 0usize;
    while pos < n_in {
        let take = (n_in - pos).min(step);
        let mut chunk: Vec<Vec<f32>> = Vec::with_capacity(channels);
        for ch in planar_in {
            let mut buf = vec![0.0f32; step];
            buf[..take].copy_from_slice(&ch[pos..pos + take]);
            chunk.push(buf);
        }
        let res = resampler.process(&chunk, None).map_err(|e| e.to_string())?;
        for (o, r) in out.iter_mut().zip(res) {
            o.extend(r);
        }
        pos += take;
    }

    let ratio_out_len = ((n_in as f64 * out_rate as f64 / in_rate as f64).round()) as usize;
    let target_len = ratio_out_len.max(1);

    // Overlap-add kuyruğunu ve `saved_frames` içinde kalan son parçayı boşalt.
    //
    // `FftFixedIn` için `loop { process_partial(None, None) ... is_empty kısmına
    // kadar }` SONS UZAR: her çağrı `chunk_size_in` sıfır enjekte eder ve çıktı
    // uzunluğu `(saved + chunk) / fft_size_in` şeklinde hesaplanır; `saved` gerçek
    // girdiden (513 'lük FFT parçasına tam bölünmeyen) artık değeri 512'ye
    // dönünce 171/0 ile sonsuza kadar salınır → hiçbir zaman tümü boş olmaz.
    //
    // Doğru yol: kuramsal uzunluğa erişene dek sınırlı sayıda boşaltma yap,
    // sonra çıktıyı hedef uzunluğa kırp. `process_partial(None, ...)` kalan
    // gerçek örnekleri (sıfırlarla birlikte) tek FFT parçası olarak işleyip
    // overlap tail'i dışarı atar; fazlalık sıfırlar `truncate` ile atılır.
    while out[0].len() < target_len {
        let res = resampler
            .process_partial::<Vec<f32>>(None, None)
            .map_err(|e| e.to_string())?;
        for (o, r) in out.iter_mut().zip(res) {
            o.extend(r);
        }
    }
    // Fazla üretilen (sıfır dolgulu) örnekleri hedef boyuta kırp.
    for o in out.iter_mut() {
        o.truncate(target_len);
    }

    Ok((channels, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sabit genlikli sinüs üretir.
    fn sine(amp: i16, freq: f32, rate: u32, frames: usize) -> Vec<i16> {
        (0..frames)
            .map(|i| (amp as f32 * (2.0 * core::f32::consts::PI * freq * i as f32 / rate as f32).sin()) as i16)
            .collect()
    }

    /// Tane sıfır geçişinden sinyal frekansını yaklaşık tahmin eder.
    fn estimate_freq(samples: &[i16], rate: u32) -> f64 {
        let mut crossings = 0usize;
        for w in samples.windows(2) {
            if (w[0] < 0) != (w[1] < 0) {
                crossings += 1;
            }
        }
        let frames = samples.len().max(1) as f64;
        crossings as f64 * rate as f64 / (2.0 * frames)
    }

    #[test]
    fn stereo_downmix_averages_channels() {
        // L = 1000 sabit, R = 2000 sabit → mono 1500.
        let src = [1000i16, 2000, 1000, 2000, 1000, 2000];
        let mono = downmix_stereo_to_mono(&src);
        assert_eq!(mono, vec![1500, 1500, 1500]);
    }

    #[test]
    fn stereo_downmix_handles_extremes() {
        // (32767 + 32767)/2 = 32767 ; (32767 + -32768)/2 → -1/2 → 0 (trunc).
        let src = [32767i16, 32767, 32767, -32768, -32768, -32768];
        let mono = downmix_stereo_to_mono(&src);
        assert_eq!(mono, vec![32767, 0, -32768]);
    }

    #[test]
    fn downmix_mono_is_identity() {
        let src = [5i16, -9, 300, -299];
        assert_eq!(downmix_to_mono(&src, 1), src.to_vec());
    }

    #[test]
    fn direct_to_mono_16k_when_already_16k_mono() {
        let src = sine(8000, 440.0, 16_000, 512);
        let out = to_mono_16k(&src, 1, 16_000).unwrap();
        assert_eq!(out, src);
    }

    #[test]
    fn resample_48k_to_16k_preserves_tone_frequency() {
        // 1 kHz ton 48 kHz'te üret, 16 kHz'e düşür — frekans korunmalı.
        let src = sine(12000, 1000.0, 48_000, 48_000); // 1 saniye
        let dst = resample_48k_to_16k(&src).unwrap();
        // 1 sn 48k → 16k ≈ 16.000 örnek (parça sınırına göre küçük sapma).
        let ratio = dst.len() as f64 / 48_000.0;
        assert!((0.30..=0.36).contains(&ratio), "oran beklenenden uzak: {ratio}");

        let f = estimate_freq(&dst[160..dst.len() - 160], 16_000);
        assert!((950.0..=1050.0).contains(&f), "frekans korunmadı: {f}");
    }

    #[test]
    fn resample_48k_to_16k_preserves_amplitude() {
        // DC ofset (sabit 8000) — yeniden örneklemenin büyütülmesi olmamalı.
        let src = vec![8000i16; 10_240]; // 512 * 20 örnek (~213 ms)
        let dst = resample_48k_to_16k(&src).unwrap();
        assert!(!dst.is_empty());
        // Ortadaki %90 kısmında tümü ~8000 olmalı (FFT kenar etkisinden kaçın).
        let lo = dst.len() / 20;
        let hi = dst.len() - lo;
        let mid = &dst[lo..hi];
        let avg_dev = mid.iter().map(|&s| (s - 8000).abs() as f64).sum::<f64>() / mid.len() as f64;
        assert!(avg_dev < 60.0, "genlik sapması fazla: {avg_dev}");
    }

    #[test]
    fn resample_length_is_approximate() {
        // 5120 örnek 48k → FFT parçası başına ~171 çıkış → ~1710 toplam.
        let src = sine(5000, 300.0, 48_000, 5120);
        let dst = resample_48k_to_16k(&src).unwrap();
        let expected = 5120.0 * 16_000.0 / 48_000.0; // ≈ 1706.7
        let diff = (dst.len() as f64 - expected).abs();
        assert!(diff < 20.0, "uzunluk sapması fazla: {}", diff);
    }

    #[test]
    fn to_mono_16k_downmix_then_resample() {
        // Stereo 48k → mono 16k.
        let n = 9600usize;
        let l = sine(10000, 500.0, 48_000, n);
        let r = sine(10000, 500.0, 48_000, n);
        let mut inter = Vec::with_capacity(n * 2);
        for i in 0..n {
            inter.push(l[i]);
            inter.push(r[i]);
        }
        let mono = to_mono_16k(&inter, 2, 48_000).unwrap();
        assert!(!mono.is_empty());
        let ratio = mono.len() as f64 / n as f64;
        assert!((0.30..=0.36).contains(&ratio), "oran: {ratio}");
        let f = estimate_freq(&mono[100..], 16_000);
        assert!((450.0..=560.0).contains(&f), "frekans korunmadı: {f}");
    }
}