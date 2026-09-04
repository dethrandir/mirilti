# mirilti

Yerel, gizlilik-öncelikli dikte uygulaması. Kısayola bas, konuş, metin imlece gider.
Türkçe birinci sınıf vatandaştır: sondan eklemeli morfoloji, kesme işareti, büyük harf
ve teknik terim karışımı için özel tasarlanmış düzeltme hattı içerir.

> **Durum:** Aktif geliştirme — Faz 0 (repo iskeleti) aşamasında. Plan ve yol haritası
> için [PLAN.md](./PLAN.md), kararlar ve ölçümler için [NOTES.md](./NOTES.md).

## Ne yapar

- **Yerel ASR**: whisper.cpp `whisper-server`, ayrı süreç, `127.0.0.1`.
- **Yerel düzeltme**: llama.cpp `llama-server` + gemma sınıfı düzeltici LLM, ayrı süreç.
  Post-processing bir düzeltmedir; saparsa ham metne döner.
- **Akış bölünmez**: onay ekranı yok. Kayıt → transkripsiyon → düzeltme → yapıştırma.
- **Ses asla kaybolmaz**: motorlar hazır olmasa bile kayıt hemen başlar.
- **Hiçbir şey makineden çıkmaz**: telemetri yok; tek istisna kullanıcının açıkça
  kurduğu BYOK uçları.
- **Her dikte ölçülür**: kayıt/transkripsiyon/düzeltme/yapıştırma süreleri geçmişe yazılır.

## Gereksinimler

- Linux x86_64 (referans: Fedora 43, KDE Plasma, Wayland)
- Tauri v2 geliştirme için: `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`,
  `librsvg2-dev` vb. (CI ve yerel build)

## Geliştirme

```bash
pnpm install        # frontend bağımlılıkları
pnpm tauri dev      # geliştirme (Vite + Rust core)
pnpm build          # frontend typecheck + build
# Rust:
cd src-tauri && cargo check && cargo clippy --all-targets -- -D warnings
```

## Lisans

AGPL-3.0-only. Üçüncü taraf motorların (whisper.cpp, llama.cpp) lisansları "Hakkında"
ekranında listelenir.