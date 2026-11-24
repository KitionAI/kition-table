# Table kit

The Table kit opens, edits and checks table files (`.ktable`). A file holds one or more tables with
typed fields (text, long text, number, rating, single and multi select, date, checkbox, URL,
attachment, user, link, lookup, rollup, formula, AI …), their records, views (grid, kanban, gallery,
calendar) with filters, sorts and groups, and dashboards. An agent can add and update records,
fields and views, filter, sort and group views, check the file for values that don't fit their
fields, broken links and lookups and formula errors, and export a table to CSV or Markdown.

## Tools

The kit's prefix is `table`. Its tools are:

| Tool | Does |
|---|---|
| `table_inspect` | The file as JSON: its tables with their fields, views, record counts and first records, and its dashboards (see [Reading the file](#reading-the-file)); a preview with `preview: true` |
| `table_query` | Reads `records`, `fields` and `views` of the object model by id, or by following fields from the root |
| `table_commands` | Finds commands by keywords (`record`, `field`, `view`, `filter`, `sort`, `dashboard`), each with its parameter schema. At most 12 answers; add words when it says "showing 12" |
| `table_run` | Runs 1 to 50 command steps `{command, params}` in order; it stops at the first failure and saves after the last step |
| `table_batch` | Runs one batch of object-model operations on `records`, `fields` and `views` |
| `table_check` | What is wrong with the file, as diagnostics |
| `table_create` | Makes a new `.ktable` file holding the starter table (see [New files](#new-files)) |
| `table_export` | Writes the file's first table to a new CSV or Markdown file |
| `table_draft_ready`, `table_draft_reopen` | Asks the person to review your draft, or takes it back to edit |

Every tool takes `path`, the table file's path in the workspace. With `draft: true` your edits go
to your draft of the file and leave the file as it is; only the person merges or discards a
draft.

## Addressing and coordinates

- **Use stable object IDs.** Tables, fields, views and records expose a UUID as `tool_id` in
  inspect and command reads. A record also exposes it as `row_key`. Commands, their nested
  references, `on`, table/view parts, queries, batches, receipts and conflicts use the same
  UUID. It survives rename, reorder, save and reopen. Dashboards already use their string `id`.
  Existing integer inputs remain supported as local projection addresses, counted from 1 and
  scoped by kind. They are declared as index addressing and hold the relevant list window;
  structural changes can refuse them as `relocated`. They are not stable tool IDs. The short
  numeric examples below assume a fresh read; prefer its `tool_id` in subsequent calls.
- **Record values are keyed by the field's `name`**, not its `title`: a field titled `Status` is
  written as `{"status": "Done"}`. A key that names no field of the table is refused with
  `invalid_params`, one `data.errors` entry per key at `/values/<key>` (in `transact`,
  `/ops/<i>/records/<j>/…`), naming the field when the key is its title; the call changes nothing.
- **Names.** A table's or field's `name` is lowercase ASCII letters, digits and `_`, starting with a
  letter and not ending with `_` (`^[a-z](?:[a-z0-9_]*[a-z0-9])?$`), unique among the document's
  tables or the table's fields. Give fields explicit names when you will write their values: an
  explicit name is kept as given, and one not in that form, or already taken (also by an earlier
  field of the same call), is refused with `invalid_params` at `/name` or `/fields/<i>/name`.
  Without one, the name is derived from the title: letters and digits kept and lowercased, every
  other run of characters made one `_`, `f_` before a leading digit, `field` when nothing is left
  (a title such as `电话`), then `_2`, `_3`, … when taken. Read derived names back from the answer.
- **A `link_to_record` value** is a list of the linked records' row keys or record ids
  (`{"members": [3, 4]}`), in [`table.records.create`](/kits/table/commands/table.records.create/),
  [`table.records.update`](/kits/table/commands/table.records.update/) and `transact`; records read
  it back as `{row_key, display}` objects, and `null` or `[]` unlinks. A link field whose
  `symmetric_field_id` names the target table's link field back shows the links made from either
  side.
- **Filters and sorts refer to fields by `field_id`**, not by name.
- [`table.records.list`](/kits/table/commands/table.records.list/) pages with `offset` (the number
  of records to skip, counted from **0**) and `limit`; it answers `total`.
- Leave `asOrigin` out of every call: it is for host features, not agents.
- **Field types** are the identifiers `text`, `long_text`, `number`, `date`, `datetime`,
  `single_select`, `multi_select`, `checkbox`, `url`, `attachment`, `document_link`, `formula`,
  `ai`, `rating`, `user`, `link_to_record`, `lookup`, `rollup`, `button`, `created_time`,
  `last_modified_time`, `created_by`, `last_modified_by` and `auto_number`; view types are `grid`,
  `kanban`, `calendar` and `gallery`. The `type` and `options` of
  [`table.fields.create`](/kits/table/commands/table.fields.create/) and
  [`table.tables.create`](/kits/table/commands/table.tables.create/) describe each type and the
  options it reads (a select's `choices`, a link's `target_table_id`, …). There is no email or
  phone type: use `text`, or `url` for a link to open. Any other `type` is refused, naming the
  valid ones.
- **Answers:** every command answers `{"result": …, "change": …}`: `result` is the request's own
  answer (for [`table.tables.list`](/kits/table/commands/table.tables.list/),
  `{"total", "items"}`), and `change` what the call changed, or `null`. Each command's output
  schema describes both.

## Where a step acts: `target`, `at` and `on`

Nothing a call does depends on, or leaves behind, what the person's editor shows: name tables,
views, records and fields by id every time, with the ids earlier answers gave you.

- A `table_run` step is `{command, params?, on?, at?}`. The run's `target` (`{"table": 2}` or
  `{"view": 3}`) is the part every step acts in; a step's `at.part` overrides it for that step.
  Commands still take their own `tableId` (and `viewId`): give it, matching the part.
- `on` names the **record** or the **field** a step acts on, by id (`[4]`). It goes into the
  command's `recordId` (a record of the command's `tableId`: `table.records.update`, `.delete`,
  `.history`, `table.links.*`) or `fieldId` (`table.fields.update`, `.delete`, `.duplicate`,
  `.linkCandidates`, `table.views.patchField`), which `table_commands` with `detail` marks as the
  command's `targets`; then do not give that parameter too. The command says whether `on` is a
  record or a field. A command without such a parameter takes no `on`, and an id `on` gives
  elsewhere is refused (a table file keeps no selection; an id that is both a record and a field
  of the part's table is refused as ambiguous).
- No command reads what the editor shows. `table.preview.show` changes only the person's picture
  and is put back after the call; never use it to choose what a later step acts on.
- Positions are not ids: `offset` in `table.records.list`, `rows` in a preview and `slide` (a
  table's position) change when records or tables are added or moved.

## Previews of a named table or view

`table_inspect` with `"preview": true` draws `target`: `{"table": 2}` (the table as its first view
shows it), `{"view": 3}` (the records the view shows, filtered and in its order), a diagnostic's
`target` (`{"path": "tables/2/records/4/fields/5"}`: the cell outlined), and `rows: [from, to]`
for some records of it (positions counted from 1). Without `target` it draws the first table's
first view, never what the editor shows. `scale` (up to 2) and `maxSize` set its size, and
`preview` in the answer says what was drawn (`tableId`, `viewId`). The same call answers the same
picture, and drawing it changes nothing.

## What a call changed, and what others changed

- **Answers and receipts.** Every command answers `{"result", "change"}`; `change` lists, by
  integer id, the tables, schemas and views it changed and the records upserted or deleted (with
  their row keys). A successful `table_run` or `table_batch` also answers `changes`: `created`,
  `changed` (each with `edits`: a field, `before` and `after`), `moved` and `deleted`, naming
  objects by the object model's ids: a record by its `row_key` (its `cells` keyed by the fields'
  object-model ids), a field, view or table by its object-model id. Match them with
  `table.records.list` (each record's `row_key`) or `table_query`.
- **Pass the latest `revision` as `basis`.** Every answer of inspect, query, run and batch has a
  short `revision`. Pass the last one you were given as `basis` on your next call: the answer then
  says what others (people, other agents) changed since, in `others` (with `by`).
- **A `conflict`** (retryable) means others changed what the write acts on since your `basis`
  (`deleted`, `relocated`, `changed`, or `unknown` when it cannot be told object by object). Nothing was run.
  Read `others` and `conflicts`, look again, decide again, and send the call with the `revision`
  the refusal gives as `basis`. Do not undo others' changes. A typed per-call premise resolves a record's integer id to its persistent
  `row_key`, including a deleted record's tombstone, before comparing changes.

## What writes hold

Every command object parameter accepts the UUID used by receipts, conflicts and queries.
Reads expose it as `tool_id`; records also call it `row_key`. Use these UUIDs for `on` and
for table/view parts. Existing integer inputs remain local projection addresses, not stable
tool ids. Their declarations use index addressing and list windows: a structural change
before such an address can refuse it as `relocated`. UUID writes do not hold those windows.

The guard uses the facts declared for each call. Supplying a basis does not make an undeclared
dependency safe. Their direct tests are `places`, `outside`, and `saved`.

| Commands | Premise |
|---|---|
| `tables.update`, `fields.update`, `views.update` | Supplied stored properties; replacing `options`, `meta`, or `config` holds that whole value. Changing the primary field also holds `Table.primaryField`; the new primary field only that it is one of the table's fields. |
| `views.patchConfig`, `putFilterTree`, `putSorts`, `putGroups` | Effective setting, reported as derived `~rowHeight`, `~frozenColumnCount`, `~filterTree`, `~sorts`, or `~groups`. The search is the editor's own and is never stored. An absent config and an absent key have the same value. |
| `views.patchField` | The addressed width, visibility, position or frozen value; field existence. Widths are integer pixels, rounded and clamped to 40 through 1200 on model-batch entry. Position and freezing are stored in `config.field_layout`, not SQL-only editor state. |
| `records.create`, `records.update`, transaction `cells.set` | Each written cell, and its field's name membership and type (a link's options too, a select's choices: the names of the choices its values name, or every choice and the field's list of them when a value names none it lists; a computed field's value is dropped). Required flags and `readonly` are not held: the required check only refuses the call, and no write reads `readonly`. The table need only exist. Repeated transaction operations union their dependencies. |
| All named cell writes, including transaction values and combined values/order writes | Table-scoped name membership, including prior ownership after a cross-table move; a name swapped, renamed away or moved out changes the address. |
| Record order changes, field insertion, transaction `fields.move`, positive-position bulk copy | The destination gap, in stored list coordinates. Moves additionally hold the moving object's position. Record anchors hold their position. |
| Link writes, through cells or `links.replace`/`delete` | Existing membership, target record existence and ownership, the effective symmetric partner, its schema and the reverse membership the write may replace. Target display-cell changes alone are unrelated. |
| Formula/lookup/rollup schema and AI results | Fields a formula names, by name membership; fields and tables an option names, that they are there (the cycle check reads more, but only refuses). AI application holds the AI field's settings, the source's name, title and type, and transitive source values, linked membership and linked source cells. A whole-record concatenating rollup holds every linked field and value. Formula references are `{field_name}`; there is no separate UUID formula syntax. |
| `fields.duplicate` | The source field's settings and position, and the names occupying generated-name candidates. It creates an empty column; it does not copy source cells. |
| `tables.create`, `fields.create`, new-table `import.run` | Name membership through the first free generated suffix; unrelated names and display titles are independent, also for `tables.create {}` with no place parameter. |
| Field delete | Every stored property of the field, not where it is; all current cell values in that field, row membership and affected links. |
| Field type update | The field's `type` only. A type update changes the schema and computed projection; it does not convert stored cell values, so a cell written meanwhile stays under the new type. |
| `records.bulkCopy` | Stored source cells, every field's type (which are stored), and an explicit destination gap. It omits computed and link values. Bulk delete and transaction deletion hold selected records and affected links, not where the records are. |
| `import.run`, `csv.import` | Every field's name, title and type (a link's options) and their order, and name membership; replacement holds rows and their membership; upsert also holds key values and cells it can replace. Append does not hold unrelated existing cell values. |
| Dashboards and view references | The replaced dashboard (not where it is), nested table/view existence and ownership, field definitions and table-scoped field names. View settings hold only the setting replaced; a filter, sort or group holds its field's name (written beside its id) and table; a view config holds only that the fields it names are there. |
| Table/record/view deletion | Every stored property of the object being deleted (a record: its every cell), not where it is; table deletion also holds all its fields, views, records, object lists and link cleanup. Replacement imports hold removed records and their links. |
| Undo/redo | An outside reread imports only what the file adds, outside both undo histories, so the session keeps its own: undo or redo takes back or puts again only its own step and preserves the other writer's values. A file of another document, or one that lacks a change of the session, is not reconciled. |
| Attachments, exports and reads | Attachment upload adds a content-addressed blob, not an attachment cell. Reads and exports do not change table content. |

There is no point/region write, spatial bounds, parent transform or carried-child motion in
this kit. Reordering a table leaves its records' own positions unchanged; `carried` is not
applicable to that list operation.

For a call that declares `covers`, the kit compares numeric windows itself, alongside name
membership. Independent-file prefix deletion and
move-out tests cover insertion, a single local address and both move-destination forms.
Raw model batches (`table_batch`) run under the premise contracts the model declares
(`batchOps` in `model/schema.json`). Most operations are `direct`: they hold their object, the
fields they write, their references and the insertion window of an index, so `create`,
`list.insert` and `list.move` refuse when an outside change moved or removed what was before
their index. A raw cell write (`map.set` or `map.remove` of `Record.cells` under a field's id)
holds that one cell and, of the field, that it is there, its `type`, `options` and `choices`, and
every choice's `name` (the written value is read through them): a deleted column refuses
`deleted`, a changed type or option set refuses `changed`, while a rename of the field, the
record's place and its other cells are independent.

What the document holds, for raw writes: a select's options are `Choice` objects (`name`,
`color`) its `Field.choices` lists in order (a value it does not list is one of its
`otherChoices`), and its cells hold choice ids (a
`multi_select` a list of them); a `long_text` cell holds the id of a `CellText` object (the ids of its `record`
and `field`, and its `text`, which merges per character); a field's AI prompt is its `aiPrompt` text.
A view's filters are `Filter` objects in `View.filters` (groups and conditions, each naming its
group by `parent` and, a condition, its field by object id); its sorts and groups are `Sort` and
`Group` objects in `View.sorts` and `View.groups` (the config's `sorts` and `groups`, which the
editor and `views.putSorts`/`views.putGroups` write alike); its hidden columns and column layout
are the maps `View.hiddenFields` (the config's `hidden_field_names`, which `views.patchField`'s
visibility writes too) and `View.layout`; `View.config` keeps the rest, which changes as a unit
(row height, frozen columns, `cover_field`). A dashboard's widgets are `Widget`
objects in `Dashboard.widgets` (`key`, `title`, `type`, `query` naming fields by object id,
`settings`, `layout`). An AI action's instruction is the field's `aiPrompt` text, not a key of
`options.ai_config`. The commands take and answer plain values (option
names, the text, fields by name) and store them in that shape.

A call that reads a table's schema in field order holds that table's own `~fieldOrder`, a
small hash of its ordered field identities, so the same-server journal keeps its `before`: a
field another writer adds to another table leaves the call independent, while a move of the
table's own fields refuses. Local integer addresses are held by their windows (`within`),
which the guard checks itself; a record another agent adds to another table leaves a write by
local integers independent, and one inserted above the addressed record refuses `relocated`
(`list_preimages_on_same_server_local_addresses`,
`a_schema_read_holds_its_own_tables_field_order`).
Stable-ID ownership uses small per-object derived values. Destructive whole-object premises
can still refuse unrelated properties of their source. Layout patches compare effective
entries, including null-to-object transitions, so sibling layout entries are independent.

Receipts compact identical numeric shifts and identical assignments (the same value written
to the same field) as `footprint`, naming all affected UUIDs. Other edits beyond the default
receipt's bound are counted in `truncated`; request `changes: "all"` for every edit, and never
treat a receipt with `truncated` as complete. An edit names its `field`; `before` and `after`
are given when known and small. The guard's own derived facts (`~fieldOrder`, `~linkCells`,
`~mirrors`, `~rowIds`, `~owner`) are `premiseOnly`: receipts never report them.

Offline kit-env generation makes and replays creation tasks for every stored type (Table,
Field, View, Record, Dashboard). A raw `create` is given what a create gives: a Record's
`createdAt` (the time of the write) and `createdBy`, a Table's empty `description`, and a
Field's empty `formula` (receipts leave empty strings out of a created object's values). A creation receipt replayed through `table_batch` (`kition_env::ops_for`) therefore
makes the same object under the receipt's id
(`waiting_for_sdk_creation_replay_and_required_defaults`). Required JSON values
must remain writable and have a same-type example in the seed file. For command-level record
creation, inspect `required` fields and supply their values, for example
`{"tableId": 1, "values": {"title": "New record"}}` in the starter table.

## Reading the file

`table_inspect` answers every table with:

- `id`, `tool_id`, `name`, `label` (the title people see, which `title` params set), `recordCount`;
- `fields`: each field's `id`, `name` (the key of its values), `label`, `type`, `primary`, and the
  `options` that matter for values: a select's `choices`, a link's `target_table_id`, a lookup's or
  rollup's `link_field_id` and `target_field_id`;
- `views`: each view's `id`, `label`, `type`, and what it does by field name: `filter`
  (`status neq "Done"`), `sorts` and `groups` (`title asc`), `hidden`;
- `records`: its first records as `{columns, rows, from, to, total, next?}`. `columns` are the field
  names, and each row is `{id, tool_id, cells}`, one cell per column (`null` when empty). A link's cell lists
  the linked records as `{id, display}`, by record id; text over 200 characters is cut and ends in
  `…`.

It lists at most 20 records of a table and 100 in all. When a table's records are cut, `next` is
the step that lists the rest: run it as it is with `table_run`.
[`table.read`](/kits/table/commands/table.read/) `{"tableId": 1}` lists a table's records and
`{"viewId": 2}` the records a view shows (filtered and in its order), 50 at a time (`limit`, up to
200); each answer's `next` goes on from where it stopped (`cursor`).

```json
table_run {"path": "crm.ktable", "steps": [
  {"command": "table.read", "params": {"viewId": 2, "limit": 100}}
]}
```

## New files

A new file (`table_create`, or a host's `document.open` with `create: {"format": "ktable"}`) is
not empty: like a new table in the editor, it holds one starter table, table 1 `Table` (name
`table`), with fields `title` (text, primary), `status` (single_select: `To do`, `In progress`,
`Done`) and `notes` (long_text), one grid view `All`, and no records.

To build your own schema, rename the starter table and change its fields, or delete it. A host
that makes a file for its own schema deletes it as a system write, so the person's Undo does not
bring it back; ids are never reused, so its first own table is table 2:

```json
{"command": "table.tables.delete", "params": {"tableId": 1, "asOrigin": "system:setup"}}
{"command": "table.tables.create", "params": {"title": "Contacts", "fields": [
  {"title": "Name", "type": "text", "primary": true},
  {"title": "Email", "type": "text"},
  {"title": "Stage", "type": "single_select", "options": {"choices": ["Lead", "Customer"]}}
]}}
```

A file with no table at all is valid and saves.

## Workflow

1. `table_inspect` to learn the ids, field names, types, views and records; `table.read` for
   records it cut.
2. `table_commands` with keywords. Use the exact id it returns and follow its parameter schema;
   never guess a command id.
3. `table_run` a few steps, each naming its table (`tableId`, and the run's `target` or `at.part`)
   and its record or field (`on`, or the command's own `recordId` or `fieldId`). A failed step rolls
   the whole run back. Pass the last `revision` as `basis`.
4. `table_check`, then fix what it reports. Many diagnostics carry a `fix` you can run as a step.
5. `table_inspect` again to confirm the result.

## Example 1: add a record and update another

```json
table_inspect {"path": "crm.ktable"}
```

The answer lists table 1 with fields `title` (text, primary), `status` (single select:
`To do`, `In progress`, `Done`) and `notes` (long text), view 1 `All`, and its records. Then:

```json
table_run {"path": "crm.ktable", "steps": [
  {"command": "table.records.create", "params": {"tableId": 1, "values": {"title": "Book the venue", "status": "To do"}}},
  {"command": "table.records.update", "params": {"tableId": 1, "recordId": 1, "values": {"status": "In progress", "notes": "Draft shared on Monday"}}}
]}
table_check {"path": "crm.ktable"}
```

The first step answers the new record with `"id": 3`; both steps answer `"changed": true`, and the
check finds nothing wrong.

## Example 2: a number field and a filtered, sorted view

```json
table_run {"path": "crm.ktable", "steps": [
  {"command": "table.fields.create", "params": {"tableId": 1, "name": "priority", "title": "Priority", "type": "number"}},
  {"command": "table.records.update", "params": {"tableId": 1, "recordId": 2, "values": {"priority": 2}}},
  {"command": "table.views.create", "params": {"tableId": 1, "title": "Open work", "type": "grid"}}
]}
```

The field is answered with `"id": 4`, the view with `"id": 2`. Show only what is not done, by title:

```json
table_run {"path": "crm.ktable", "steps": [
  {"command": "table.views.putFilterTree", "params": {"tableId": 1, "viewId": 2,
    "tree": {"conjunction": "and", "children": [{"condition": {"field_id": 2, "operator": "neq", "value": "Done"}}]}}},
  {"command": "table.views.putSorts", "params": {"tableId": 1, "viewId": 2, "items": [{"field_id": 1, "direction": "asc"}]}}
]}
table_check {"path": "crm.ktable"}
```

Both steps answer the stored tree and sorts, and the check finds nothing wrong.

## Example 3: edit two tables in one run

`table_inspect` lists table 1 `Notes` (fields `title`, `status`, `notes`; records 1 `Ask about
visas` and 2 `Find a sitter`) and table 2 `Packing` (fields 4 `item` and 5 `packed`; records 3
`Passport`, 4 `Charger`, 5 `Boots`). Mark two items packed and finish a note, whatever the person
is looking at:

```json
table_run {"path": "trip.ktable", "basis": "ykl0", "target": {"table": 2}, "steps": [
  {"command": "table.records.update", "on": [3], "params": {"tableId": 2, "values": {"packed": true}}},
  {"command": "table.records.update", "on": [4], "params": {"tableId": 2, "values": {"packed": true}}},
  {"command": "table.records.update", "at": {"part": {"table": 1}}, "on": [2], "params": {"tableId": 1, "values": {"status": "Done"}}}
]}
```

Each step answers its record (`"id": 3`, …) and a `change` naming it; `changes` lists the three
records (by row key) with their cells before and after. The next call starts again from nothing:
name the tables and records again.

## Example 4: preview a view, then rename a field

```json
table_inspect {"path": "trip.ktable", "target": {"view": 2}, "preview": true}
table_run {"path": "trip.ktable", "basis": "ykl1", "steps": [
  {"command": "table.fields.update", "on": [5], "params": {"tableId": 2, "title": "In the bag"}}
]}
```

The preview shows view 2 of table 2 (`preview.viewId` is 2), whatever the editor shows. The rename
answers the field with `"id": 5` and its name unchanged (`packed`: values keep their key), and
`changes.changed` names one `Field` with `title` `"Packed"` → `"In the bag"`.

## Pitfalls

- **Unknown keys:** a value key that names no field is refused, and nothing is written. When the
  key is a field's title, the error says so (`the field titled "Primary channel" is named
  primary_channel`): use that name, or read the names with
  [`table.fields.list`](/kits/table/commands/table.fields.list/).
- **Select options:** a single-select value must be one of the field's `options.choices`. Writing
  another value succeeds, and `table_check` then warns `table.select-option-missing`. Either add
  the choice first ([`table.fields.update`](/kits/table/commands/table.fields.update/)
  `{tableId, fieldId, options}`) or use an existing one.
- **Formula fields** are computed: write their `formula`, never their values.
- **Empty values:** a record's `values` holds only the fields that have a value; a missing key means
  empty.
- **Nothing carries over between calls.** No call remembers the table, view or records of the one
  before, or what a `table.preview.show` showed: name them by the ids earlier results gave.

## Checks

`table_check` lists what is wrong with the file. Each diagnostic has a stable `code`, a
`severity` (`error`, `warning` or `info`), a `message`, a `target` and, for some, a `fix`: a
command step to run as it is. Act on the code, not on the message. A `fix` is a suggestion: run it
only when it is what the person asked for. Every code the kit reports is on the
[checks page](/kits/table/checks/).

The check reports values that are not one of a select's options (`table.select-option-missing`,
whose `fix` adds the choice), empty required fields, duplicate primary values, broken links and
lookups, formula errors, and dashboards on missing tables.
