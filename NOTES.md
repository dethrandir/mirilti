# NOTES.md — Kararlar, Ölçümler, Bilinen Sınırlar

> PLAN.md'deki doğrulanmış varsayımlar (§0.3), faz sonuçları, spike kararları ve ölçümler
> burada tutulur. Plan'da uydurulan detay değil, **doğrulanmış** bilgi girer.

## Faz 0 — Repo iskeleti

**Doğrulanan:**

- Ortam: node 26, pnpm 10.32, rustc 1.96, `gh` → `dethrandir` (`repo` + `workflow`).
- Tauri v2 Linux system prereq'ler mevcut: gtk 3.24.52, webkit2gtk-4.1 2.52.5,
  libsoup-3.0 3.6.6, javascriptcoregtk-4.1, librsvg. **Eksik:** `ayatana-appindicator3`
  (yalnızca tepsi ikonu Faz 8'de gerekli — build/dokümana not).
- Scaffold: `pnpm create tauri-app` → `react-ts` template, Tauri v2, bu dizine kuruldu.
- Tailwind v4 (CSS-first, `@tailwindcss/vite`) + shadcn/ui (new-york) kuruldu.
  Token sözleşmesi tek yerde: `src/index.css`. `@/` alias aktif (vite + tsconfig).
- TypeScript 6.0 `baseUrl` deprecation → `paths: { "@/*": ["./src/*"] }` baseUrl'siz.
- `pnpm build` ve `cargo check` geçti (Faz 0 kapısı).

**Kararlar:**

- git: `main` dalı, remote = `github.com/dethrandir/mirilti.git` (§0.4).
- `.gitignore`: `_arsiv/`, `*.gguf`, `*.bin`, `history.db`, `evalset/audio/`, `*.wav`,
  `src-tauri/target/`. **Ders:** `models/`/`engines/` global pattern'i `src/models` Rust
  modülünü de ezdi → CI iki kere kırmızı. Model dosyaları XDG dizinlerinde, repoda asla
  olmayacak; pattern kaldırıldı.
- CI Tauri build: `cargo tauri` yerine **`pnpm tauri build --no-bundle`** (cargo-tauri
  binary'si PATH'te yok, CLI node paketi olarak gelir).
- **Faz 0 kapandı (2026-09-05):** iskelet + tooling + CI + ilk push, Actions **yeşil**
  (frontend: biome+tsc+vite; rust: clippy+test+tauri build).

## Yapılacak spike'lar / Faz 1 girişi

## Faz 1 — Spike sonuçları (2026-09-05)

### 1a — Overlay (V1) ✅
- **Karar: C planı + görünürlük-kontrollü overlay.** A planı (gtk-layer-shell)
  **elenmiştir**: KWin `wlr-layer-shell-v1` protokolünü desteklemez (KDE kendi
  `org_kde_plasma_shell`'ini kullanır), `gtk-layer-shell-0` sistemde yok. Bu pencere
  KWin'de overlay katmanına alınamaz, normal pencere gibi davranırdı.
- **Çalışan prototip:** ikinci Tauri penceresi (`label: overlay`, `visible:false`
  doğar, transparent, alwaysOnTop, skipTaskbar, 96×96) + `src-overlay/` mini React +
  `set_overlay_visible` komutu + `pin_overlay_bottom_right` (X11'de sağ-alt köşeye).
- **Görsel doğrulama:** production build çalıştırıldı — `mirilti-overlay` penceresi
  sağ-alt bölgede açıldı, kırmızı nefes-alan kayıt halkası render edildi. `visible:false`
  doğduğu için boşta kullanıcının tıklamasını engellememesi sağlanır.
- **KWin "üstte+tut sabit" (Wayland):** `alwaysOnTop:true` tek başına güvenilmez
  (Wayland'de `_NET_WM_STATE` yok; GTK `keep_above` no-op kalır). Kalıcı çözüm KWin
  pencere kuralı: sınıf `mirilti` → "Keep above = Force" + "Position = Force  sağ-alt".
  (X11'de `alwaysOnTop` yeterli.) Kullanıcıya tek tıkla içe aktarılacak kural dosyası
  Faz 1d/9'da.
- **Tıklama geçirgenliği SINIRI:** KWin per-pencere input passthrough sağlamaz
  (xdg_toplevel'de yok). Overlay kendi 96×96 alanında tıklamayı yakalar → varsayılan
  `visible:false` + transparan tasarım bunu dengeler. §9 "girdi almaz" hedefi görünürlük
  düzeyinde karşılandı.
- **Bulgu:** pencerenin XWayland altında 200×200 açıldığı gözlendi (tauri.conf 96×96
  yazsa da) — GTK/webview min-boyut davranışı; Wayland native'de tekrar ölçülecek.

- V2 kısayol portalı kalıcılığı → Faz 1b.
- V4 yapıştırma portalı güvenilirliği → Faz 1c.
- whishper/llama sürüm bayrakları → Faz 3 yetenek sondası (V6/V7).