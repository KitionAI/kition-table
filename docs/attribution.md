# Attribution

Third-party code and non-code assets in this repository and the kit package, with their source and
licence. Kition Table's own code is under the Apache License 2.0 ([LICENSE](../LICENSE));
[NOTICE](../NOTICE) says where the code came from. The repository holds no third-party source
code. The editor's grid derived from Teable until 2026-11-18, when it was replaced by the grid the
engine lays out (`crates/view/grid`, `web/src/editor/table/canvas/`), written by the Kition
authors; every remaining file under `web/src/` was compared with Teable's `packages/sdk` grid and
`packages/core` colours (commits `1e353a0e` and `bd8cfc05`), and none derives from them.

## Code bundled in the kit package

The engine (`bindings/wasm`, built to WebAssembly) and the editor (`web/`, built with Vite) bundle
these third-party libraries. Versions are the ones `Cargo.lock` and `web/pnpm-lock.yaml` resolve.

| Library | Version | License | Where |
|---|---|---|---|
| [SQLite](https://sqlite.org) | as bundled by libsqlite3-sys and sqlite-wasm-rs | Public domain | Engine: the `.ktable` file format |
| [rusqlite](https://github.com/rusqlite/rusqlite) / libsqlite3-sys | 0.38.0 / 0.36.0 | MIT | Engine (native and headless) |
| [sqlite-wasm-rs](https://github.com/Spxg/sqlite-wasm-rs) | 0.5.5 | MIT | Engine (WebAssembly) |
| `kition-tessera-icons` ([kition-tessera](https://github.com/KitionAI/kition-tessera)) | pinned by commit | Apache-2.0; its `lucide:` icons are [Lucide](https://lucide.dev) path data, ISC (Feather-derived ones MIT, Copyright (c) 2013-present Cole Bemis); texts in [NOTICE](../NOTICE) | Engine: the picture renderer |
| Inter, Nanum Gothic, Noto Color Emoji, Droid Sans Fallback | The upstream files `scripts/fetch-fonts.sh` downloads and checks ([development.md](development.md#fonts)) | SIL OFL 1.1 (Inter, Nanum Gothic, Noto Color Emoji), Apache-2.0 (Droid Sans Fallback); OFL text in [NOTICE](../NOTICE) | The headless executable (`crates/view/fonts`, `embed` feature) and the web package (`web/public/fonts/`, copied by `web/scripts/fonts.mjs` and checked against `web/fonts.lock.json`, each file with its license) |
| [resvg](https://github.com/linebender/resvg), usvg | 0.50.0 | Apache-2.0 OR MIT | Engine: SVG attachments drawn as thumbnails (no text, nothing loaded from outside the file) |
| [tiny-skia](https://github.com/linebender/tiny-skia), tiny-skia-path | 0.12.0 | BSD-3-Clause; text in [NOTICE](../NOTICE) | Engine: resvg's rasterizer |
| [arrayref](https://github.com/droundy/arrayref) | 0.3.9 | BSD-2-Clause; text in [NOTICE](../NOTICE) | Engine: through tiny-skia |
| [heic-rs](https://github.com/tbraun96/heic-rs) | 0.1.4 (pinned) | MIT OR Apache-2.0; grants no HEVC patent licence | Engine: HEIC attachments decoded as thumbnails |
| [gif](https://github.com/image-rs/image-gif) | 0.14.2 | MIT OR Apache-2.0 | Engine: GIF attachments (the first frame) |
| Other Rust crates in `Cargo.lock` | as locked | MIT, Apache-2.0, or both (each crate's own licence) | Engine |
| [CodeMirror](https://codemirror.net) (`@codemirror/autocomplete`, `commands`, `language`, `search`, `state`, `view`) | 6.x | MIT | Editor: formula and text editing |
| [React](https://react.dev), React DOM | 19.3.1 | MIT | Editor |
| [react-grid-layout](https://github.com/STRML/react-grid-layout), [react-resizable](https://github.com/react-grid-layout/react-resizable) | 2.3.0, 3.2.0 | MIT | Editor: dashboards |
| [i18next](https://www.i18next.com), [react-i18next](https://react.i18next.com) | 26.5.2, 17.1.0 | MIT | Editor: translations |
| [clsx](https://github.com/lukeed/clsx), [tailwind-merge](https://github.com/dcastil/tailwind-merge) | 2.1.1, 3.7.0 | MIT | Editor: class names |
| [zod](https://zod.dev) | 4.1.13 | MIT | Editor: data validation |
| `@kition/sdk` ([kition-sdk](https://github.com/KitionAI/kition-sdk)), `@kition/tessera` ([kition-tessera](https://github.com/KitionAI/kition-tessera)) | 0.5.2 and 0.1.1 (rehearsal copies `@kitionstage/sdk`, `@kitionstage/tessera` until their first release), from npm | Apache-2.0; Tessera's Lucide icons ISC (Feather-derived ones MIT), texts in [NOTICE](../NOTICE) | Editor |

## Assets

| Path | Author | Source | License | Notes |
|---|---|---|---|---|
| `web/public/templates/batch-product-designer/`, `web/public/templates/youtube-tiktok-thumbnail-generator/`, `web/public/templates/receipt-ocr-database/cover.webp` (56 pictures) | Kition, generated with OpenAI gpt-image 2.0 through Kition Cloud | The original PNGs carry C2PA Content Credentials signed by OpenAI ("OpenAI Media Service API", action `c2pa.created` by `gpt-image` 2.0, no input ingredients); Kition commits `3a9d54a3` and `83662d3a`, converted to WebP (metadata dropped) in `97d29e46`, moved here in `042293b`. The template manifests record "Original Kition Cloud AI generation" | Output owned by Kition under OpenAI's terms; no third-party author | AI-generated (IPTC digital source type `trainedAlgorithmicMedia`). Checked: generated products and people only, no real brands, trademarks or addresses |
| `web/public/templates/receipt-ocr-database/records/` (10 pictures) | Kition authors | Rendered by `web/scripts/render-receipts.mjs` from `web/src/templates/receiptSamples.json`: fictional stores, addresses and items (the template seeds its records from the same data) | Apache-2.0 | Text set in Cousine and Inter (SIL OFL 1.1), which are not shipped |
| `web/public/templates/table-covers/` (11 pictures) | Kition authors | Rendered by `web/scripts/render-covers.mjs` from HTML and CSS (Kition `docs/design.md` colours) | Apache-2.0 | Text set in Inter (SIL OFL 1.1), which is not shipped |
| `web/public/templates/**/manifest.json` | Kition authors | written for the templates | Apache-2.0 | Template definitions |
| `conformance/*.ktable`, `conformance/edit.json` | Kition Table contributors | written for the conformance suite | Apache-2.0 | Fixtures, one per diagnostic code |
| `conformance/expected/*.png`, `conformance/expected/tolerance.json` | Kition Table contributors | rendered by this repository's engine | Apache-2.0 | Reference renders |
| `cases/**/input/*.ktable` | Kition Table contributors | written for the cases | Apache-2.0 | Case inputs |
| `model/schema.json` | Kition Table contributors | written for the KDM model | Apache-2.0 | The document's KDM schema |
