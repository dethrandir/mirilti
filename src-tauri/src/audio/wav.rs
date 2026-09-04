//! WAV yazma/okuma ve `pending/` yazma (§6, Faz 2).
//!
//! `hound` ile 16 kHz mono i16 WAV üretiriz; dosyaya ya da bellek içi byte
//! tamponuna yazabiliriz. Ham WAV, işlenene kadar XDG data dizininde
//! `pending/` altında tutulur; işlem başarılı olunca temizleme yardımcısı var.

use std::path::{Path, PathBuf};

use hound::{WavSpec, WavWriter};

use crate::audio::{AudioError, TARGET_BITS, TARGET_CHANNELS, TARGET_SAMPLE_RATE};

/// 16 kHz mono i16 için standart WAV spec.
pub fn mono_16k_spec() -> WavSpec {
    WavSpec {
        channels: TARGET_CHANNELS,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: TARGET_BITS,
        sample_format: hound::SampleFormat::Int,
    }
}

/// `path`'e 16 kHz mono i16 WAV yazar.
pub fn write_wav_i16_mono(
    path: &Path,
    samples: &[i16],
    sample_rate: u32,
) -> Result<(), AudioError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: TARGET_BITS,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).map_err(|e| AudioError::Wav(e.to_string()))?;
    write_samples(&mut writer, samples).map_err(|e| AudioError::Wav(e.to_string()))?;
    writer
        .finalize()
        .map_err(|e| AudioError::Wav(e.to_string()))
}

/// 16 kHz mono i16 WAV'ı bellek içi byte tamponuna yazar.
///
/// `hound` 3.5 alttaki yazıcıyı geri vermediği için geçici bir dosyaya yazıp
/// bytes'ı geri okuruz; dosya yazma yolunu da yeniden kullanır.
pub fn wav_bytes_i16_mono(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, AudioError> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir().join(format!("mirilti-wav-{stamp}.wav"));
    write_wav_i16_mono(&path, samples, sample_rate)?;
    let bytes = std::fs::read(&path).map_err(AudioError::Io)?;
    let _ = std::fs::remove_file(&path);
    Ok(bytes)
}

fn write_samples<W: std::io::Write + std::io::Seek>(
    writer: &mut WavWriter<W>,
    samples: &[i16],
) -> Result<(), hound::Error> {
    for &s in samples {
        writer.write_sample(s)?;
    }
    Ok(())
}

/// 16 kHz mono i16 WAV okur; `(örnek_oranı, kanal, örnekler)` döndürür
/// (test ve "yeniden işle" akışı için).
pub fn read_wav_i16_mono(path: &Path) -> Result<(u32, u16, Vec<i16>), AudioError> {
    let mut reader = hound::WavReader::open(path).map_err(|e| AudioError::Wav(e.to_string()))?;
    let spec = reader.spec();
    let rate = spec.sample_rate;
    let channels = spec.channels;
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<i16>, _>>()
        .map_err(|e| AudioError::Wav(e.to_string()))?;
    Ok((rate, channels, samples))
}

/// XDG veri dizininde `pending/` klasörünü döndürür ve oluşturur.
///
/// Yol: `$XDG_DATA_HOME/mirilti/pending` (veya `~/.local/share/mirilti/pending`).
pub fn pending_dir() -> Result<PathBuf, AudioError> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into());
            PathBuf::from(home).join(".local").join("share")
        });
    let dir = base.join("mirilti").join("pending");
    std::fs::create_dir_all(&dir).map_err(AudioError::Io)?;
    Ok(dir)
}

/// `pending/` altına benzersiz adlı bir WAV yazar; dosya yolunu döndürür.
///
/// İşlenmemiş ses burada yaşar (İlke 2: ses asla kaybolmaz). Ad, zaman damgalı.
pub fn write_pending_wav(samples: &[i16], sample_rate: u32) -> Result<PathBuf, AudioError> {
    let dir = pending_dir()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let path = dir.join(format!("rec-{stamp}.wav"));
    write_wav_i16_mono(&path, samples, sample_rate)?;
    Ok(path)
}

/// Başarıyla işlenmiş bir `pending/` WAV dosyasını temizler.
///
/// Dosya yoksa sessizce başarılı sayılır; `pending_dir` dışındaki yollara
/// dokunmaz (güvenlik: yalnızca `pending/` altını silmeye izin var).
pub fn remove_pending(path: &Path) -> Result<(), AudioError> {
    let pending = pending_dir()?;
    if !path.starts_with(&pending) {
        return Err(AudioError::Wav(format!(
            "yol pending/ altında değil, silinmedi: {}",
            path.display()
        )));
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AudioError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_roundtrip() {
        let dir = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = dir.join(format!("mirilti-wav-test-{stamp}.wav"));

        let samples: Vec<i16> = (0..1000).map(|i| (i as i16) * 10).collect();
        write_wav_i16_mono(&path, &samples, 16_000).expect("yazma başarılı");
        let (rate, channels, read) = read_wav_i16_mono(&path).expect("okuma başarılı");

        assert_eq!(rate, 16_000);
        assert_eq!(channels, 1);
        assert_eq!(read.len(), samples.len());
        for (a, b) in read.iter().zip(&samples) {
            assert_eq!(a, b);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wav_bytes_parses_back() {
        let samples = vec![0i16, 1000, -1000, 32767, -32768];
        let bytes = wav_bytes_i16_mono(&samples, 16_000).expect("bytes yazma");
        assert!(bytes.len() > 44); // RIFF + data en azından
        let mut reader =
            hound::WavReader::new(std::io::Cursor::new(bytes)).expect("parse");
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.channels, 1);
        let read: Vec<i16> = reader
            .samples::<i16>()
            .collect::<Result<_, _>>()
            .expect("örnekler");
        assert_eq!(read, samples);
    }

    #[test]
    fn remove_pending_guards_outsiders() {
        let dir = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        // pending/ dışı bir yol reddedilmeli.
        let outside = dir.join(format!("mirilti-out-{stamp}.wav"));
        assert!(remove_pending(&outside).is_err());

        // pending/ içinde ama mevcut olmayan dosya sessizce Ok.
        let pend = pending_dir().expect("pending klasörü");
        let missing = pend.join(format!("missing-{stamp}.wav"));
        assert!(remove_pending(&missing).is_ok());
    }

    #[test]
    fn write_pending_produces_readable_wav() {
        let samples = vec![500i16, -500, 300, -300];
        let path = write_pending_wav(&samples, 16_000).expect("pending yazma");
        let (rate, channels, read) = read_wav_i16_mono(&path).expect("okuma");
        assert_eq!(rate, 16_000);
        assert_eq!(channels, 1);
        assert_eq!(read, samples);
        remove_pending(&path).expect("temizlik");
    }
}