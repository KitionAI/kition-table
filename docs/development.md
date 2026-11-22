# Developing Kition Table

How to build Table from source, run its tests, and find your way around the repository. To use
Table in your own app, start with the [README](../README.md).

## Repository layout

| Path | What |
|---|---|
| `crates/` | The engine (Rust), in groups: `engine`, `formats`, `view` ([Layout](#layout) lists each crate) |
| `bindings/` | `wasm`: `kition-table-kit`, the engine behind the `KitDocument` contract (the editor's engine) |
| `apps/` | `kition-table`: the headless executable (this kit's headless program; other kits keep it in `bindings/headless`) |
| `model/` | `schema.json`: the document's KDM schema (`kit.json` `model.schema`), compiled into `bindings/wasm` |
| `web/` | The kit's editor on `@kition/sdk/editor` |
| `agent/` | The agent guide, the command list and the check catalogue |
| `conformance/` | One fixture per check code, and the reference renders |
| `cases/` | The test cases of the feature catalogue (`features.yaml`, beside `kit.json`, the kit manifest) |
| `docs/` | This file, the writing format, gaps, roadmap, the quality reference, attribution; `features/` (the engine commands), `parity/` |
| `assets/` | `readme/`: the pictures in the README |

## Quick start

Requirements and fonts are below. On a clean clone:

```sh
scripts/fetch-fonts.sh ~/kition-table-fonts        # see "Fonts"
export KITION_FONTS_DIR=~/kition-table-fonts

# The headless kit, and the table tools for any MCP client
cargo build --release -p kition-table              # target/release/kition-table
cargo install --git https://github.com/KitionAI/kition-sdk kition-kits --bin kition-kits-mcp
kition-kits-mcp --workspace <folder of tables> --kit target/release/kition-table

# The editor, in a development host
cd web
pnpm install
pnpm run engine                                    # the engine (WebAssembly) into public/engine, the fonts into public/fonts
node scripts/sample.mjs                            # a sample table for dev.html
pnpm dev                                           # http://localhost:5189/dev.html
```

## Requirements

- Rust 1.95 or newer, with the `wasm32-unknown-unknown` target for the page's engine.
- [wasm-pack](https://rustwasm.github.io/wasm-pack/) to build the engine for the editor. SQLite is
  compiled into the engine, so the WebAssembly build needs a C compiler that targets wasm32:
  Homebrew's LLVM on macOS (`brew install llvm`; the build script finds it), or set
  `CC_wasm32_unknown_unknown` and `AR_wasm32_unknown_unknown`. Apple's clang cannot target wasm32.
- A current Node.js (22 or newer) and pnpm 10 for the editor.
- The four fonts the engine draws text with, in a folder named by `KITION_FONTS_DIR` (see
  [Fonts](#fonts)). They are not in this repository.

The Rust crates of the [Kition SDK](https://github.com/KitionAI/kition-sdk) and Kition Tessera are
git dependencies pinned by commit in `Cargo.toml`; Cargo fetches them.

Run Cargo from the repository: `.cargo/config.toml` compiles SQLite with its `sqlite_dbpage`
table, which saving reads the document's pages through. A build without it saves nothing (the
error names the setting).

## Fonts

The picture (thumbnails, the agent's preview, `table.file.render`) draws text with the faces
below. `scripts/fetch-fonts.sh <folder>` downloads each file, with its license, from its upstream
source into `<folder>`, laid out as shown, and checks its SHA-256; then set `KITION_FONTS_DIR` to
that folder (the default is a `kition-fonts` folder beside this repository: a checkout of
[kition-fonts](https://github.com/KitionAI/kition-fonts), which has the same layout and these files
among others). The headless executable embeds them at build time (`kition-table-fonts`, feature
`embed`); without them it builds with a warning and its previews have no text
(`KITION_FONTS_REQUIRED=1` makes that an error). The picture and preview tests read them and fail
without them.

| Path under `$KITION_FONTS_DIR` | Font | License | Download |
|---|---|---|---|
| `fonts/inter/Inter-Variable.ttf` | Inter | OFL-1.1, `fonts/inter/OFL.txt` | <https://github.com/google/fonts/raw/45e3d5269e4a17d823f6d84470bc084dc5ae46f6/ofl/inter/Inter%5Bopsz%2Cwght%5D.ttf>, license `ofl/inter/OFL.txt` at the same commit |
| `fonts/droid-sans-fallback/DroidSansFallbackFull.ttf` | Droid Sans Fallback (Chinese and Japanese) | Apache-2.0, `fonts/droid-sans-fallback/LICENSE` and `NOTICE` | <https://android.googlesource.com/platform/frameworks/base/+/cac6285b48e88e118f69992d5c97d56163b6274c/data/fonts/DroidSansFallbackFull.ttf>, notice: the `NOTICE` file in the same folder; license: upstream has none there, so `LICENSE` (the Apache License 2.0 text) comes from <https://github.com/KitionAI/kition-fonts/raw/66949b770d1132d26284417368e154e8e5ae1acf/fonts/droid-sans-fallback/LICENSE> |
| `fonts/nanum-gothic/NanumGothic-Regular.ttf` | Nanum Gothic (Korean) | OFL-1.1, `fonts/nanum-gothic/OFL.txt` | <https://github.com/google/fonts/raw/45e3d5269e4a17d823f6d84470bc084dc5ae46f6/ofl/nanumgothic/NanumGothic-Regular.ttf>, license `ofl/nanumgothic/OFL.txt` at the same commit |
| `fonts/noto-color-emoji/Noto-COLRv1.ttf` | Noto Color Emoji (emoji, COLRv1) | OFL-1.1, `fonts/noto-color-emoji/OFL.txt` | <https://github.com/googlefonts/noto-emoji/raw/1ffdd21391dd1f25c081fa93a9dea0c7c029442b/2D/fonts/Noto-COLRv1.ttf>, license `2D/fonts/LICENSE` at the same commit |

The files are renamed to the paths in the first column (googlesource serves a file's raw bytes
base64-encoded with `?format=TEXT`: `curl -sL '<url>?format=TEXT' | base64 -d`). Their SHA-256
sums:

```text
29160a80ff49ddcab2c97711247e08b1fab27a484a329ce8b813d820dc559031  fonts/inter/Inter-Variable.ttf
acb6440a713d880a13a21b468ba7cd43f5a2b2934972e51be791c880730777b8  fonts/droid-sans-fallback/DroidSansFallbackFull.ttf
76f45ef4a6bcff344c837c95a7dcc26e017e38b5846d5ae0cdcb5b86be2e2d31  fonts/nanum-gothic/NanumGothic-Regular.ttf
b8e25ea68db82f9e4d0aee921f4420be2be39887bd5c893a2ad98710531f9d0c  fonts/noto-color-emoji/Noto-COLRv1.ttf
```

## Build the engine and run the tests

```sh
cargo build -p kition-table                         # the headless executable
cargo test -p kition-table-kit --features headless  # the engine: commands, checks, the contract
cargo test -p kition-table                          # the headless kit and the agent's tools
cargo test -p kition-table-file -p kition-table-api -p kition-table-import -p kition-table-ai
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

`cargo test --workspace` runs every crate's tests at once. The integration tests are separate
files (`bindings/wasm/tests/`, `apps/kition-table/tests/`); `--test <name>` runs one of them.
`KITION_TABLE_WRITE_COMMANDS=1 cargo test -p kition-table-kit shipped_commands_file` rewrites
`agent/commands.json` after a command changes.

On a developer machine, run only the fast checks (`cargo fmt --all -- --check`, `cargo check -p
<touched crates> --all-targets`, the wasm `cargo check`, `pnpm typecheck` in `web/`). Clippy,
`cargo test --workspace` and every `test:*` script of `web/` run in the kit's CI run on GitHub
(`.github/workflows/ci.yml`, which calls Kition's reusable `kit-ci.yml`); a change is done when
the CI run of HEAD passes and HEAD is on `origin/main` (Kition's `scripts/kit-ci.sh`).

The cases under `cases/` are run by kition-sdk's case runner; eval cases (`cases/table.agent.*`)
have a real model edit a table through the kits' MCP server and are started only by a maintainer
(docs/quality.md). Check them without a model from a kition-sdk checkout beside this repository:

```sh
node ../kition-sdk/tools/eval-cases.mjs --repo . --dry-run   # each case fails before and passes after its solution
```

`kit.json` is checked with kition-sdk's `tools/check-manifest.mjs`.

Read [CONTRIBUTING.md](../.github/CONTRIBUTING.md), [AGENTS.md](../AGENTS.md) and
[quality.md](quality.md) before a change: tests are the spec, every bug fix starts with a failing
case under `cases/regressions/`, and every feature has an entry in `features.yaml`.

## Build and run the editor

```sh
cd web
pnpm install
pnpm run engine      # builds bindings/wasm into web/public/engine and copies the locked fonts into web/public/fonts
node scripts/sample.mjs   # web/public/sample/Sample.ktable, which dev.html opens (pnpm test, test:flow and test:modes write it first)
pnpm build           # type check and the package in web/dist
pnpm test            # the built engine from Node
pnpm test:unit       # the editor's own specs (Vitest, jsdom)
pnpm run test:flow   # browser flows in Chromium and WebKit (Playwright; run with no HTTP proxy set)
```

The flow tests need Playwright's browsers (`pnpm exec playwright install chromium webkit`).

The editor's grid is the engine's: the engine arranges the view, lays it out and draws it in bands,
which the page plays on a canvas (`web/src/editor/table/canvas/`, shown by
`components/TableEditorGridView.tsx`); the editor holds no records for it. The grid also has a
development page, `web/dev/grid.html` (`pnpm dev`, then
`/dev/grid.html?file=<a .ktable URL>&view=<id>`; `hud=1` shows its frame and band timings), which
is not part of `pnpm build` and opens a file in the engine without the editor. Its interaction (selection,
keyboard, hover, the row head controls, menus' entry points) and its editing (the cell editors,
the clipboard, the drags, attachment pictures and uploads, the AI overlay; writes through the
engine's commands, each logged to `window.__ktableGridWrites`, after which the view follows with
`gridSync` and fetches again only the written records' bands) work there; it stands in for the editor's
other handlers and logs each call to `window.__ktableGridEvents`. The browser flows
(`tests/flow.mjs`) drive the same grid inside the editor. It draws with the same font
files the engine shapes with: `pnpm fonts` copies them from `KITION_FONTS_DIR` into
`web/public/fonts/` and refuses any whose SHA-256 is not in `web/fonts.lock.json`; `pnpm build`
packages them from there. Its tests, with no HTTP proxy set:

```sh
pnpm run test:grid    # text widths and screen == picture, in Chromium and WebKit
pnpm run test:grid:interaction   # selection, keyboard, hover and menus, in Chromium and WebKit
pnpm run test:grid:editing   # editors, clipboard, drags, pictures, AI overlay, in Chromium and WebKit
pnpm run test:grid:fonts   # the engine's lazy fonts: the first frame waits only for Inter, in Chromium and WebKit
pnpm run test:perf    # scrolling 100,000 records (writes target/perf/ on first run; minutes)
PERF_TIER=strict PERF_HEADED=1 pnpm run test:perf   # the lines themselves, on a maintainer's machine
node tests/perfEdit.mjs chromium   # 50 edits, then a whole column cleared, the clear undone and redone, the column pasted and filled, then a record appended, the column cleared and both undone, on the same 100,000 records
node tests/restAfterJump.mjs chromium   # the scroll bar dragged to the end in slow frames: the view rests with its exact bands
```

`test:perf` checks the lines of `cases/table.editor.grid/scroll-perf/expected/lines.json` (relaxed
by its `relax` in the default `ci` tier). On a GitHub-hosted runner, where GitHub sets
`RUNNER_ENVIRONMENT=github-hosted` in every job (a self-hosted runner has `self-hosted`), it still
measures every line and prints each number against its line, but the lines that depend on the
runner's speed (frame intervals, content play, skeleton frames and skeleton runs, band
generation) are informational: one over its line prints `INFO` and does not fail the job. The
lines that do not depend on its speed (the axes' size, the bytes transferred, the steady scrolls'
prefetch plan and prefetch ahead, page errors) still fail there. `PERF_SPEED_LINES=fail` or
`PERF_SPEED_LINES=report` forces either behaviour anywhere. A shared runner's speed varies from
run to run, so the speed lines are checked where the speed is known: on self-hosted runners and on
the owner's machine, where the strict tier (`PERF_TIER=strict PERF_HEADED=1 pnpm run test:perf`)
runs before a release.

The grid's text is measured by the engine and only drawn by the page, so the engine must set
Inter's optical size (`opsz`) as the page's Canvas does. Chromium's Canvas follows the font size
(as CSS text does everywhere); WebKit's Canvas (Playwright's WebKit on Linux, and WebKitGTK by
the same engine) keeps Inter's default optical size, 14, at every size. Once Inter is loaded the
grid measures a probe at 14 and 32 px (`canvasOpticalSizing` in `canvas/gridFonts.ts`) and, when
its Canvas keeps the default, tells the engine (`gridOpticalSizing fixed-default`) before it asks
for any band; the bands, drafts, `gridMeasure` and `gridRaster` then use the default optical size.
The headless picture, previews and exports always follow the font size. The grid's own text is 10
to 13 px, under the axis's floor, where both settings are the same; `tests/widths.mjs` checks 16
and 20 px too and prints what the page found.

The editor depends on kition-sdk's `@kition/sdk` and Kition Tessera's `@kition/tessera` from npm,
at the versions the committed `web/pnpm-lock.yaml` pins. To work against local checkouts instead,
clone `kition-sdk` and `kition-tessera` beside this repository (or set `KITION_SDK` and
`KITION_TESSERA`), run `pnpm install` in each of them and build the two packages (their exports
are the built `dist/`: `pnpm --filter @kition/sdk build` in kition-sdk, `pnpm build` in
kition-tessera's `packages/tessera`), and then run `KITION_LINK=1 pnpm install` here:
`web/.pnpmfile.cjs` links them. The linked install rewrites `web/pnpm-lock.yaml`;
do not commit that. A plain `pnpm install` (or `pnpm install --frozen-lockfile`) always installs
from npm, even when the checkouts are there.

The template pictures under `web/public/templates/` are rendered by `web/scripts/render-covers.mjs`
and `web/scripts/render-receipts.mjs` (see [attribution.md](attribution.md)); they are tools,
not tests.

The README's screenshots are taken by [`web/scripts/readme-shots.mjs`](../web/scripts/readme-shots.mjs)
from the development host; it is a tool, not a test. `assets/readme/made-1.png` … `made-4.png` are
the `thumb.png` of four kition-showcase samples (`table/b2b-sales-pipeline`,
`table/garden-brand-content-calendar`, `table/camera-gear-rentals`, `table/family-recipe-box`),
copied as they are.

## Native hosts

`bindings/wasm` also builds as a native Rust library. Depend on this kit's SDK crate and
import its re-exported `KitDocument` contract; a host does not need to match a separate
`kition-sdk` Git revision. The table SDK also exports its `Origin` type for host writes.
The SDK constrains the Loro facade and internal implementation to their compatible versions,
because a consuming workspace does not inherit the kit's lockfile.

```rust,ignore
use kition_table_kit::{KitDocument, Origin, TableDocument};
let mut document = TableDocument::create("contacts.ktable", 0, 0)?;
document.execute_as("table.tables.list", serde_json::json!({}), &Origin::system("system:host"))?;
let bytes = document.save(None)?;
```

## Layout

| Path | What |
|---|---|
| `kit.json` | The kit manifest (KHP 1.0 and Kition platform fields) |
| `crates/engine/file` | `kition-table-file`: the `.ktable` file, SQLite holding the document's KDM replica and its relational projection (derived values, history, blobs, dashboards) |
| `crates/engine/api` | `kition-table-api`: the request layer, `handle(file, method, params)` |
| `crates/engine/time` | `kition-time`: millisecond timestamps that build for wasm32 |
| `crates/engine/arrange` | `kition-table-arrange`: the view model, which records a view shows and in what order (filter, sorts, groups, search), kept up to date edit by edit: the editor's views (extras `gridOpen`, `gridSync`, `gridHits`, `gridSummary`, `gridStats`), the agent's `read` and the picture |
| `crates/engine/ai-field` | `kition-table-ai`: AI fields' prompts and results; the host runs the model (`host/generate`) |
| `crates/formats/import` | `kition-table-import`: CSV, TSV and XLSX import with type inference |
| `crates/view/*` | The picture and the grid: fonts (`fonts`), the grid's layout (`grid`), its display list and rasterizer (`display`), the whole picture (`picture`) |
| `bindings/wasm` | `kition-table-kit`: `TableDocument` (`KitDocument`, engine contract 2, extras) |
| `apps/kition-table` | `kition-table`: the engine served headless over the Kit Protocol, with the object model |
| `web/` | The editor on `@kition/sdk/editor`: the table editor, its UI primitives, theme, strings (`web/locales/<locale>/`) and starter templates |
| `model/schema.json` | The document's KDM schema |
| `agent/` | The agent guide, the command list and the check catalogue |
| `cases/`, `features.yaml`, `conformance/` | The feature catalogue, its test cases, and one fixture per check code |
| `docs/` | The writing format, gaps and roadmap; the engine commands in `docs/features/` |

## How it fits together

- **One engine.** The engine is Rust. It implements the Kition SDK's `KitDocument` contract on the
  SDK's Kit Data Model: a `.ktable` file holds the document's replica and a relational projection
  of it in SQLite.
- **In the page.** The editor (`web/`) is built on the SDK's `@kition/sdk/editor`. The engine runs
  as WebAssembly in a Web Worker; hosts embed the editor in a sandboxed frame over the Kit UI
  protocol, and name a table or dashboard to open with a view (`document.view` when the editor
  opens, `document/reveal` after): `{ "path": "tables/<id>" }` or `{ "path": "dashboards/<id>" }`.
- **Headless.** `kition-table` speaks the Kit Protocol of the
  [Kit Harness Protocol (KHP)](https://github.com/kitharness/kitharness); the SDK's `kition-kits-mcp`
  serves it to agents.
- **Offline.** The engine never reaches the network: AI fields build their prompts and apply
  results in the engine, and the model call is the host's.

## Publish

Kition ships the kit's package as npm packages: `@kition/kit-table-ui` (the editor) and
`@kition/kit-table-<platform>` (the headless program). They are built on GitHub, never on a
developer machine: the Package workflow (`.github/workflows/package.yml`, which calls Kition's
reusable `kit-package.yml`) builds the commit it is started on, with `gh workflow run package.yml
-R KitionAI/kition-table`, and publishes what npm lacks of that version. Kition pins it with
`node scripts/pin-kits.mjs table` (see `docs/kits.md` there). A version is published once: bump `version` in `kit.json` to publish a
change.
