# Table writing format

How a model writes Kition Table documents: the steps it sends, how it names what it changes,
what a new document starts with, the named values commands accept, and worked examples.
The package ships the short version for models as [agent/guide.md](../agent/guide.md); this file is
the complete reference it is drawn from. Everything below comes from the engine itself
(its 55 commands and their parameter schemas, and what `inspect` reports) and from the guide.

## What a model writes

A model never writes the file bytes. It calls the kit's agent tools (prefix `table`), and the
engine applies each change, keeps it in undo history and saves the file:

| Tool | Use it to |
| --- | --- |
| `table_inspect {path}` | See the file: its tables and dashboards, and a preview |
| `table_commands {path, query}` | Find commands by keywords (`record`, `field`, `view`, `filter`, `sort`, `dashboard`). At most 12 answers; add words when it says "showing 12" |
| `table_run {path, steps}` | Run 1 to 50 steps `{command, params}` in order; it stops at the first failure and saves after the last step |
| `table_query` / `table_batch` | Read or write `records`, `fields`, `views` through the object model |
| `table_check {path}` | List problems: values that are not one of a select's options, empty required fields, duplicate primary values, broken links and lookups, formula errors, dashboards on missing tables |
| `table_create {path}` | Make a new table file, holding the starter table (below) |

## Syntax

A step is one engine command with its parameters, as JSON:

```json
{"command": "<command id>", "params": {"<name>": <value>}}
```

`table_run` takes a list of steps and runs them in order, stopping at the first failure. Parameter
names, types and required fields come from each command's schema; `table_commands` returns them.
Commands by namespace (the part of the id before the first dot):

| Namespace | Commands | Change the document | Open to the agent | Examples |
| --- | ---: | ---: | ---: | --- |
| `edit` | 2 | 2 | 2 | `edit.undo`, `edit.redo` |
| `table` | 52 | 31 | 52 | `file.get`, `table.list`, `table.add` |

### How things are addressed

- **Everything has a numeric id**, from 1, unique within its kind: `tableId`, `fieldId`, `recordId`,
  `viewId`. Read them with `table.list` (tables with their fields and views),
  `field.list {tableId}`, `record.list {tableId, limit?, offset?}` and
  `tableView.list {tableId}`. Never guess an id; a record id that does not exist fails with
  `no rows in result set`.
- **Record values are keyed by the field's `name`**, not its `title`: a field titled `Status` is
  written as `{"status": "Done"}`. A key that names no field of the table is refused with
  `invalid_params`, one `data.errors` entry per key at `/values/<key>` (in `edit.transact`,
  `/ops/<i>/records/<j>/values/<key>` for `cells.set` and `/ops/<i>/records/<j>/<key>` for
  `records.create`), and the call changes nothing. When the key is a field's title, or its name
  in another case or with spaces for underscores, the message names the field:
  `` `values.Primary channel` names no field of table 2; the field titled "Primary channel" is named `primary_channel` ``.
- **A `link_to_record` value** is a list of the linked records' row keys or record ids
  (`{"members": [3, 4]}`), in `record.add`, `record.set` and `edit.transact`;
  records read it back as `{row_key, display}` objects, and `null` or `[]` unlinks. A link field
  whose `symmetric_field_id` names the target table's link field back shows the links made from
  either side.
- **Filters and sorts refer to fields by `field_id`**, not by name.
- `record.list` pages with `offset` (from 0) and `limit`; it answers `total`.
- Leave `asOrigin` out of every call: it is for host features, not agents.

## Text writing format

Besides single commands, the engine takes whole text in one step:

- `file.importCsv` `the params of the data-document route POST /tables/{tableId}/import/csv: its ids (tableId, …), query values, and body fields; asOrigin? (system:<name>) writes as a host feature, outside the undo history`: Adds one record per row of CSV text to an existing table; the header row names fields by title or name, and other columns are left out.
- `file.runImport` `the data-import execute request (its destination table, write_mode, schema_mode, unique_by?, field_overrides?, import_token?) with fileName, content (base64 of the bytes), and sheet?; asOrigin? (system:<name>) writes as a host feature`: Imports a file given as base64 into a new or existing table, with write mode, schema mode, unique key and field overrides.

## Defaults

What a new document starts with:

- File name `Untitled table.ktable`, size 640 × 420 (`kit.json` `creates`).
- One starter table, as a new table in the editor has: table 1 `Table` (name `table`) with fields
  `title` (text, primary), `status` (single_select: `To do`, `In progress`, `Done`) and `notes`
  (long_text), one grid view `All`, and no records. A host that builds its own schema deletes it
  with `table.delete {tableId: 1, asOrigin: "system:<name>"}` (see
  [agent/guide.md](../agent/guide.md), New files).

What `table_inspect` reports for a new document made at that size (generated from the engine; lists shortened to 20 entries):

```json
{
  "canUndo": false,
  "canRedo": false,
  "readOnly": null,
  "tables": [
    {
      "id": 1,
      "title": "Table"
    }
  ],
  "dashboards": []
}
```

## Themes, layouts and other named values

Parameters that take one of a fixed list of names (themes, layouts, presets, styles, kinds, modes,
units and similar), from the command schemas. A parameter not listed here takes free values; its
schema says which.

The command schemas declare no fixed name lists; values are described in each parameter's text.

## Examples

### Example 1: add a record and update another

```json
{"tool": "table_run", "arguments": {"path": "crm.ktable", "steps": [
  {"command": "table.list", "params": {}}
]}}
```

The answer lists table 1 with fields `title` (text, primary), `status` (single select:
`To do`, `In progress`, `Done`) and `notes` (long text), and view 1 `All`. Then:

```json
{"tool": "table_run", "arguments": {"path": "crm.ktable", "steps": [
  {"command": "record.add", "params": {"tableId": 1, "values": {"title": "Book the venue", "status": "To do"}}},
  {"command": "record.set", "params": {"tableId": 1, "recordId": 1, "values": {"status": "In progress", "notes": "Draft shared on Monday"}}}
]}}
{"tool": "table_check", "arguments": {"path": "crm.ktable"}}
```

The first step answers the new record with `"id": 3`; both steps answer `"changed": true`, and the
check finds nothing wrong.

### Example 2: a number field and a filtered, sorted view

```json
{"tool": "table_run", "arguments": {"path": "crm.ktable", "steps": [
  {"command": "field.add", "params": {"tableId": 1, "name": "priority", "title": "Priority", "type": "number"}},
  {"command": "record.set", "params": {"tableId": 1, "recordId": 2, "values": {"priority": 2}}},
  {"command": "tableView.add", "params": {"tableId": 1, "title": "Open work", "type": "grid"}}
]}}
```

The field is answered with `"id": 4`, the view with `"id": 2`. Show only what is not done, by title:

```json
{"tool": "table_run", "arguments": {"path": "crm.ktable", "steps": [
  {"command": "tableView.setFilterTree", "params": {"tableId": 1, "viewId": 2,
    "tree": {"conjunction": "and", "children": [{"condition": {"field_id": 2, "operator": "neq", "value": "Done"}}]}}},
  {"command": "tableView.setSorts", "params": {"tableId": 1, "viewId": 2, "items": [{"field_id": 1, "direction": "asc"}]}}
]}}
{"tool": "table_check", "arguments": {"path": "crm.ktable"}}
```

Both steps answer the stored tree and sorts, and the check finds nothing wrong.

## Workflow

1. `table_inspect`, then `table_run` with `table.list` to learn the ids, field names and types.
2. `table_commands` with keywords to find a command; use only the ids it returns.
3. `table_run` with a few steps.
4. `table_check`, and fix what it reports. Many diagnostics carry a `fix` you can run as a step.

## Pitfalls

- A single-select value must be one of the field's `options.choices`. Writing another value
  succeeds, and `table_check` then warns `table.select-option-missing`, with a `fix` that adds the
  choice. Either add the choice first (`field.set {tableId, fieldId, options}`) or use an
  existing one.
- Formula fields are computed: write their `formula`, never their values.
- A record's `values` holds only the fields that have a value; a missing key means empty.
