//! Pano katmanı (PLAN.md §8.2).
//!
//! Gerçek uygulama Faz 7: Tauri clipboard plugin'i (pano sahipliği/yükleme,
//! "panoyu koru" V9). Bu Faz 1c spike'ında bağımlılık eklemeden, mevcut CLI
//! yardımcılarını (`wl-copy`/`xclip`) dış süreç olarak çağıran hafif bir
//! backend soyutlaması koyuyoruz — burada amaç §8.1'in "önce panoyu doldur,
//! sonra Ctrl+V gönder" akışını **gerçekten** çalıştırabilmek; kalıcı pano
//! stratejisi Faz 7'nin işi.
//!
//! # Platform
//! - Wayland: `wl-copy` / `wl-paste` (KDE + wlroots kompozitörleri).
//! - X11: `xclip -selection clipboard`.
//! - İkisi de yoksa `ClipboardImpl::None` — §8.1 adım 4 (yalnızca pano) da kapanır.
//!
//! # Neden `#![allow(dead_code)]`
//! Spike modülü: `ClipboardBackend::read` henüz üretim akışından çağrılmıyor (Faz 7
//! "panoyu koru" V9 orada gelir). `clippy -D warnings` altında kırılmaması için modül
//! seviyesinde izin verilir; Faz 7'de tüketici yazılınca kaldırılabilir.
#![allow(dead_code)]

/// Panoya yazma/okuma soyutlaması. Faz 7'de aynı kontratla Tauri plugin'i
/// `SystemClipboard`'un yerini alır.
pub trait ClipboardBackend {
    /// Metni panoya yazar.
    fn write(&self, text: &str) -> Result<(), String>;
    /// Panodaki metni okur (en iyi çaba; Faz 7'de doğrulanır, V9).
    fn read(&self) -> Result<String, String>;
}

/// Hangi pano arka ucunun algılandığı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardImpl {
    /// Hiçbir CLI yardımcısı yok.
    None,
    /// Wayland (`wl-copy` / `wl-paste`).
    WlCopy,
    /// X11 (`xclip`).
    XClip,
}

impl ClipboardImpl {
    /// İnsan okur ad (Sistem Sağlığı ekranı / §8.1 zincir raporu için).
    pub fn label(self) -> &'static str {
        match self {
            ClipboardImpl::None => "yok",
            ClipboardImpl::WlCopy => "wl-copy",
            ClipboardImpl::XClip => "xclip",
        }
    }
}

/// Ortam bağımsız pano algılama: asla panic etmez, yalnızca bool/None döner.
pub fn detect_clipboard() -> ClipboardImpl {
    if command_exists("wl-copy") {
        ClipboardImpl::WlCopy
    } else if command_exists("xclip") {
        ClipboardImpl::XClip
    } else {
        ClipboardImpl::None
    }
}

/// CLI yardımcısını dış süreç olarak kullanan pano backend'i.
#[derive(Debug)]
pub struct SystemClipboard {
    backend: ClipboardImpl,
}

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemClipboard {
    /// Algılanan backend ile boş bir pano backend'i kurar.
    pub fn new() -> Self {
        Self {
            backend: detect_clipboard(),
        }
    }

    /// Bu örneğin kullandığı pano arka ucu.
    pub fn backend(&self) -> ClipboardImpl {
        self.backend
    }
}

impl ClipboardBackend for SystemClipboard {
    fn write(&self, text: &str) -> Result<(), String> {
        match self.backend {
            ClipboardImpl::WlCopy => run_status("wl-copy", &[], text),
            ClipboardImpl::XClip => run_status("xclip", &["-selection", "clipboard"], text),
            ClipboardImpl::None => Err(
                "pano CLI yardımcısı bulunamadı (wl-copy/xclip). Kurulum için §11/a §8.1'e bakın."
                    .to_string(),
            ),
        }
    }

    fn read(&self) -> Result<String, String> {
        let bytes = match self.backend {
            ClipboardImpl::WlCopy => run_capture("wl-paste", &[])?,
            ClipboardImpl::XClip => run_capture("xclip", &["-selection", "clipboard", "-o"])?,
            ClipboardImpl::None => {
                return Err("pano CLI yardımcısı bulunamadı (wl-paste/xclip).".to_string());
            }
        };
        String::from_utf8(bytes).map_err(|e| format!("pano içeriği geçerli UTF-8 değil: {e}"))
    }
}

/// Bir komutun PATH'te olup olmadığını söyler. Komut çalıştırılamazsa `false`.
pub(crate) fn command_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Veriyi stdin'den besleyip komutu çalıştırır; yalnızca exit durumunu döner.
///
/// `stdout`/`stderr` **null'a** yönlendirilir — önemli: `wl-copy` varsayılan
/// olarak bir daemon çocuk süreceği çatallar; o çocuk bizim pipe'ları `inherit`
/// etseydi `wait_with_output` EOF bekleyip sonsuza dek bloke olurdu. Null ile bu
/// çıkmaz önlenir (`wait()` doğrudan çocuk bitince döner). Komut bulunamazsa
/// zarif `Err` — panic yok.
fn run_status(prog: &str, args: &[&str], stdin_data: &str) -> Result<(), String> {
    use std::io::Write;

    let mut cmd = std::process::Command::new(prog);
    cmd.args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{prog} çalıştırılamadı: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(stdin_data.as_bytes())
            .map_err(|e| format!("{prog} stdin'e yazılamadı: {e}"))?;
    }
    let status = child
        .wait()
        .map_err(|e| format!("{prog} beklenemedi: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{prog} exit {:?}", status.code()))
    }
}

/// Dış süreci çalıştırır; stdout'u yakalar (oku). Süreç çalıştırılamaz ya da
/// sıfır olmayan kodla biterse zarif `Err(String)` döner — panic yok.
fn run_capture(prog: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new(prog)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("{prog} çalıştırılamadı: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let detail = if err.trim().is_empty() {
            format!("exit {:?}", out.status.code())
        } else {
            err.into_owned()
        };
        Err(format!("{prog}: {detail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `detect_clipboard` her ortamda bir değer döner ve asla panic etmez.
    #[test]
    fn detect_never_panics_and_yields_exhaustive_variant() {
        let hmm = detect_clipboard();
        match hmm {
            ClipboardImpl::None | ClipboardImpl::WlCopy | ClipboardImpl::XClip => {}
        }
        assert!(!hmm.label().is_empty());
    }

    /// CLI yardımcısı yokken okuma/yazma zarif `Err` döner — asla panic değil.
    #[test]
    fn write_with_none_backend_is_graceful_err() {
        let c = SystemClipboard {
            backend: ClipboardImpl::None,
        };
        assert!(c.write("deneme").is_err());
        assert!(c.read().is_err());
    }

    /// `command_exists` mevcut bir komutta `true`, yoksa `false` döner (CI — güvenli).
    #[test]
    fn command_exists_returns_bool() {
        assert!(command_exists("sh"));
        assert!(!command_exists("definitely-not-a-real-binary-xyz"));
    }
}
