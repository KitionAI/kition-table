# Changelog

Changes to Kition Table, newest first. A version is the kit package version in `kit.json`;
each package is published with a `package-v<version>` tag.

## 0.5.0

- The editor's grid is rewritten: the engine arranges the view, lays it out and draws it in bands
  (the same display list the headless picture rasterizes), and the page plays the bands on a
  canvas with the package's fonts; selection, the cell editors, the clipboard and the drags are the
  page's. The editor no longer reads the whole table to show a grid view (the kanban, calendar and
  gallery still read every record), and the footer's sums come from the engine. The old canvas
  grid, derived from Teable's `packages/sdk` grid, is deleted, and with it the Teable notice in
  NOTICE and its unused dependencies (react-hammerjs, react-hotkeys-hook, lodash-es, lru-cache,
  scroller, ts-key-enum, ts-keycode-enum).
- The package carries the grid's faces in `fonts/` (`web/fonts.lock.json`, copied from
  kition-fonts by `web/scripts/fonts.mjs`), which the page and the engine both load.
- Engine extras: `gridAxes` takes the view's config as the editor holds it (`config`), and
  `gridSummary` sums number fields over the editor's arrangement.

## 0.4.0

The first version with prebuilt headless executables: the Kition SDK release that carries this
version builds `kition-table` for macOS (arm64, x64), Linux (arm64, x64) and Windows (x64) from the
`package-v0.4.0` tag. Its number is above every version this repository has published before:
the 0.3.1 package of 17 November 2026 was withdrawn when the history was rewritten, and
0.1.0 stays published as it is. From this version on a published tag and its assets are never
deleted, moved or reused.

- `scripts/fetch-fonts.sh <folder>`: downloads the fonts the build needs from their upstream
  sources and checks each file's SHA-256, so a source build needs no private repository.

## 0.1.0

First public release of the source, under the Apache License 2.0. It contains:

- The Rust engine (`kition-table-kit`): the `.ktable` file (SQLite holding the document's KDM
  replica and its relational projection), tables with typed fields, records, links, lookups,
  rollups, formulas and AI fields, views with filters, sorts and groups, dashboards, attachments
  held in the file, undo and redo, CSV, TSV and XLSX import, CSV and Markdown export, and
  `table_check` with fix hints.
- The headless kit (`kition-table`) for agents over the Kit Protocol, with the object model of the
  document's tables, fields, views and records under ids that survive reopening, and writes with
  a `basis` that are refused when someone else changed what they rest on.
- The kit's editor (`web/`): grid, kanban, gallery and calendar views, dashboards, the field and
  view settings, imports, and starter templates, in kit-editor's frame.
- The picture the engine draws for thumbnails and the agent's preview, of a named table, view or
  dashboard.
- NOTICE credits the editor's canvas grid (`web/src/editor/table/grid/`), derived from Teable's
  `packages/sdk` grid (MIT, Copyright (c) 2023-2025 Teable, Inc.), and Loro (MIT), bundled by the
  editor and linked by the engine; docs/attribution.md lists which grid files are copied,
  modified or the Kition authors' own.
- A raw `list.move` in a `batch` names its destination as a gap in the list before the moved
  item is removed, as the guard reads it: to move the first of two items to the end, write
  `"from":0,"to":2`.
- Quality: a missing private tool (kition-server) is a skip, not a failure.
- Quality vocabulary: cases run in the layers `unit`, `gate`, `corpus` and `eval` (eval cases are
  run by kition-sdk's `tools/eval-cases.mjs`); cases and features name their `dimension`, a case's
  `source` names its `license`, and only a maintainer approves expected-result changes.
- Toolchain: Node 22.23.4, pnpm 10.34.9.

Known gaps are listed in [docs/gaps.md](docs/gaps.md).
