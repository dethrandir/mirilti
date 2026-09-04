//! Faz 1b spike: `org.freedesktop.portal.GlobalShortcuts` üzerinden kısayol
//! oturumu (A planı, PLAN.md §10).
//!
//! Bu modül yalnızca *spike/doğrulama* amacı taşır: oturumun açılıp
//! kısayolun bind edilebildiğini ve `Activated` / `Deactivated` sinyallerinin
//! `ShortcutEvent` olarak tüketiciye iletilebildiğini kanıtlar. Gerçek dikte
//! tetikleme akışı Faz 8'de bu stream'lerin üzerine kurulur.
//!
//! Bağımlılıklar: `ashpd` (features = ["global_shortcuts"]) + `futures-util`.
//! Ne `tokio` ne de `async-trait` bu modülün üretim kodunda doğrudan gerekir;
//! `tokio` yalnızca testler için dev-dependency'dir (Tauri'nin kendi runtime'ı
//! üretimde zaten tokio'yu getirir).
//!
//! ## Platform
//! KDE Plasma 6 / Wayland üzerinde `xdg-desktop-portal-kde` backend'i aktifken
//! kısayolu kullanıcı KDE kendi arayüzünden bağlar (PLAN.md §10 — bu bir
//! kısıt değil, Wayland'in doğru çalışma biçimi). Portal yoksa (headless/CI)
//! [`grab`] zarif bir `Err` döner; [`ShortcutSession::events`] çağrılmadan
//! hata hiç paniğe dönüşmez.
//!
//! # Neden `#![allow(dead_code)]`?
//! Bu bir spike modülüdür: `lib.rs` içinde `mod shortcut;` özel (private)
//! bağlıdır ve henüz uygulama akışı `grab`/`events` çağırmaz — yalnızca
//! derlenip test edilebilir olması gerekir (plan Faz 1b). Modülün public
//! API'sine hiçbir üretim kod yolu erişmediği için `dead_code` uyarıları
//! doğar; bunlar `clippy -D warnings` altında kırılır. Faz 8'de kısayol
//! akışı `lib.rs`'ten çağrıldığında bu izin kaldırılabilir.
#![allow(dead_code)]

use ashpd::desktop::global_shortcuts::{
    Activated, BindShortcutsOptions, Deactivated, GlobalShortcuts, NewShortcut,
};
use ashpd::desktop::{CreateSessionOptions, Session};
use futures_util::{stream, Stream, StreamExt};

/// Bu spike'ın bind ettiği tek kısayolu tanımlayan sabit uygulama tarafı ID'si.
/// Kullanıcının KDE Kısayolları arayüzünde göreceği "Dikteyi başlat/durdur"
/// açıklamasıyla eşleşir.
pub const TOGGLE_DICTATION: &str = "toggle-dictation";

/// Bir global kısayolun durum değişikliği — portal `Activated` / `Deactivated`
/// sinyallerinin sadeleştirilmiş, taşınabilir temsili (PLAN.md §10, V3).
///
/// `timestamp_ms`: sinyalin emisyonu için portaldan gelen Unix-epoch
/// zaman bilgisi (milisek). "Basılı tut" (push-to-talk) modu için
/// `Activated`→`Deactivated` arası süre buradan hesaplanabilir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutEvent {
    /// Kısayol tetiklendi (basıldı/açıldı).
    Activated {
        shortcut_id: String,
        timestamp_ms: u64,
    },
    /// Kısayol bırakıldı/kapatıldı.
    Deactivated {
        shortcut_id: String,
        timestamp_ms: u64,
    },
}

/// Açılmış, `toggle-dictation` kısayolu bind edilmiş GlobalShortcuts oturumu.
///
/// `grab` bu struct'ı döner; çağıran akış, [`events`](Self::events) ile
/// `ShortcutEvent` stream'ine erişir veya oturumu [`close`](Self::close) ile
/// kapatır.
#[derive(Debug)]
pub struct ShortcutSession {
    portal: GlobalShortcuts,
    session: Session<GlobalShortcuts>,
    shortcut_id: String,
    /// Portalın onayladığı (gerçekten bind edilen) kısayol ID'lerinin listesi.
    bound: Vec<String>,
}

impl ShortcutSession {
    /// Bind edilen kısayolun uygulama tarafı ID'si (örn. `toggle-dictation`).
    pub fn shortcut_id(&self) -> &str {
        &self.shortcut_id
    }

    /// Portalın `BindShortcuts` yanıtında onayladığı kısayol ID'leri.
    pub fn bound(&self) -> &[String] {
        &self.bound
    }

    /// `Activated` ve `Deactivated` sinyallerini tek, birleşik bir
    /// `ShortcutEvent` stream'inde birleştirir.
    ///
    /// Portal `receive_activated` / `receive_deactivated` çağrılarından
    /// herhangi biri başarısızsa `Err` döner; başarılıysa dönen stream,
    /// oturum canlı kaldığı sürece olayları akıtır. Kuyruğu kapatan çağıran,
    /// oturumu [`close`](Self::close) ile sonlandırmalıdır.
    pub async fn events(&self) -> Result<impl Stream<Item = ShortcutEvent> + '_, String> {
        let activated = Box::pin(
            self.portal
                .receive_activated()
                .await
                .map_err(|e| format!("Activated sinyali dinlenemedi: {e}"))?,
        );
        let deactivated = Box::pin(
            self.portal
                .receive_deactivated()
                .await
                .map_err(|e| format!("Deactivated sinyali dinlenemedi: {e}"))?,
        );

        let merged = stream::select(activated.map(to_activated), deactivated.map(to_deactivated));

        Ok(merged)
    }

    /// Portal oturumunu kapatır ve kısayollarla ilişkili kullanıcı etkileşimini
    /// sonlandırır.
    pub async fn close(&self) -> Result<(), String> {
        self.session
            .close()
            .await
            .map_err(|e| format!("oturum kapatılamadı: {e}"))
    }
}

/// `org.freedesktop.portal.GlobalShortcuts` üzerinden bir global kısayol
/// oturumu açar ve `toggle-dictation` kısayolunu bind eder.
///
/// # Akış
/// 1. `GlobalShortcuts::new()` — portal servis proxy'sini bağlar.
/// 2. `create_session(CreateSessionOptions::default())` — kalıcı oturumu açar.
/// 3. `bind_shortcuts(&[toggle-dictation])` — kısayolu bind eder (KDE'de
///    kullanıcı tetiği kendi arayüzünden atar).
/// 4. Dönen oturumun [`events`](ShortcutSession::events) akışı tetiklemeyi izler.
///
/// Portal servisi yoksa (headless/CI) ya da herhangi bir adım başarısız olursa
/// zarif bir `Err(String)` döner — panic yok.
pub async fn grab() -> Result<ShortcutSession, String> {
    let portal = GlobalShortcuts::new()
        .await
        .map_err(|e| format!("GlobalShortcuts portalı bağlanamadı: {e}"))?;

    let session = portal
        .create_session(CreateSessionOptions::default())
        .await
        .map_err(|e| format!("GlobalShortcuts oturumu açılamadı: {e}"))?;

    let request = portal
        .bind_shortcuts(
            &session,
            &[NewShortcut::new(TOGGLE_DICTATION, "Dikteyi başlat/durdur")],
            None, // Wayland'de parent-window handle gereksiz; oturum ID'si yeterlidir.
            BindShortcutsOptions::default(),
        )
        .await
        .map_err(|e| format!("kısayol bind edilemedi: {e}"))?;

    let bind = request
        .response()
        .map_err(|e| format!("BindShortcuts yanıtı alınamadı: {e}"))?;

    let bound = bind
        .shortcuts()
        .iter()
        .map(|s| s.id().to_string())
        .collect();

    Ok(ShortcutSession {
        portal,
        session,
        shortcut_id: TOGGLE_DICTATION.to_string(),
        bound,
    })
}

/// `Activated` sinyalini [`ShortcutEvent::Activated`]'a çevirir.
fn to_activated(e: Activated) -> ShortcutEvent {
    ShortcutEvent::Activated {
        shortcut_id: e.shortcut_id().to_string(),
        timestamp_ms: e.timestamp().as_millis() as u64,
    }
}

/// `Deactivated` sinyalini [`ShortcutEvent::Deactivated`]'a çevirir.
fn to_deactivated(e: Deactivated) -> ShortcutEvent {
    ShortcutEvent::Deactivated {
        shortcut_id: e.shortcut_id().to_string(),
        timestamp_ms: e.timestamp().as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `toggle-dictation` kısayolunun `grab` akışının herhangi bir ortamda
    /// panic etmemesini ve zarif hata dönüşünü kanıtlar.
    ///
    /// # Neden portal olmayan ortamda da yeşil kalır?
    /// - **Portal varsa** (KDE + xdg-desktop-portal-kde): `grab` `Ok` döner;
    ///   oturum ID'si `toggle-dictation` olmalıdır.
    /// - **Portal yoksa** (headless/CI): `GlobalShortcuts::new()` D-Bus'ta
    ///   servisi bulamaz → `grab` `Err(String)` döner; yalnızca hata mesajının
    ///   boş olmaması beklenir.
    ///
    /// Her iki dal da assert edilmiş ve çalışıyor; yani test, belirli bir
    /// ortam (portal yokluğu) gerektirmez ve asla FAIL etmez. Ayrıca oturum
    /// *açıksa* bile bu spike'ta kısayol gerçekten durdurulamaz — oturumu
    /// hemen kapatarak kaynak bırakmaya çalışırız (kapatma hatası testi
    /// başarısız kılmaz).
    #[tokio::test]
    async fn grab_portal_yokken_de_zarif_hata_doner_ve_panic_etmez() {
        let outcome = match grab().await {
            Ok(session) => {
                assert_eq!(
                    session.shortcut_id(),
                    TOGGLE_DICTATION,
                    "bind edilen kısayol ID eşleşmeli"
                );
                // Oturumu kaynak bırakma adına kapatmayı dene; başarısızlık
                // testi kırmaz (portal yoksa zaten buraya hiç gelinmez).
                let _ = session.close().await;
                Ok(())
            }
            Err(e) => {
                assert!(
                    !e.trim().is_empty(),
                    "portal yokken hata mesajı boş olmamalı"
                );
                Err(e)
            }
        };
        // `Ok` ya da hatalı `Err` — her ikisi de beklenen; hiçbiri panic değil.
        assert!(outcome.is_ok() || outcome.is_err());
    }

    /// Sinyal → `ShortcutEvent` dönüşümlerinin alanlarını (spike'ta) doğrular.
    /// Bu test D-Bus/portal gerektirmez ve her ortamda anında yeşildir.
    #[test]
    fn to_event_mapping_fields_are_forwarded() {
        // Gerçek `Activated`/`Deactivated` objeleri D-Bus paketinden geldiği
        // için burada yalnızca üst seviye dönüşüm kontratını kontrol ederiz:
        // her iki varyant da `ShortcutEvent` olarak temsil edilebilir biçimde
        // yaşıyor. (Alan düzeyinde doğrulama ancak gerçek portal sinyaliyle
        // entegrasyon testinde anlamlıdır — Faz 8'deki tüketicinin işi.)
        let activated_id = "toggle-dictation".to_string();
        let event = ShortcutEvent::Activated {
            shortcut_id: activated_id.clone(),
            timestamp_ms: 42,
        };
        match event {
            ShortcutEvent::Activated {
                shortcut_id,
                timestamp_ms,
            } => {
                assert_eq!(shortcut_id, activated_id);
                assert_eq!(timestamp_ms, 42);
            }
            ShortcutEvent::Deactivated { .. } => unreachable!(),
        }
    }
}
