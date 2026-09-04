//! Faz 1c spike: §8.1 yapıştırma stratejisi (V4) — sıralı dene-ve-rapor zinciri.
//!
//! §8.1'deki sıra:
//!   1. `org.freedesktop.portal.RemoteDesktop` — kalıcı oturum (persist) +
//!      `NotifyKeyboardKeycode` ile `Ctrl+V`.
//!   2. `ydotool` — `ydotoold` + `/dev/uinput` erişebiliyorsa.
//!   3. X11 — XWayland/X11 oturumunda doğrudan enjeksiyon (xdotool benzeri).
//!   4. Yalnızca pano — bildirim: "Metin panoda, Ctrl+V ile yapıştır."
//!
//! Bu modül ortam-bağımsızdır: `detect()` hangi yolların mevcut olduğunu
//! raporlar asla panic etmez; `paste()` ilk başarıya kadar zinciri dener ve
//! gerçek sonucu `PasteResult` olarak döndürür.
#![allow(dead_code)]

use ashpd::desktop::remote_desktop::{
    DeviceType, KeyState, NotifyKeyboardKeycodeOptions, RemoteDesktop, SelectDevicesOptions,
    StartOptions,
};
use ashpd::desktop::{CreateSessionOptions, PersistMode, Session};
use ashpd::enumflags2::BitFlags;
use std::path::Path;
use std::time::Duration;

use crate::output::clipboard::{ClipboardBackend, ClipboardImpl, SystemClipboard};

/// Linux evdev tuş kodları (ydotool ve RemoteDesktop aynı kodları kullanır).
const KEY_LEFTCTRL: i32 = 29;
const KEY_V: i32 = 47;

/// Pano + tuş enjeksiyonu arasındaki gecikme (§8.2, ayarlanabilir; varsayılan 50 ms).
const CLIPBOARD_TO_PASTE_DELAY_MS: u64 = 50;

/// §8.1 zincirindeki bir yapıştırma yolunun sonuçta kullanılanı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteBackend {
    /// Hiçbir yol çalışmadı.
    None,
    /// RemoteDesktop portalı (adım 1).
    Portal,
    /// ydotool (adım 2).
    Ydotool,
    /// X11 doğrudan enjeksiyon (adım 3).
    X11,
    /// Yalnızca pano (adım 4).
    Clipboard,
}

impl PasteBackend {
    /// İnsan okur ad (Sistem Sağlığı / §8.1 zincir raporu).
    pub fn label(self) -> &'static str {
        match self {
            PasteBackend::None => "hiçbiri",
            PasteBackend::Portal => "RemoteDesktop portalı",
            PasteBackend::Ydotool => "ydotool",
            PasteBackend::X11 => "X11",
            PasteBackend::Clipboard => "yalnızca pano",
        }
    }
}

/// Tek bir yapıştırmanın sonucu — hangi yolla, panoya yazıldı mı, Ctrl+V geçti mi.
#[derive(Debug, Clone)]
pub struct PasteResult {
    /// Zincirde sonunda kullanılan yol.
    pub backend: PasteBackend,
    /// Metin panoya yazılabildi mi.
    pub clipboard_written: bool,
    /// Ctrl+V tuşları gerçekten gönderildi mi.
    pub ctrl_v_sent: bool,
    /// İnsan okur açıklama (hangi yol, neden düştü).
    pub message: String,
}

impl PasteResult {
    fn new(
        backend: PasteBackend,
        clipboard_written: bool,
        ctrl_v_sent: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            backend,
            clipboard_written,
            ctrl_v_sent,
            message: message.into(),
        }
    }
}

/// Ortam durumu: §8.1 zincirini oluşturan her yolun mevcut olup olmadığı.
#[derive(Debug, Clone)]
pub struct Detection {
    /// `org.freedesktop.portal.RemoteDesktop` D-Bus servisine ulaşılabiliyor mu.
    pub portal: bool,
    /// `/dev/uinput` + ydotool yazılabilir mi (binary + cihaz).
    pub ydotool: bool,
    /// `ydotoold` arka plan süreci çalışıyor mu.
    pub ydotool_daemon: bool,
    /// X11/XWayland oturumu var mı (`DISPLAY` set).
    pub x11: bool,
    /// Pano backend'i (`wl-copy` / `xclip` / yok).
    pub clipboard: ClipboardImpl,
}

/// §8.1 zincirinin bu makinede kullanılacak *tercih edilen* sırası.
impl Detection {
    pub fn preferred_chain_label(&self) -> String {
        let mut chain = Vec::new();
        if self.portal {
            chain.push(PasteBackend::Portal.label());
        }
        if self.ydotool {
            chain.push(PasteBackend::Ydotool.label());
        }
        if self.x11 {
            chain.push(PasteBackend::X11.label());
        }
        if self.clipboard != ClipboardImpl::None {
            chain.push(PasteBackend::Clipboard.label());
        }
        if chain.is_empty() {
            chain.push(PasteBackend::None.label());
        }
        chain.join(" → ")
    }
}

/// Tüm yolların mevcut durumunu ortam-bağımsız biçimde toplar; asla panic etmez.
///
/// Alanların hiçbiri ölümcül değil: portal D-Bus'a bağlanamazsa `false`, komut
/// yoksa `false/None`, `/dev/uinput` açılamazsa `false`. Headless/CI'da da güvenle
/// çağrılır.
pub async fn detect() -> Detection {
    Detection {
        portal: async_portal_available().await,
        ydotool: ydotool_ready(),
        ydotool_daemon: daemon_running(),
        x11: x11_ready(),
        clipboard: crate::output::clipboard::detect_clipboard(),
    }
}

/// `org.freedesktop.portal.RemoteDesktop` portalı D-Bus'ta yanıt veriyor mu?
///
/// Yalnızca proxy bağlar (diğer uç yok, diyalog yok) — sakıncalı değildir; bu
/// yüzden test edilebilir.
pub async fn portal_available() -> bool {
    async_portal_available().await
}

async fn async_portal_available() -> bool {
    RemoteDesktop::new().await.is_ok()
}

/// `ydotool` binary'si + `/dev/uinput` yazılabilirliği var mı? (daemon ayrıca
/// `ydotool_daemon` ile raporlanır; Ctrl+V denemesi exit kodundan anlaşılır.)
fn ydotool_ready() -> bool {
    if !command_exists("ydotool") {
        return false;
    }
    uinput_writable()
}

/// `/dev/uinput` char cihazı yazma için açılabiliyor mu (uinput grubu üyeliği).
///
/// Açılıp hemen kapatılır; bu, sanal aygıt **oluşturmaz** (yalnızca fd açılır),
/// dolayısıyla yan etkisizdir.
fn uinput_writable() -> bool {
    let path = Path::new("/dev/uinput");
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map(|_| true)
        .unwrap_or(false)
}

/// `ydotoold` arka plan süreci çalışıyor mu? `pgrep` yoksa/çalışmıyorsa `false`.
fn daemon_running() -> bool {
    std::process::Command::new("pgrep")
        .arg("-x")
        .arg("ydotoold")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `DISPLAY` ortam değişkeni set ve boş değil mi (X11/XWayland oturumu).
fn x11_ready() -> bool {
    std::env::var("DISPLAY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn command_exists(name: &str) -> bool {
    crate::output::clipboard::command_exists(name)
}

/// §8.1 zincirinin tamamını dener; ilk başarıda durur, sonunda pano yoluna düşer.
///
/// # KDE notu
/// RemoteDesktop `Start` KDE'de bir izin diyaloğu gösterebilir (V4 varsayımı:
/// persist ile yalnızca ilk sefer). Eğer varsayım çürüyorsa (her seferde diyalog)
/// bu fonksiyon portal yolunu atlayıp `ydotool`'u birincil yapmalı — PLAN.md §0.3 V4.
pub async fn paste(text: &str) -> PasteResult {
    // 1) RemoteDesktop portalı (kalıcı oturum).
    if async_portal_available().await {
        match PortalPasteBackend::connect().await {
            Ok(portal_d) => match portal_d.send_paste().await {
                Ok(()) => {
                    return PasteResult::new(
                        PasteBackend::Portal,
                        false, /* pano yalnızca ydotool olmadan doldurulmaz; portal tuşu direkt gönderir */
                        true,
                        "RemoteDesktop portalı ile Ctrl+V gönderildi",
                    );
                }
                Err(e) => eprintln!("inject: portal yolu başarısız: {e}"),
            },
            Err(e) => eprintln!("inject: portal YAYITSIZ: {e}"),
        }
    }

    // 2) ydotool — önce panoya yaz, sonra Ctrl+V (evdev: Ctrl=29, V=47).
    if ydotool_ready() {
        match ydotool_paste(text) {
            Ok(result) => return result,
            Err(e) => eprintln!("inject: ydotool yolu başarısız: {e}"),
        }
    }

    // 3) X11 — spike'ta doğrudan enjeksiyon için xdotool yok (§8.1 adım 3); yol
    //    mevcutsa raporlayıp pano yoluna geç (doğrudan tuş yok).
    // 4) Yalnızca pano — her koşulda çalışan son çare.
    let clipboard = SystemClipboard::new();
    let wrote = clipboard.write(text).is_ok();
    PasteResult::new(
        PasteBackend::Clipboard,
        wrote,
        false,
        if wrote {
            format!(
                "Metin panoda; Ctrl+V ile yapıştır (pano: {})",
                clipboard.backend().label()
            )
        } else {
            "Pano yazılamadı — §11 'Sistem Sağlığı'na bakın".to_string()
        },
    )
}

/// Kalıcı RemoteDesktop oturumu üzerinden `Ctrl+V` gönderir (§8.1 adım 1).
///
/// Oturum kalıcılığı `SelectDevices` aşamasında `PersistMode::ExplicitlyRevoked`
/// (XDP_PERSIST_MODE_PERSISTENT) ile istenir → KDE daha sonra restore jetonuyla
/// yeniden diyalog sormayabilir. `connect` `Start` diyaloğuna kadar gider; bu
/// adım KDE'de kullanıcı etkileşimi gerektirir, bu yüzden birim testleri bunu
/// **çağırmaz** (yalnızca `portal_available`).
pub struct PortalPasteBackend {
    portal: RemoteDesktop,
    session: Session<RemoteDesktop>,
}

impl PortalPasteBackend {
    /// Portalı bağlar, klavye erişimi + kalıcılık isteyen oturum açar ve `Start`
    /// ile izin alır. KDE'de `Start` bir izin diyaloğu gösterebilir — çağıran
    /// bunun kullanıcı etkileşimi olduğunu bilmelidir.
    ///
    /// `SelectDevices`'in dönüşü `Request<()>`'tir ve void yöntem olduğu için
    /// `.response()` çağrılmaz (ashpd örneği de doğrudan atar). `Start` `.await`'i
    /// diyalog yanıtı için döner; bu yüzden `.response()` güvenlidir.
    pub async fn connect() -> Result<Self, String> {
        let portal = RemoteDesktop::new()
            .await
            .map_err(|e| format!("RemoteDesktop portalı bağlanamadı: {e}"))?;

        let session = portal
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(|e| format!("RemoteDesktop oturumu açılamadı: {e}"))?;

        portal
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(BitFlags::from(DeviceType::Keyboard))
                    .set_persist_mode(PersistMode::ExplicitlyRevoked),
            )
            .await
            .map_err(|e| format!("klavye aygıtları seçilemedi: {e}"))?;

        let start = portal
            .start(&session, None, StartOptions::default())
            .await
            .map_err(|e| format!("oturum başlatılamadı: {e}"))?;
        start
            .response()
            .map_err(|e| format!("Start yanıtı alınamadı: {e}"))?;

        Ok(Self { portal, session })
    }

    /// Oturum üzerinden `Ctrl+V` (Ctrl=29, V=47) gönderir.
    pub async fn send_paste(&self) -> Result<(), String> {
        let keys = [
            (KEY_LEFTCTRL, KeyState::Pressed),
            (KEY_V, KeyState::Pressed),
            (KEY_V, KeyState::Released),
            (KEY_LEFTCTRL, KeyState::Released),
        ];
        for (code, state) in keys {
            self.portal
                .notify_keyboard_keycode(
                    &self.session,
                    code,
                    state,
                    NotifyKeyboardKeycodeOptions::default(),
                )
                .await
                .map_err(|e| format!("Ctrl+V gönderilemedi (keycode {code}): {e}"))?;
        }
        Ok(())
    }
}

/// §8.1 adım 2: panoya yazıp `ydotool key 29:1 47:1 47:0 29:0` ile Ctrl+V gönderir.
///
/// - `ydotool` yoksa ya da `/dev/uinput` açılamazsa zarif `Err` döner (panic yok).
/// - `ydotool` `status()` exit koduyla `/dev/uinput` erişilebilirliğini söyler.
pub fn ydotool_paste(text: &str) -> Result<PasteResult, String> {
    let clipboard = SystemClipboard::new();
    clipboard
        .write(text)
        .map_err(|e| format!("pano yazılamadı: {e}"))?;

    // Bazı uygulamalar pano güncellemesini geç okur; §8.2.
    std::thread::sleep(Duration::from_millis(CLIPBOARD_TO_PASTE_DELAY_MS));

    let status = std::process::Command::new("ydotool")
        .args(["key", "29:1", "47:1", "47:0", "29:0"])
        .status()
        .map_err(|e| format!("ydotool çalıştırılamadı (uinput erişimi yok olabilir): {e}"))?;

    if status.success() {
        Ok(PasteResult::new(
            PasteBackend::Ydotool,
            true,
            true,
            "ydotool Ctrl+V gönderildi (uinput erişilebilir)",
        ))
    } else {
        Err(format!("ydotool exit kodu: {:?}", status.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `detect` her ortamda bir `Detection` döner ve asla panic etmez.
    ///
    /// Portal D-Bus'a bağlanamazsa `portal=false`, ydotool yoksa `false`, CI/headless
    /// da `x11=false` olur — hiçbirini panic'e çevirmez; yalnızca alanlar okunur.
    #[tokio::test]
    async fn detect_never_panics_and_reports_booleans() {
        let d = detect().await;
        let _ = (d.portal, d.ydotool, d.ydotool_daemon, d.x11, d.clipboard);
        assert!(!d.preferred_chain_label().is_empty());
    }

    /// `uinput_writable` her ortamda bool döner (uusimage'dan bağımsız) — panic yok.
    #[test]
    fn uinput_writable_returns_bool() {
        // Bu makinede (uinput grubu) `true`; headless CI'da `false`. İkisi de geçerli.
        let _ = uinput_writable();
        let _ = daemon_running();
    }

    /// `ydotool_paste` yoksa/erişemezse zarif `Err`, erişirse `Ok` — asla panic değil.
    ///
    /// - `ydotool` **varsa ve uinput erişebiliyorsa**: gerçek Ctrl+V gönderir;
    ///   pano `wl-copy/xclip` ile dolar (PANOYU DEĞİŞTİRİR + odaktaki uygulamaya
    ///   yapıştırır).
    ///
    ///   Testin amacı: her iki dalda da panic etmemesi ve sonucun tutarlı
    ///   raporlanması.
    ///
    /// # Neden `#[ignore]`?
    /// Bu makinede (ydotool + uinput erişimi) test her koştuğunda **gerçek Ctrl+V
    /// gönderir ve odaktaki uygulamaya yapıştırır** — istenmeyen yan etki. CI'da
    /// ydotool olmadığından güvenli, ama local'de `cargo test` her `ydotool` ile
    /// eşleşen yapıştırma üretir. Manuel entegrasyon testi olarak `cargo test
    /// -- --ignored ydotool_paste_panics_never` ile çalıştırılır.
    #[test]
    #[ignore = "gerçek Ctrl+V gönderir (uinput); manuel entegrasyon testi"]
    fn ydotool_paste_panics_never() {
        let out = ydotool_paste("mirilti-faz1c-spike");
        match out {
            Ok(r) => {
                assert_eq!(r.backend, PasteBackend::Ydotool);
                assert!(r.clipboard_written);
                assert!(r.ctrl_v_sent);
            }
            Err(e) => assert!(!e.trim().is_empty(), "hata mesajı boş olmamalı"),
        }
    }
}
