# Gaps

> **Last updated:** 2026-11-16 · **Source:** `features.yaml` (updated 2026-11-16) and the engine's command list

Every known shortfall of Kition Table, from the feature catalogue: features that are not yet `has`,
then the differences recorded on features that work. Update the feature in `features.yaml` when you
land work, then this list. The comparison with the target application is in
[parity/target-app-parity.md](parity/target-app-parity.md); the order of work is in [roadmap.md](roadmap.md).

## Features not done

- `table.ai-fields` (P1, partial): AI fields: prepare a request and apply its answer to cells. The engine has these commands; no case yet shows the area working end to end in the editor and surviving save and reopen.
- `table.history` (P1, partial): Undo, redo, transactions and the change feed. The engine has these commands; no case yet shows the area working end to end in the editor and surviving save and reopen.

## Differences on working features

- `table.dashboards`: file.render is covered by the command tests, not by the round trip in bindings/wasm/src/round_trip_tests.rs.
- `table.from-files`: the model is scripted in every test; in the browser flow the host runs no models, so the dialog is tested with the plain design and no AI fill. The files' blobs stay in the file after a failed or canceled run, and after Undo: the engine stores blobs outside the document and cannot release one.
- `table.import-export`: file.exportMarkdown and file.previewImport are covered by the command tests, not by the round trip in bindings/wasm/src/round_trip_tests.rs.
- `table.links`: recordLink.delete is covered by the command tests, not by the round trip in bindings/wasm/src/round_trip_tests.rs.
- `table.schema`: table.delete and field.linkCandidates are covered by the command tests, not by the round trip in bindings/wasm/src/round_trip_tests.rs.
- `table.views`: tableView.delete is covered by the command tests, not by the round trip in bindings/wasm/src/round_trip_tests.rs.

## Known engine gaps

Found by reading the write handlers; not fixed yet:

- Link values written through `values`, `transact`, `csv.import` and `import.run` accept a record
  of any table, where `links.replace` requires the target table.
- `tables.update` with `primary_field_id` skips the cycle check that `fields.update` with
  `is_primary` runs.
- A type change away from `formula` keeps `readonly`; `required` on a computed field blocks every
  create.
- Option ids resolve deleted rows and other tables' rows.
- `fields.duplicate` copies a symmetric partner claim.
- Deleting a field or a table leaves lookups, formulas, partners, view settings and dashboards
  that name it dangling.
- `views.update` drops `filter_tree` but keeps sorts and groups; `patchField` writes its three
  parts in three commits; `putSorts` and `putGroups`
  answer the request rather than what was stored, and report validation errors as internal.
- Upsert imports write each match from a snapshot taken before the first write; `csv.import` and
  `import.run` map columns and convert cells differently.
- `ai.apply` writes into a field of any type with an `ai_config`, reports a dropped computed write
  as done, and hashes the stored config rather than the one run.
- `attachments.put` answers the first stored MIME type and name of identical bytes.
- `records.create` ignores `order`.
- A delete holds every field of what it removes, so an outside edit of a cell of a record about
  to be deleted refuses the delete.

## Catalogue coverage

The catalogue has 15 features, and 54 of the engine's 54 commands are claimed
by one of them.
