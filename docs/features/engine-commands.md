# Engine commands beyond the request layer

`TableDocument` answers every `kition-table-api` method as the command `table.<method>` (and the
extra `call(method, params)`), each answering `{ "result", "change" }`. This page describes the
engine's own methods: file import, AI fields without the model call, the records changed since a
version, and writes as a host feature (`asOrigin`). `result` is what is described below; `change`
is the `TableChange` the request made (`null` for reads), with its `origin`.

| Method | Reads only | Writes as |
| --- | --- | --- |
| `import.preview` | yes | - |
| `import.run` | no | `import:import` (Kition's import origin): never an undo step |
| `ai.prepare` | yes | - |
| `ai.apply` | no | `system:aifield` (Kition's AI field origin): never an undo step |
| `changes.since` | yes | - |
| `read` | yes | - |
| `preview.show` | yes | - |

Ids are numbers or decimal strings. Errors are the engine's error messages (as the runtime's `400`
answers worded them).

## Schemas for agents

Every command carries its parameters as a KHP `ParameterSchemaSpec` (`KitCommand.schema`, KHP
A5 and A6), so the headless kit lists all of them to agents with typed parameters, and
`agent/commands.json` (beside `kit.json`) holds, per command, its tool name (A2), its
`inputSchema`, and its `outputSchema` as JSON Schema (section 8 of KHP's `spec/agent.md`). Both
are generated in `bindings/wasm/src/agent.rs`:

- Parameters come from the Rust types the requests deserialize into (`schemars`): the ids from
  the route template in `METHODS` (`tableId`, `viewId`, ...; integers, required), the query
  values the route reads, and the body from `kition-table-api`'s and `kition-table-import`'s
  request types. The bodies a handler reads key by key (`tables.update`, `fields.update`,
  `views.update`, `dashboards.upsert`) are described by types in `agent.rs` that list exactly
  the keys the handler reads. The AI field methods and `changes.since` read loose JSON and are
  written out there. Mutating commands also list `asOrigin`, which agents leave out.
- Answers are `{ result, change }` or, when the command fails, the A18 error
  `{ error: { code, message, index?, retryable? } }`. The output schema requires none of the
  three, so it accepts both. `change` is `TableChange` (its own type) with `documentId` and
  `origin`, or `null`; `result` describes the top-level shape of the route's answer.

`agent/commands.json` is checked by a test; after changing a command or a request type, run
`KITION_TABLE_WRITE_COMMANDS=1 cargo test -p kition-table-kit shipped_commands_file`. The tests in
`bindings/wasm/src/agent_tests.rs` run every command on a real document and validate its params,
its answer, and its error results against the schemas.

## `import.preview`

Params: the preview request of `data-import.schema.json` (`source?`, `sheet?`) with the file in
the params instead of a multipart upload or a workspace read:

```json
{ "fileName": "people.csv", "content": "<base64 of the bytes>", "sheet": "", "source": { "kind": "workspace", "workspace_path": "data/people.csv" } }
```

`source` is optional: with `kind: "workspace"` the preview names that workspace path (the host
read the bytes); otherwise it is an upload named by `fileName`. CSV and TSV must be UTF-8; XLSX
is read with its sheets; at most 100 MB.

Answer: `DataImportPreviewResponse`:

```json
{
  "import_token": "sha256:<hex of the bytes' SHA-256>",
  "source": { "kind": "upload", "upload_name": "people.csv" },
  "filename": "people.csv", "format": "csv", "encoding": "utf-8", "delimiter": ",",
  "row_count": 2, "field_count": 2,
  "fields": [{ "index": 0, "title": "Name", "type": "text", "nullable": false, "sample_values": ["Ada", "Grace"] }],
  "sample_rows": [["Ada", 10]], "warnings": [], "sheets": []
}
```

`selected_sheet` and `sheets` describe a workbook. `expires_at` is left out: the kit keeps no
previews.

## `import.run`

Params: `DataImportExecuteRequest` with the file again:

```json
{
  "fileName": "people.csv", "content": "<base64>", "sheet": "<the preview's selected_sheet>",
  "target": { "kind": "existing_table", "table_id": 3 },
  "write_mode": "append", "schema_mode": "auto",
  "unique_by": ["Name"], "field_overrides": [{ "index": 1, "title": "Points", "type": "number" }],
  "import_token": "sha256:…"
}
```

- `target.kind`: `existing_table` (`table_id`), `new_table` (`table_title`), or `new_document`
  (`table_title`; a new table in the open document, its `path` unused: the kit holds one
  document). `document_id` is not checked.
- `write_mode`: `append`, `replace` (the table's records are deleted first), or `upsert`
  (`unique_by` field titles or names; rows without a key are skipped).
- `schema_mode`: `auto` (a starter table's default fields are replaced), `preserve` (missing
  columns become new fields), or `replace`.
- `import_token` is optional; when given it must be the bytes' token. `idempotency_key` is
  accepted and unused.

Answer: `DataImportResult`, synchronously (no job):

```json
{ "document_id": 1, "table_id": 3, "path": "Tasks.ktable", "rows_total": 2, "rows_created": 2, "rows_updated": 0, "rows_skipped": 0, "fields_created": 0, "fields_updated": 0, "warnings": [] }
```

One `import.run` is one engine request. New records are written 500 per write. An import that
fails leaves the document as it was.

## `ai.prepare`

Params: the `.../ai/run` routes' body with their path ids:

```json
{ "tableId": 1, "fieldId": 4, "recordId": 7 }
{ "tableId": 1, "fieldId": 4, "record_ids": [7, 8], "limit": 100, "action": "customize", "config": { "type": "customize", "prompt": "…" } }
```

`recordId` runs one cell; otherwise `record_ids` (or `recordIds`) are the batch: distinct positive
ids, at most `limit` and 100. `action` and `config` carry a per-record prompt; they must match the
field's configured action. `scope`, `force`, and `runtime_model` are accepted and unused, as the
runtime left them.

Answer:

```json
{
  "fieldId": 4,
  "output": "text",
  "model": "<the field's ai_config.runtime_model, when set>",
  "requests": [{
    "recordId": 7,
    "messages": [{ "role": "user", "parts": [
      { "type": "text", "text": "You are filling a single cell…" },
      { "type": "image", "hash": "<sha256 of a blob held in the file>", "mime": "image/png" },
      { "type": "image", "url": "https://…", "mime": "image/jpeg" }
    ] }],
    "options": { "count": 3, "size": "1024x1024", "width": 1024, "height": 1024, "quality": "low", "useCase": "inline_illustration" }
  }],
  "skipped": [{ "recordId": 8, "reason": "Source field is empty" }]
}
```

`output` is `image` for `image_generation` and `image_customization` fields; `options` comes
with image requests only. The source cell's images (at most four) are blobs held in the file
(read them with `attachments.read`) or `data:`/`http(s):` URLs. A batch skips the records that
cannot run; one cell that cannot run is an error.

## `ai.apply`

Params:

```json
{
  "tableId": 1, "fieldId": 4, "recordId": 7,
  "results": [{ "recordId": 7, "text": "…" }, { "recordId": 8, "images": [{ "mime": "image/png", "content": "<base64>" }] }, { "recordId": 9, "error": "…" }]
}
```

A result carries `text` (text fields), `image` or `images` (image fields; stored as blobs held in
the file), or `error`. `action` and `config` are as for `ai.prepare`.

Answer, with `recordId` (one cell): `RunDataAIFieldCellResponse`, `{ record, field, state }`; a
failed result is an error. Without it: `RunDataAIFieldBatchResponse`:

```json
{ "items": [{ "record": { … }, "state": { … } }], "field": { … }, "accepted": 2, "completed": 1, "failed": 1, "errors": [{ "record_id": 9, "message": "…" }], "limit": 100 }
```

`state` is `{ status: "done", source_hash, config_hash, model_id: 0, updated_at, error: "" }`.

## `read`

The records of one table, or of one view, a page at a time (KHP A24a, A25): what `inspect()` lists
of each table's first records, for the rest. Params: `{ "tableId" | "viewId", "limit"?: 50,
"cursor"? }`. A table's records come in order; a view's are the ones its filter keeps, in its order
(groups, then sorts), as the editor shows them. `limit` is 1 to 200; `cursor` is the `next` cursor
of a previous answer (the number of records before the page, as a string).

Answer: `{ "tableId", "viewId"?, "columns", "rows", "from", "to", "total", "next"? }`. `columns` are
the field names (the keys of `values` in `records.create` and `records.update`); each row is `{ id,
cells }`, the record id and one value per column (`null` when empty), a link's records as `{ id,
display }` by record id, text over 200 characters cut and ending in `…`. `from` and `to` count the
listed records from 1 among `total`; `next` is the step that lists the records after them
(`{ "command": "file.read", "params": { …, "cursor" } }`), absent after the last.

`inspect()` answers each table as `{ id, name, title, label, primaryFieldId, recordCount, fields,
views, records }` (`records` as `read` answers it, at most 20 a table and 100 in all), and the
dashboards as `{ id, title, label, widgets, sourceTableId? }`. Hosts that relay `inspect()` over the
Kit Protocol leave out every `title` key, so `label` carries each title to agents.

## `file.outline`

The document's outline, and the manifest's `outline.command` (Kition platform K24): the parts of
the file a host lists under it and opens the file at. It takes no parameters, reads only, and like
`file.export` answers at the top level, not as `{ "result", "change" }`, since the host reads
`items` there: the tables, then the dashboards, each in the document's order, as
`{ "id", "title", "kind", "view" }`. `kind` is `table` or `dashboard`; a table without a title goes
by its name; `view` is the place as the editor takes it in `document.view` and `document/reveal`
(`{ "path": "tables/<id>" }`, `{ "path": "dashboards/<id>" }`, `web/src/shown.ts`). A host needs
nothing else to list a table file's parts: it does not read `table.list` or
`dashboard.list` for that.

## `file.render`

The agent's preview (KHP A39), and the manifest's `agent.render` (Kition platform K21): the
document's picture, what `render` draws, as PNG. Like `file.export` it answers at the top
level, not as `{ "result", "change" }`, since the kits server reads `png` there. The picture is
what the editor shows (`crates/view/picture`): the tab bar (the tables and dashboards), the shown
table's view bar (its views, and what the shown view filters, sorts and groups by), then the view:
a grid in the editor's geometry (each field's type icon and title, the values drawn by type: choice
chips in their tones, checkboxes, rating stars, links, people, dates in the field's format, group
header rows, add rows), laid out by `crates/view/grid` as a display list and rasterized by
`crates/view/display`; a kanban board, a gallery or a calendar list; or a dashboard's widgets
(numbers, bar, line and pie charts, tables). Text is drawn in the faces the host registers with
`addFont` (Inter, Droid Sans Fallback, Nanum Gothic and Noto Color Emoji; see
[development.md](../development.md#fonts)); a character none of them covers is a box. The headless executable embeds
them at build time (`kition-table-fonts` feature `embed`, from `KITION_FONTS_DIR`). Without them
the picture has no text.

Params: `{ "target"?, "slide"?, "scale"?: 1, "maxSize"?: 4096 }`. `target` is a diagnostic's target
as `table_check` answered it, or a part: `path` `tables/<id>` (the table as the editor opens it),
`tables/<id>/views/<id>` (a view, the records it shows), `tables/<id>/fields/<id>`,
`tables/<id>/records/<id>` or `tables/<id>/records/<id>/fields/<id>` (every field and record of the
table, in order, the field, record or cell outlined and the picture framed around it), or
`dashboards/<id>`; or `sheet`, a table's title, without `path`; with `rows: [from, to]`, only the
records at those positions (counted from 1, as the grid numbers them; the footer says
`3–5 of 12 records`). Without `target`, `slide` is the table at that position (counted from 0), a
host's part preview; without either, what `preview.show` picked. `scale` is above 0 and up to 2;
the picture shrinks so its longer side is at most `maxSize` (1 to 4096).

Answer: `{ "png": "<base64>", "mimeType": "image/png", "width", "height", "scale", "tableId",
"viewId" }`. An unknown table, view, field, record, or dashboard, or rows outside the records
shown, is a `not found` error. `conformance/expected/` holds the expected previews of
`conformance/basic.ktable` (`bindings/wasm/tests/preview.rs`, and through the headless kit with
its embedded faces, `apps/kition-table/tests/render.rs`).

## `preview.show`

Picks what the document's picture shows: `{ "tableId"?, "viewId"?, "dashboardId"? }`, a table with
its first view, a view (its table is shown), or a dashboard; `{}` shows what the editor opens (the
first table's first view). It changes no data and is not saved.

Answer: `{ "tableId", "table", "viewId", "view", "viewType", "dashboardId", "records" }` (for a
dashboard, `dashboardId` and `dashboard`). An unknown table, view or dashboard is a `not found`
error.

## `changes.since`

Params: `{ "version": "<base64 of a version the engine answered>" }`; `""` means from the start.

Answer:

```json
{
  "version": "<base64 of the current version>",
  "created": [{ "tableId": 1, "recordId": 7, "origin": "person:kit" }],
  "updated": [{ "tableId": 1, "recordId": 7, "fieldIds": [3], "origin": "system:workflow" }],
  "deleted": [{ "tableId": 1, "recordId": 8, "origin": "agent:turn-1" }]
}
```

Each write since the version is reported with its origin: `person:<principal>` (the editor's
person is `person:kit`), `agent:<principal>`, `extension:<principal>`, `import:import` (a file
import), `import:sync` (a co-editor's changes), or `system:<name>` (`system:aifield`,
`system:create`, a host feature's `asOrigin`). A record created and then updated is listed in
both, in write order; repeated updates by one origin are merged. A link edit updates the linking
record in the link field. Writes older than the last 1,000 the file remembers are reported as
`system:unknown`. A version the document never reached is an error.

## `asOrigin`

Any mutating `table.*` command takes `"asOrigin": "system:<name>"` (`<name>`: letters, digits,
`-`, `_`, `.`, at most 64): the request writes as that `system` origin, never an undo step, so
the person's undo does not revert it and `changes.since` reports it. Any other origin is refused,
and so is `asOrigin` on `undo` and `redo`. Host features (workflows) write this way.
