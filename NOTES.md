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

- V1 overlay (wlr-layer-shell) → lazer: gerçek doğrulama Faz 1a'da.
- V4 yapıştırma portalı güvenilirliği → Faz 1c.
- whishper/llama sürüm bayrakları → Faz 3 yetenek sondası (V6/V7).