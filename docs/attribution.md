# Attribution

Third-party code and non-code assets in this repository and the kit package, with their source and
licence. Kition Table's own code is under the Apache License 2.0 ([LICENSE](../LICENSE));
[NOTICE](../NOTICE) says where the code came from. The only third-party source code in the
repository is the editor's canvas grid, derived from Teable (below).

## Code derived from other projects

| Path | Upstream | License | Notes |
|---|---|---|---|
| `web/src/editor/table/grid/` | [Teable](https://github.com/teableio/teable) `packages/sdk/src/components/grid/`, Copyright (c) 2023-2025 Teable, Inc. | MIT (text in [NOTICE](../NOTICE)) | The canvas grid. |
| `web/src/editor/table/grid/_shim/grid-core.ts` | Teable `packages/core/src/models/field/colors.ts`, same copyright | MIT | Stands in for `@teable/core`: the select colour list, with the grid's own helpers |

Only Teable's MIT-licensed `packages/` are used; nothing comes from its AGPL-3.0 applications
(`apps/`, `plugins/`). Checked 2026-11-17 by comparing every file under `web/src/` and `web/locales/`
with Teable's repository at commits `1e353a0e` and
`b8425f5a`. Paths below are relative to `web/src/editor/table/grid/`.

- Copied unchanged from Teable's `grid/` (36): `CellScroller.tsx`, `index.ts`, `configs/grid.ts`,
  `configs/index.ts`, `components/editor/index.ts`, `hooks/index.ts`, `hooks/useDrag.ts`,
  `hooks/useResizeObserver.ts`, `hooks/useScrollFrameRate.ts`, `hooks/useVisibleRegion.ts`,
  `managers/index.ts`, `managers/coordinate-manager/` (`CoordinateManager.ts`, `index.ts`,
  `interface.ts`), `managers/image-manager/index.ts`,
  `managers/performance-tracker/` (`PerformanceTracker.ts`, `index.ts`),
  `managers/selection-manager/index.ts`, `managers/sprite-manager/index.ts`, `renderers/index.ts`,
  `renderers/base-renderer/` (`index.ts`, `interface.ts`), `renderers/cell-renderer/`
  (`booleanCellRenderer.ts`, `chartCellRenderer.ts`, `index.ts`, `loadingCellRenderer.ts`,
  `utils.ts`), `renderers/layout-renderer/` (`index.ts`, `interface.ts`), `utils/` (`element.ts`,
  `group.ts`, `hotkey.ts`, `index.ts`, `measure.ts`, `range.ts`, `utils.ts`).
- Copied from Teable's `grid/` and modified by the Kition authors (38): `Grid.tsx`,
  `InfiniteScroller.tsx`, `InteractionLayer.tsx`, `RenderLayer.tsx`, `TouchLayer.tsx`,
  `interface.ts`, `components/` (`ErrorIndicator.tsx`, `LoadingIndicator.tsx`, `index.ts`),
  `components/editor/` (`BooleanEditor.tsx`, `EditorContainer.tsx`, `RatingEditor.tsx`,
  `SelectEditor.tsx`, `TextEditor.tsx`), `configs/gridTheme.ts`, `hooks/` (`useAutoScroll.ts`,
  `useColumnFreeze.ts`, `useColumnResize.ts`, `useEventListener.ts`, `useKeyboardSelection.ts`,
  `useSelection.ts`), `managers/image-manager/ImageManager.ts`,
  `managers/selection-manager/CombinedSelection.ts`, `managers/sprite-manager/` (`SpriteManager.ts`,
  `sprites.tsx`), `renderers/base-renderer/baseRenderer.ts`, `renderers/cell-renderer/`
  (`buttonCellRenderer.ts`, `imageCellRenderer.ts`, `interface.ts`, `linkCellRenderer.ts`,
  `numberCellRenderer.ts`, `ratingCellRenderer.ts`, `selectCellRenderer.ts`,
  `textCellRenderer.ts`, `userCellRenderer.ts`), `renderers/layout-renderer/layoutRenderer.ts`,
  `utils/` (`region.ts`, `rowControl.ts`).
- The Kition authors' own (23): `useGridAdapter.ts`, `hooks/useReactRuntime.ts`,
  `components/AIOverlayIndicator.tsx`, `components/editor/TableDateCellEditor.tsx`,
  `renderers/aiActionLayer.ts`, `renderers/cell-renderer/alignment.ts`,
  `utils/selectChoiceColors.ts`, `_shim/grid-types.ts`, `_shim/use-is-touch-device.ts`, and every
  `*.spec.ts` and `*.spec.tsx` in the folder.

## Code bundled in the kit package

The engine (`bindings/wasm`, built to WebAssembly) and the editor (`web/`, built with Vite) bundle
these third-party libraries. Versions are the ones `Cargo.lock` and `web/pnpm-lock.yaml` resolve.

| Library | Version | License | Where |
|---|---|---|---|
| [SQLite](https://sqlite.org) | as bundled by libsqlite3-sys and sqlite-wasm-rs | Public domain | Engine: the `.ktable` file format |
| [rusqlite](https://github.com/rusqlite/rusqlite) / libsqlite3-sys | 0.38.0 / 0.36.0 | MIT | Engine (native and headless) |
| [sqlite-wasm-rs](https://github.com/Spxg/sqlite-wasm-rs) | 0.5.5 | MIT | Engine (WebAssembly) |
| `kition-tessera-icons` ([kition-tessera](https://github.com/KitionAI/kition-tessera)) | pinned by commit | Apache-2.0; its `lucide:` icons are [Lucide](https://lucide.dev) path data, ISC (Feather-derived ones MIT, Copyright (c) 2013-present Cole Bemis); texts in [NOTICE](../NOTICE) | Engine: the picture renderer |
| Inter, Noto Color Emoji, Droid Sans Fallback ([kition-fonts](https://github.com/KitionAI/kition-fonts)) | kition-fonts' pinned files | SIL OFL 1.1 (Inter, Noto Color Emoji), Apache-2.0 (Droid Sans Fallback); OFL text in [NOTICE](../NOTICE) | Headless executable only (`crates/view/fonts`, `embed` feature); the web package ships no font |
| Other Rust crates in `Cargo.lock` | as locked | MIT, Apache-2.0, or both (each crate's own licence) | Engine |
| [CodeMirror](https://codemirror.net) (`@codemirror/autocomplete`, `commands`, `language`, `search`, `state`, `view`) | 6.x | MIT | Editor: formula and text editing |
| [React](https://react.dev), React DOM | 19.3.1 | MIT | Editor |
| [react-grid-layout](https://github.com/STRML/react-grid-layout), [react-resizable](https://github.com/react-grid-layout/react-resizable) | 2.3.0, 3.2.0 | MIT | Editor: dashboards |
| [react-hammerjs](https://github.com/JedWatson/react-hammerjs) (with hammerjs) | 1.0.1 | MIT | Editor: touch gestures |
| [react-hotkeys-hook](https://github.com/JohannesKlauss/react-keymap-hook) | 5.3.3 | MIT | Editor: shortcuts |
| [i18next](https://www.i18next.com), [react-i18next](https://react.i18next.com) | 26.5.2, 17.1.0 | MIT | Editor: translations |
| [lodash-es](https://lodash.com) | 4.18.1 | MIT | Editor |
| [lru-cache](https://github.com/isaacs/node-lru-cache) | 10.2.0 | ISC | Editor |
| [scroller](https://github.com/popham/scroller) | 0.0.3 | MIT | Editor: grid scrolling |
| [clsx](https://github.com/lukeed/clsx), [tailwind-merge](https://github.com/dcastil/tailwind-merge) | 2.1.1, 3.7.0 | MIT | Editor: class names |
| [ts-key-enum](https://gitlab.com/nfriend/ts-key-enum), [ts-keycode-enum](https://github.com/nfriend/ts-keycode-enum) | 2.0.12, 1.0.6 | MIT | Editor: key names |
| [zod](https://zod.dev) | 4.1.13 | MIT | Editor: data validation |
| `@kition/kit-editor`, `@kition/kit-host`, `@kition/kit-ui-protocol` ([kition-sdk](https://github.com/KitionAI/kition-sdk)), `@kition/tessera-icons` ([kition-tessera](https://github.com/KitionAI/kition-tessera)) | 0.1.0, from npm | Apache-2.0; tessera-icons' Lucide icons ISC (Feather-derived ones MIT), texts in [NOTICE](../NOTICE) | Editor |

## Assets

| Path | Author | Source | License | Notes |
|---|---|---|---|---|
| `web/public/templates/batch-product-designer/`, `web/public/templates/youtube-tiktok-thumbnail-generator/`, `web/public/templates/receipt-ocr-database/cover.webp` (56 pictures) | Kition, generated with OpenAI gpt-image 2.0 through Kition Cloud | The original PNGs carry C2PA Content Credentials signed by OpenAI ("OpenAI Media Service API", action `c2pa.created` by `gpt-image` 2.0, no input ingredients); Kition commits `3a9d54a3` and `83662d3a`, converted to WebP (metadata dropped) in `97d29e46`, moved here in `042293b`. The template manifests record "Original Kition Cloud AI generation" | Output owned by Kition under OpenAI's terms; no third-party author | AI-generated (IPTC digital source type `trainedAlgorithmicMedia`). Checked: generated products and people only, no real brands, trademarks or addresses |
| `web/public/templates/receipt-ocr-database/records/` (10 pictures) | Kition authors | Rendered by `web/scripts/render-receipts.mjs` from `web/src/templates/receiptSamples.json`: fictional stores, addresses and items (the template seeds its records from the same data) | Apache-2.0 | Text set in Cousine and Inter (SIL OFL 1.1, from kition-fonts), which are not shipped |
| `web/public/templates/table-covers/` (11 pictures) | Kition authors | Rendered by `web/scripts/render-covers.mjs` from HTML and CSS (Kition `docs/design.md` colours) | Apache-2.0 | Text set in Inter (SIL OFL 1.1, from kition-fonts), which is not shipped |
| `web/public/templates/**/manifest.json` | Kition authors | written for the templates | Apache-2.0 | Template definitions |
| `conformance/*.ktable`, `conformance/edit.json` | Kition Table contributors | written for the conformance suite | Apache-2.0 | Fixtures, one per diagnostic code |
| `conformance/golden/*.png`, `conformance/golden/tolerance.json` | Kition Table contributors | rendered by this repository's engine | Apache-2.0 | Reference renders |
| `cases/**/input/*.ktable` | Kition Table contributors | written for the cases | Apache-2.0 | Golden case inputs |
| `model/schema.json` | Kition Table contributors | written for the KDM model | Apache-2.0 | The document's KDM schema |
