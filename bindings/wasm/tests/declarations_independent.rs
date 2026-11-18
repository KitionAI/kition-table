//! Every command that changes a table file, compared with a reading of its handler
//! (`kition-table-api`'s `dispatch`, the engine methods `import.run` and `ai.apply`, undo and
//! redo) and the file operations behind it (`kition-table-file`, its KDM model, commit and
//! projection): which parameters each resolves and how, what it reads of the file and of the
//! editor, and what it writes. Each row is one code path on [`fixture`], with what a write with
//! `basis` must hold: a change refuses it when it alters what the write lands on (a local integer
//! address names what was there) or a fact its result depends on. A read that only refuses the
//! call (the required check, the cycle check) is not such a fact; `readonly` is read by no write.
//!
//! What a write holds is computed as the guard does ([`held`]): the editor state the command
//! declares; its target parameter's objects and the objects other parameters name; `under`'s
//! premises replacing them; and, for a step that names a place (every command here with a table,
//! field, view or record parameter), the objects and windows `under` answers and the facts
//! `covered` refuses for (`TableDocument::covered_facts`): name membership (`~names:est`,
//! `~tableNames:plan`), link membership (`~linkCells:<record>.<field>`, every link of a record,
//! `~linkCells:*.<field>` every link of a field), partners (`~mirrors`), row identities
//! (`~rowIds`), field lists (`~lists`; the kit holds one as its table's `~fieldOrder`), layout entries and select labels
//! (`options.choices:Done`). Every row runs twice: with tool ids, and with every table, field,
//! view and record as its local integer, which must hold the same and the window of each
//! integer's place in its list (integers are local projection addresses).
//!
//! Rows marked `pinned` state a fact in the declaration's own spelling: a candidate name's
//! membership for every field's `name`; the links of a record or field; the select label's
//! membership; `~owner` of an object addressed by id; the column's rows added since; a written
//! field's `type` (computed values are dropped); the AI source's `title` in the prompt;
//! csv.import's first field in order whose name or title matches; and the integer windows of
//! `tableId` inside `transact` ops and of `@` ids given in the rows.
//!
//! No parameter is resolved differently from its name; no editor state is read (the picture's
//! table or view is the only editor state, and no write reads it).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::{BTreeMap, BTreeSet};

use kition_kit::KitCommand;
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::{Value, json};

fn run(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params.clone()).unwrap_or_else(|e| panic!("{command} {params}: {e}"))["result"].clone()
}

fn int(value: &Value) -> i64 {
    value["id"].as_i64().unwrap_or_else(|| panic!("no id in {value}"))
}

fn tool(value: &Value) -> String {
    value["tool_id"].as_str().or_else(|| value["row_key"].as_str()).unwrap_or_else(|| panic!("no tool id in {value}")).to_owned()
}

/// One object of the fixture: its role, tool id, local integer id, and name (tables, fields).
#[derive(Clone)]
struct Role {
    role: &'static str,
    tool: String,
    local: Option<i64>,
    name: Option<String>,
}

/// The document the rows run on, by role:
/// - `$Base`: the root; `$D`: dashboard `d1` (source table `$T`, one widget);
/// - `$T` (the starter table `table`): fields `$Title` (primary, required), `$Status` (single
///   select: To do / In progress / Done), `$Notes` (long text), `$Owner` (link to `$P`, symmetric
///   with `$PTasks`), `$Est` (number), `$Tags` (multi select a / b), `$Look` (lookup of `$Name`
///   through `$Owner`), `$Calc` (formula `{est} * 2`), `$Summary` (AI summary of `$Notes`);
///   records `$R1` (One, To do, N1, 1, [a], owner `$P1`), `$R2` (Two, Done, N2, 2, owner `$P2`),
///   `$R3` (Three); views `$V` (grid "All": filter Status is To do, sort Est desc) and `$K`
///   (kanban grouped by Status);
/// - `$P` (`people`): fields `$Name` (primary), `$PTasks` (link to `$T`); records `$P1` (Ada),
///   `$P2` (Grace); view `$PV`.
fn fixture() -> (TableDocument, Vec<Role>) {
    let mut doc = TableDocument::create("Read.ktable", 0, 0).unwrap();
    let p = run(&mut doc, "table.tables.create", json!({"title":"People","fields":[{"title":"Name","type":"text","primary":true}]}));
    let pid = int(&p);
    let owner = run(&mut doc, "table.fields.create", json!({"tableId":1,"title":"Owner","type":"link_to_record","options":{"target_table_id":pid}}));
    let ptasks = run(
        &mut doc,
        "table.fields.create",
        json!({"tableId":pid,"title":"Tasks","type":"link_to_record","options":{"target_table_id":1,"symmetric_field_id":int(&owner)}}),
    );
    let est = run(&mut doc, "table.fields.create", json!({"tableId":1,"title":"Est","type":"number"}));
    let tags = run(&mut doc, "table.fields.create", json!({"tableId":1,"title":"Tags","type":"multi_select","options":{"choices":["a","b"]}}));
    let look = run(
        &mut doc,
        "table.fields.create",
        json!({"tableId":1,"title":"Look","type":"lookup","options":{"link_field_id":int(&owner),"target_field_id":p["fields"][0]["id"]}}),
    );
    let calc = run(&mut doc, "table.fields.create", json!({"tableId":1,"title":"Calc","type":"formula","formula":"{est} * 2"}));
    let summary = run(
        &mut doc,
        "table.fields.create",
        json!({"tableId":1,"title":"Summary","type":"ai","options":{"ai_config":{"type":"summarize","source_field_id":3}}}),
    );
    let p1 = run(&mut doc, "table.records.create", json!({"tableId":pid,"values":{"name":"Ada"}}));
    let p2 = run(&mut doc, "table.records.create", json!({"tableId":pid,"values":{"name":"Grace"}}));
    let r1 = run(
        &mut doc,
        "table.records.create",
        json!({"tableId":1,"values":{"title":"One","status":"To do","notes":"N1","est":1,"tags":["a"],"owner":[tool(&p1)]}}),
    );
    let r2 =
        run(&mut doc, "table.records.create", json!({"tableId":1,"values":{"title":"Two","status":"Done","notes":"N2","est":2,"owner":[tool(&p2)]}}));
    let r3 = run(&mut doc, "table.records.create", json!({"tableId":1,"values":{"title":"Three"}}));
    let k = run(&mut doc, "table.views.create", json!({"tableId":1,"title":"Board","type":"kanban"}));
    run(&mut doc, "table.views.putGroups", json!({"tableId":1,"viewId":int(&k),"items":[{"field_id":2,"direction":"asc"}]}));
    run(&mut doc, "table.views.putSorts", json!({"tableId":1,"viewId":1,"items":[{"field_id":int(&est),"direction":"desc"}]}));
    run(
        &mut doc,
        "table.views.putFilterTree",
        json!({"tableId":1,"viewId":1,"tree":{"conjunction":"and","children":[{"condition":{"field_id":2,"operator":"is","value":"To do"}}]}}),
    );
    run(
        &mut doc,
        "table.dashboards.upsert",
        json!({"dashboardId":"d1","title":"Overview","order":1,"source_table_id":1,"layout":[],"widgets":[{"id":"w1","type":"metric"}]}),
    );
    let bytes = doc.save(None).unwrap();
    let doc = TableDocument::open("Read.ktable", &bytes).unwrap();
    let inspect = doc.inspect().unwrap();
    let mut roles = Vec::new();
    let base = doc.file().replica().objects_of_type("Base")[0].as_str().to_owned();
    roles.push(Role { role: "$Base", tool: base, local: None, name: None });
    roles.push(Role { role: "$D", tool: "d1".into(), local: None, name: None });
    let tables = inspect["tables"].as_array().unwrap();
    let names: [(&str, &[&'static str], &[&'static str]); 2] = [
        ("$T", &["$Title", "$Status", "$Notes", "$Owner", "$Est", "$Tags", "$Look", "$Calc", "$Summary"], &["$V", "$K"]),
        ("$P", &["$Name", "$PTasks"], &["$PV"]),
    ];
    for (table, (role, fields, views)) in tables.iter().zip(names) {
        roles.push(Role { role, tool: tool(table), local: Some(int(table)), name: table["name"].as_str().map(str::to_owned) });
        for (field, role) in table["fields"].as_array().unwrap().iter().zip(fields.iter()) {
            roles.push(Role { role, tool: tool(field), local: Some(int(field)), name: field["name"].as_str().map(str::to_owned) });
        }
        for (view, role) in table["views"].as_array().unwrap().iter().zip(views.iter()) {
            roles.push(Role { role, tool: tool(view), local: Some(int(view)), name: None });
        }
    }
    for (role, record) in [("$P1", &p1), ("$P2", &p2), ("$R1", &r1), ("$R2", &r2), ("$R3", &r3)] {
        roles.push(Role { role, tool: tool(record), local: Some(int(record)), name: None });
    }
    let _ = (ptasks, tags, look, calc, summary);
    (doc, roles)
}

/// `"$X"` is role X's tool id, `"@X"` its local integer id (a string `"@X"` becomes a number).
fn with_roles(value: &Value, roles: &[Role]) -> Value {
    match value {
        Value::String(text) => {
            if let Some(role) = text.strip_prefix('@') {
                let wanted = format!("${role}");
                return roles.iter().find(|r| r.role == wanted).and_then(|r| r.local).map_or_else(|| value.clone(), |id| json!(id));
            }
            roles.iter().find(|r| r.role == text).map_or_else(|| value.clone(), |r| json!(r.tool))
        }
        Value::Array(items) => Value::Array(items.iter().map(|item| with_roles(item, roles)).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, item)| (key.clone(), with_roles(item, roles))).collect()),
        _ => value.clone(),
    }
}

/// Replaces every role's tool id inside `text` by the role.
fn spelled_text(text: &str, roles: &[Role]) -> String {
    let mut out = text.to_owned();
    for role in roles.iter().filter(|r| r.tool.len() > 8) {
        out = out.replace(&role.tool, role.role);
    }
    out
}

/// The strings at a declared object path (`a.b`, `items[].c`) of `params`, at any depth below it.
fn ids_at(params: &Value, path: &str) -> Vec<String> {
    let mut current = vec![params.clone()];
    for part in path.split('.') {
        let key = part.trim_end_matches("[]");
        let mut next = Vec::new();
        for value in &current {
            match value.get(key) {
                Some(Value::Array(items)) if part.ends_with("[]") => next.extend(items.iter().cloned()),
                Some(item) => next.push(item.clone()),
                None => {}
            }
        }
        current = next;
    }
    fn strings(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) => out.push(text.clone()),
            Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    current.iter().for_each(|value| strings(value, &mut out));
    out
}

/// The strings a parameter's value holds one level deep (the guard's `mentioned_in`).
fn mentioned(value: &Value) -> Vec<String> {
    let one = |value: &Value| value.as_str().map(str::to_owned);
    match value {
        Value::Array(items) => items
            .iter()
            .flat_map(|item| match item {
                Value::Object(object) => object.values().filter_map(one).collect::<Vec<_>>(),
                other => one(other).into_iter().collect(),
            })
            .collect(),
        Value::Object(object) => object.values().filter_map(one).collect(),
        other => one(other).into_iter().collect(),
    }
}

fn spelled(set: Option<&BTreeSet<String>>) -> String {
    match set {
        None => "*".to_owned(),
        Some(set) if set.is_empty() => "-".to_owned(),
        Some(set) => set.iter().cloned().collect::<Vec<_>>().join(","),
    }
}

/// What a write with `basis` of a command holds, as the guard computes it: editor state,
/// objects (fields, positions), windows.
#[derive(Debug, PartialEq)]
struct Held {
    editor: String,
    holds: BTreeMap<String, (String, String)>,
    windows: BTreeSet<String>,
}

fn held(doc: &TableDocument, command: &KitCommand, params: &Value, roles: &[Role]) -> Held {
    let role_of = |id: &str| roles.iter().find(|r| r.tool == id).map(|r| r.role.to_owned());
    let name = |id: &str| role_of(id).unwrap_or_else(|| spelled_text(id, roles));
    let mut editor: Vec<&str> = command.reads.iter().map(|state| state.as_str()).collect();
    editor.sort_unstable();
    let mut holds: BTreeMap<String, (String, String)> = BTreeMap::new();
    if let Some(target) = command.targets {
        let values = match params.get(target) {
            Some(Value::Array(items)) => items.clone(),
            Some(one) => vec![one.clone()],
            None => vec![],
        };
        for id in values.iter().filter_map(Value::as_str).filter(|id| role_of(id).is_some()) {
            holds.insert(name(id), ("*".to_owned(), if command.position_independent { "-" } else { "*" }.to_owned()));
        }
    }
    let mut others: BTreeSet<String> = BTreeSet::new();
    for (key, value) in params.as_object().into_iter().flatten() {
        if Some(key.as_str()) == command.targets || command.places.contains(&key.as_str()) {
            continue;
        }
        others.extend(mentioned(value));
    }
    for path in command.objects.iter().filter(|path| path.contains('.') || path.contains("[]")) {
        others.extend(ids_at(params, path));
    }
    let under = doc.under(&command.id, params).unwrap_or_else(|e| panic!("{}: {e}", command.id));
    let mut windows = BTreeSet::new();
    let placed = command.places.iter().any(|place| params.get(*place).is_some_and(|value| !value.is_null()));
    if let Some(under) = under.as_ref().filter(|_| placed) {
        for within in &under.within {
            windows.insert(format!("{}.{}[{}..{}]", name(within.parent.as_str().unwrap_or_default()), within.field, within.from, within.to));
        }
    }
    if let Some(under) = &under {
        for object in under.objects.iter().filter_map(Value::as_str) {
            holds.entry(name(object)).or_insert_with(|| ("*".to_owned(), "*".to_owned()));
        }
        for premise in &under.premises {
            let id = premise.id.as_str().map(str::to_owned).unwrap_or_else(|| premise.id.to_string());
            let fields = premise.fields.as_ref().map(|fields| fields.iter().map(|f| spelled_text(f, roles)).collect::<BTreeSet<_>>());
            let positions = premise.positions.as_ref().map(|p| p.iter().cloned().collect::<BTreeSet<_>>());
            let need = (spelled(fields.as_ref()), spelled(positions.as_ref()));
            // Premises on one object merge as the guard merges needs: a union, `*` wins.
            let merged = match holds.get(&name(&id)) {
                Some(old) if under.premises.iter().filter(|p| p.id == premise.id).count() > 1 => (union(&old.0, &need.0), union(&old.1, &need.1)),
                _ => need,
            };
            holds.insert(name(&id), merged);
        }
    }
    // For a step that names a place and whose `under` asks to be asked (`covers`), `covered`
    // refuses for the facts its selection holds.
    if placed && under.as_ref().is_some_and(|under| under.covers.is_some()) {
        for (id, fact) in doc.covered_facts(&command.id, params).unwrap_or_else(|e| panic!("{}: {e}", command.id)) {
            let entry = holds.entry(name(&id)).or_insert_with(|| ("-".to_owned(), "-".to_owned()));
            entry.0 = union(&entry.0, &spelled_text(&fact, roles));
        }
    }
    // Objects without a role (links) are told by `~linkCells` facts.
    holds.retain(|role, _| role.starts_with('$'));
    for id in others.iter().filter(|id| role_of(id).is_some()) {
        holds.entry(name(id)).or_insert_with(|| ("*".to_owned(), "*".to_owned()));
    }
    // Names in parameters the command declares as names (`x-khp-name`): the object the name named.
    for (param, field) in command.name_params.iter().chain(command.targets.zip(command.name_field).iter()) {
        for text in ids_at(params, param) {
            if let Some(role) = roles.iter().find(|r| *field == "name" && r.name.as_deref() == Some(text.as_str())) {
                let entry = holds.entry(role.role.to_owned()).or_insert_with(|| ("-".to_owned(), "-".to_owned()));
                entry.0 = union(&entry.0, field);
            }
        }
    }
    Held { editor: editor.join(","), holds, windows }
}

fn union(a: &str, b: &str) -> String {
    if a == "*" || b == "*" {
        return "*".to_owned();
    }
    let set: BTreeSet<&str> = a.split(',').chain(b.split(',')).filter(|s| !s.is_empty() && *s != "-").collect();
    if set.is_empty() { "-".to_owned() } else { set.into_iter().collect::<Vec<_>>().join(",") }
}

/// One code path of a command: its parameters (objects by role), the editor state it reads, what
/// a write holds of each object (role, fields, positions), the windows.
struct Row {
    command: &'static str,
    params: &'static str,
    editor: &'static str,
    holds: &'static [(&'static str, &'static str, &'static str)],
    windows: &'static [&'static str],
}

const fn r(command: &'static str, params: &'static str, holds: &'static [(&'static str, &'static str, &'static str)]) -> Row {
    Row { command, params, editor: "", holds, windows: &[] }
}

const fn rw(
    command: &'static str,
    params: &'static str,
    holds: &'static [(&'static str, &'static str, &'static str)],
    windows: &'static [&'static str],
) -> Row {
    Row { command, params, editor: "", holds, windows }
}

/// `fields($T)` and `records($T)` in a row stand for each of them.
fn expanded(holds: &[(&str, &str, &str)]) -> BTreeMap<String, (String, String)> {
    let groups: [(&str, &[&str]); 3] = [
        ("fields($T)", &["$Title", "$Status", "$Notes", "$Owner", "$Est", "$Tags", "$Look", "$Calc", "$Summary"]),
        ("fields($P)", &["$Name", "$PTasks"]),
        ("records($T)", &["$R1", "$R2", "$R3"]),
    ];
    let mut out: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (role, fields, at) in holds {
        let members: Vec<&str> = groups.iter().find(|(group, _)| group == role).map_or_else(|| vec![*role], |(_, members)| members.to_vec());
        for member in members {
            let entry = out.entry(member.to_owned()).or_insert_with(|| ("-".to_owned(), "-".to_owned()));
            *entry = (union(&entry.0, fields), union(&entry.1, at));
        }
    }
    out
}

/// The rows; a row marked `pinned` states its fact in the declaration's spelling (see the
/// module comment).
const ROWS: &[Row] = &[
    // tables.create: the new table's name is the first free of `plan`, `plan_2`, … among the
    // tables' names; appended to `Base.tables` (no index read). Inline fields' options name other
    // tables by id: existence (the cycle check only refuses).
    // pinned (spelling): name membership of the candidates, `~tableNames:plan`, for every table's `name`.
    r("table.tables.create", r#"{"title":"Plan"}"#, &[("$Base", "~tableNames:plan", "-")]),
    // pinned (spelling and the integer rule): as above; `@P` is a local address, its window.
    rw(
        "table.tables.create",
        r#"{"title":"Plan","fields":[{"title":"Who","type":"link_to_record","options":{"target_table_id":"@P"}}]}"#,
        &[("$Base", "~tableNames:plan", "-"), ("$P", "-", "-")],
        &["$Base.tables[1..2]"],
    ),
    // tables.update: each supplied property is replaced; a primary field must be one of the
    // table's fields (its owner) and replaces `primaryField`.
    r("table.tables.update", r#"{"tableId":"$T","title":"Tasks"}"#, &[("$T", "title", "-")]),
    r("table.tables.update", r#"{"tableId":"$T","description":"D"}"#, &[("$T", "description", "-")]),
    r("table.tables.update", r#"{"tableId":"$T","meta":{"k":1}}"#, &[("$T", "meta", "-")]),
    r("table.tables.update", r#"{"tableId":"$T","primary_field_id":"$Notes"}"#, &[("$T", "primaryField", "-"), ("$Notes", "~owner", "-")]),
    // tables.delete: the table and everything it holds go, wherever they are.
    // pinned: what it holds (each field, view and record, every field) and the links into its
    // records and of its fields (`~linkCells`), which go with it.
    r(
        "table.tables.delete",
        r#"{"tableId":"$P"}"#,
        &[
            ("$P", "*", "-"),
            ("$Name", "*", "-"),
            ("$PTasks", "*", "-"),
            ("$PV", "*", "-"),
            ("$P1", "*", "-"),
            ("$P2", "*", "-"),
            ("$Base", "~linkCells:$P1,~linkCells:$P2,~linkCells:*.$Name,~linkCells:*.$PTasks", "-"),
        ],
    ),
    // fields.create: the table exists; the generated name is the first free among the table's
    // field names; `order` is a place in `Table.fields`; options name objects by id (existence;
    // the cycle check only refuses).
    // pinned (spelling): name membership `~names:score` for every field's `name`.
    r("table.fields.create", r#"{"tableId":"$T","title":"Score","type":"number"}"#, &[("$T", "~names:score", "-")]),
    rw("table.fields.create", r#"{"tableId":"$T","title":"Score","type":"number","order":2}"#, &[("$T", "~names:score", "-")], &["$T.fields[1..1]"]),
    r(
        "table.fields.create",
        r#"{"tableId":"$T","title":"Who","type":"link_to_record","options":{"target_table_id":"$P"}}"#,
        &[("$T", "~names:who", "-"), ("$P", "-", "-")],
    ),
    // pinned: `~owner` of a field an option names by id (where it is: which table), as for
    // every field, view and record a parameter names.
    r(
        "table.fields.create",
        r#"{"tableId":"$T","title":"Who","type":"lookup","options":{"link_field_id":"$Owner","target_field_id":"$Name"}}"#,
        &[("$T", "~names:who", "-"), ("$Owner", "~owner", "-"), ("$Name", "~owner", "-")],
    ),
    // fields.update: each supplied property is replaced (readonly is re-derived from the values at
    // write time). A type change writes only `type`: no stored cell is converted, so a cell
    // written meanwhile is not lost. A formula names fields by name. `is_primary` replaces the
    // table's primaryField. pinned: `~owner` of the addressed field.
    r("table.fields.update", r#"{"tableId":"$T","fieldId":"$Notes","title":"Remarks"}"#, &[("$T", "-", "-"), ("$Notes", "title,~owner", "-")]),
    r("table.fields.update", r#"{"tableId":"$T","fieldId":"$Est","type":"text"}"#, &[("$T", "-", "-"), ("$Est", "type,~owner", "-")]),
    r(
        "table.fields.update",
        r#"{"tableId":"$T","fieldId":"$Status","options":{"choices":["To do","Done","Later"]}}"#,
        &[("$T", "-", "-"), ("$Status", "options,~owner", "-")],
    ),
    r(
        "table.fields.update",
        r#"{"tableId":"$T","fieldId":"$Owner","options":{"target_table_id":"$T"}}"#,
        &[("$T", "-", "-"), ("$Owner", "options,~owner", "-")],
    ),
    r("table.fields.update", r#"{"tableId":"$T","fieldId":"$Notes","required":true}"#, &[("$T", "-", "-"), ("$Notes", "required,~owner", "-")]),
    r("table.fields.update", r#"{"tableId":"$T","fieldId":"$Notes","readonly":true}"#, &[("$T", "-", "-"), ("$Notes", "readonly,~owner", "-")]),
    r("table.fields.update", r#"{"tableId":"$T","fieldId":"$Notes","is_primary":true}"#, &[("$T", "primaryField", "-"), ("$Notes", "~owner", "-")]),
    // pinned (spelling): `~names:est` for `$Est`'s `name`, and the existence of the field the
    // name names now (which the name's membership already holds).
    r(
        "table.fields.update",
        r#"{"tableId":"$T","fieldId":"$Calc","formula":"{est} + 1"}"#,
        &[("$T", "~names:est", "-"), ("$Calc", "formula,~owner", "-"), ("$Est", "-", "-")],
    ),
    r(
        "table.fields.update",
        r#"{"tableId":"$T","fieldId":"$Summary","ai_prompt":"Shorter"}"#,
        &[("$T", "-", "-"), ("$Summary", "aiPrompt,~owner", "-")],
    ),
    r("table.fields.update", r#"{"tableId":"$T","fieldId":"$Notes","meta":{"k":1}}"#, &[("$T", "-", "-"), ("$Notes", "meta,~owner", "-")]),
    // fields.delete: the field goes with its column, wherever it is: every record's cell of it,
    // and its links. pinned: rows added since (`~rowIds`) and the field's links (`~linkCells`),
    // which go with the column; a link field's stored cells.
    r(
        "table.fields.delete",
        r#"{"tableId":"$T","fieldId":"$Notes"}"#,
        &[("$T", "-", "-"), ("$Notes", "*", "-"), ("records($T)", "cells.$Notes", "-"), ("$Base", "~linkCells:*.$Notes,~rowIds:$T", "-")],
    ),
    r(
        "table.fields.delete",
        r#"{"tableId":"$T","fieldId":"$Owner"}"#,
        &[("$T", "-", "-"), ("$Owner", "*", "-"), ("records($T)", "cells.$Owner", "-"), ("$Base", "~linkCells:*.$Owner,~rowIds:$T", "-")],
    ),
    // fields.duplicate: the source's settings, its index (the copy goes right after it), and
    // the generated name's candidates. pinned: the source whole (its `name` cannot change by a
    // command) and its place; `~names:est_copy`.
    r("table.fields.duplicate", r#"{"tableId":"$T","fieldId":"$Est"}"#, &[("$T", "~names:est_copy", "-"), ("$Est", "*", "*")]),
    // views.create: appended to the table's views; config stored as given (a field it names by
    // id: existence and owner).
    r("table.views.create", r#"{"tableId":"$T","title":"Mine","type":"grid"}"#, &[("$T", "-", "-")]),
    r(
        "table.views.create",
        r#"{"tableId":"$T","title":"Mine","type":"grid","config":{"sorts":[{"field_id":"$Est","direction":"asc"}]}}"#,
        &[("$T", "-", "-"), ("$Est", "~owner", "-")],
    ),
    // views.update: title and type replaced; a config replaces the whole config (sorts and groups
    // are carried from the value at write time). pinned: `~owner`.
    r("table.views.update", r#"{"tableId":"$T","viewId":"$V","title":"Mine"}"#, &[("$T", "-", "-"), ("$V", "title,~owner", "-")]),
    r("table.views.update", r#"{"tableId":"$T","viewId":"$V","type":"gallery"}"#, &[("$T", "-", "-"), ("$V", "type,~owner", "-")]),
    r("table.views.update", r#"{"tableId":"$T","viewId":"$V","config":{"row_height":"tall"}}"#, &[("$T", "-", "-"), ("$V", "config,~owner", "-")]),
    // views.delete: the view goes, wherever it is.
    r("table.views.delete", r#"{"tableId":"$T","viewId":"$K"}"#, &[("$T", "-", "-"), ("$K", "*", "-")]),
    // putFilterTree / putSorts / putGroups: the view by id alone; the one setting replaced; each
    // condition's or item's field, whose name is written beside it when it is the view table's.
    // pinned: the table's existence (the view goes with it: equivalent) and `~owner`.
    r(
        "table.views.putFilterTree",
        r#"{"tableId":"$T","viewId":"$V","tree":{"conjunction":"and","children":[{"condition":{"field_id":"$Status","operator":"is","value":"Done"}}]}}"#,
        &[("$T", "-", "-"), ("$V", "~filterTree,~owner", "-"), ("$Status", "name,~owner", "-")],
    ),
    r("table.views.putFilterTree", r#"{"tableId":"$T","viewId":"$V","tree":null}"#, &[("$T", "-", "-"), ("$V", "~filterTree,~owner", "-")]),
    r(
        "table.views.putSorts",
        r#"{"tableId":"$T","viewId":"$V","items":[{"field_id":"$Est","direction":"asc"}]}"#,
        &[("$T", "-", "-"), ("$V", "~sorts,~owner", "-"), ("$Est", "name,~owner", "-")],
    ),
    r(
        "table.views.putGroups",
        r#"{"tableId":"$T","viewId":"$K","items":[{"field_id":"$Status","direction":"desc"}]}"#,
        &[("$T", "-", "-"), ("$K", "~groups,~owner", "-"), ("$Status", "name,~owner", "-")],
    ),
    // patchConfig: the view of the table; each of the three settings given. pinned: `~owner`.
    r("table.views.patchConfig", r#"{"tableId":"$T","viewId":"$V","row_height":"tall"}"#, &[("$T", "-", "-"), ("$V", "~rowHeight,~owner", "-")]),
    r(
        "table.views.patchConfig",
        r#"{"tableId":"$T","viewId":"$V","frozen_column_count":2}"#,
        &[("$T", "-", "-"), ("$V", "~frozenColumnCount,~owner", "-")],
    ),
    r("table.views.patchConfig", r#"{"tableId":"$T","viewId":"$V","search_query":"x"}"#, &[("$T", "-", "-"), ("$V", "~searchQuery,~owner", "-")]),
    // patchField: the view by id, the field by id; the one entry written. pinned: `~owner`, the
    // table, and the layout entry's key (`.position`).
    r(
        "table.views.patchField",
        r#"{"tableId":"$T","viewId":"$V","fieldId":"$Est","width":200}"#,
        &[("$T", "-", "-"), ("$V", "widths.$Est,~owner", "-"), ("$Est", "~owner", "-")],
    ),
    r(
        "table.views.patchField",
        r#"{"tableId":"$T","viewId":"$V","fieldId":"$Est","visible":false}"#,
        &[("$T", "-", "-"), ("$V", "hidden.$Est,~owner", "-"), ("$Est", "~owner", "-")],
    ),
    r(
        "table.views.patchField",
        r#"{"tableId":"$T","viewId":"$V","fieldId":"$Est","position":2}"#,
        &[("$T", "-", "-"), ("$V", "config.field_layout.$Est.position,~owner", "-"), ("$Est", "~owner", "-")],
    ),
    // records.create: appended; values keyed by field name (membership), written by the field's
    // type (a computed field's value is dropped); a select label must still be one of the
    // choices; a link value names target records and is written by the link
    // field's type and options, mirrored by its partner (its options and type, the target's
    // links in it). Required flags only refuse; readonly is not read.
    // pinned (spelling): `~names:*`, `options.choices:Done`, `~mirrors`, `~linkCells`, `~owner`;
    // the field's `type` (computed values are dropped).
    r("table.records.create", r#"{"tableId":"$T","values":{"title":"Four"}}"#, &[("$T", "~names:title", "-"), ("$Title", "name,type", "-")]),
    r(
        "table.records.create",
        r#"{"tableId":"$T","values":{"title":"Four","status":"Done"}}"#,
        &[("$T", "~names:status,~names:title", "-"), ("$Title", "name,type", "-"), ("$Status", "name,options.choices:Done,type", "-")],
    ),
    r(
        "table.records.create",
        r#"{"tableId":"$T","values":{"title":"Four","owner":["$P1"]}}"#,
        &[
            ("$T", "~names:owner,~names:title", "-"),
            ("$Title", "name,type", "-"),
            ("$Owner", "name,options,type", "-"),
            ("$PTasks", "options,type,~owner", "-"),
            ("$P1", "~owner", "-"),
            ("$Base", "~linkCells:$P1.$PTasks,~mirrors:$Owner", "-"),
        ],
    ),
    // records.update: the cells given replace the record's cells; `order` is a place among the
    // table's records; link values as in create, replacing the record's links of that field
    // (and the partner's links of the old and new targets). pinned as for records.create, and a
    // link field's stored cell.
    r(
        "table.records.update",
        r#"{"tableId":"$T","recordId":"$R1","values":{"notes":"x"}}"#,
        &[("$T", "~names:notes", "-"), ("$R1", "cells.$Notes,~owner", "-"), ("$Notes", "name,type", "-")],
    ),
    rw(
        "table.records.update",
        r#"{"tableId":"$T","recordId":"$R3","order":1}"#,
        &[("$T", "-", "-"), ("$R3", "~owner", "records")],
        &["$T.records[0..0]"],
    ),
    r(
        "table.records.update",
        r#"{"tableId":"$T","recordId":"$R1","values":{"owner":["$P2"]}}"#,
        &[
            ("$T", "~names:owner", "-"),
            ("$R1", "cells.$Owner,~owner", "-"),
            ("$Owner", "name,options,type", "-"),
            ("$PTasks", "options,type,~owner", "-"),
            ("$P2", "~owner", "-"),
            ("$Base", "~linkCells:$P1.$PTasks,~linkCells:$P2.$PTasks,~linkCells:$R1.$Owner,~mirrors:$Owner", "-"),
        ],
    ),
    // records.delete / bulkDelete: the records go, wherever they are, with their links (pinned:
    // `~linkCells` of the record).
    r("table.records.delete", r#"{"tableId":"$T","recordId":"$R2"}"#, &[("$T", "-", "-"), ("$R2", "*", "-"), ("$Base", "~linkCells:$R2", "-")]),
    r(
        "table.records.bulkDelete",
        r#"{"tableId":"$T","row_keys":["$R2","$R3"]}"#,
        &[("$T", "-", "-"), ("$R2", "*", "-"), ("$R3", "*", "-"), ("$Base", "~linkCells:$R2,~linkCells:$R3", "-")],
    ),
    // bulkCopy: the sources' stored cells (which fields are stored: their types);
    // `target_position` is a place among the records. pinned: `~owner`.
    r(
        "table.records.bulkCopy",
        r#"{"tableId":"$T","row_keys":["$R1"]}"#,
        &[("$T", "-", "-"), ("$R1", "cells,~owner", "-"), ("fields($T)", "type", "-")],
    ),
    rw(
        "table.records.bulkCopy",
        r#"{"tableId":"$T","row_keys":["$R1"],"target_position":1}"#,
        &[("$T", "-", "-"), ("$R1", "cells,~owner", "-"), ("fields($T)", "type", "-")],
        &["$T.records[0..0]"],
    ),
    // links.replace / links.delete: the record's links of the field replaced; the field's type
    // and options; each target a record of the target table; the partner (its options and type,
    // the targets' links in it). pinned (spelling) `~linkCells`, `~mirrors`, `~owner`.
    r(
        "table.links.replace",
        r#"{"tableId":"$T","recordId":"$R1","field_id":"$Owner","target_row_keys":["$P2"]}"#,
        &[
            ("$T", "-", "-"),
            ("$R1", "~owner", "-"),
            ("$Owner", "options,type,~owner", "-"),
            ("$PTasks", "options,type,~owner", "-"),
            ("$P2", "~owner", "-"),
            ("$Base", "~linkCells:$P1.$PTasks,~linkCells:$P2.$PTasks,~linkCells:$R1.$Owner,~mirrors:$Owner", "-"),
        ],
    ),
    r(
        "table.links.delete",
        r#"{"tableId":"$T","recordId":"$R1","field_id":"$Owner","target_row_keys":["$P1"]}"#,
        &[
            ("$T", "-", "-"),
            ("$R1", "~owner", "-"),
            ("$Owner", "options,type,~owner", "-"),
            ("$PTasks", "options,type,~owner", "-"),
            ("$P1", "~owner", "-"),
            ("$Base", "~linkCells:$P1.$PTasks,~linkCells:$R1.$Owner,~mirrors:$Owner", "-"),
        ],
    ),
    // transact: each op as its single command does; an anchor row key and `toIndex` are places.
    // pinned: the integer rule for `tableId` and `fieldIds` (local addresses: their windows), and
    // an anchor held by its own place instead of the gap before it (equivalent).
    rw(
        "table.transact",
        r#"{"ops":[{"op":"cells.set","tableId":"@T","records":[{"rowKey":"$R1","values":{"notes":"x"}}]}]}"#,
        &[("$T", "~names:notes", "-"), ("$R1", "cells.$Notes,~owner", "-"), ("$Notes", "name,type", "-")],
        &["$Base.tables[0..1]"],
    ),
    rw(
        "table.transact",
        r#"{"ops":[{"op":"records.create","tableId":"@T","at":{"before":"$R2"},"records":[{"title":"Four"}]}]}"#,
        &[("$T", "~names:title", "-"), ("$Title", "name,type", "-"), ("$R2", "~owner", "records")],
        &["$Base.tables[0..1]"],
    ),
    rw(
        "table.transact",
        r#"{"ops":[{"op":"records.delete","tableId":"@T","rowKeys":["$R3"]}]}"#,
        &[("$T", "-", "-"), ("$R3", "*", "-"), ("$Base", "~linkCells:$R3", "-")],
        &["$Base.tables[0..1]"],
    ),
    rw(
        "table.transact",
        r#"{"ops":[{"op":"records.move","tableId":"@T","rowKeys":["$R3"],"before":"$R1"}]}"#,
        &[("$T", "-", "-"), ("$R3", "~owner", "records"), ("$R1", "~owner", "records")],
        &["$Base.tables[0..1]"],
    ),
    rw(
        "table.transact",
        r#"{"ops":[{"op":"fields.move","tableId":"@T","fieldIds":["@Est"],"toIndex":0}]}"#,
        &[("$T", "-", "-"), ("$Est", "~owner", "fields")],
        &["$Base.tables[0..1]", "$T.fields[0..0]", "$T.fields[4..5]"],
    ),
    // csv.import: each header finds the first field in order whose name or title it is; cells
    // converted by the field's type (a link column's text is a row key: the link options);
    // appended. Required flags only refuse.
    // pinned: every field's name and title and their order (`~lists`), `~names`.
    r(
        "table.csv.import",
        r#"{"tableId":"$T","content":"title,notes\nA,B\n"}"#,
        &[
            ("$T", "~names:calc,~names:est,~names:look,~names:notes,~names:owner,~names:status,~names:summary,~names:tags,~names:title", "-"),
            ("fields($T)", "name,title,type", "-"),
            ("$Owner", "options", "-"),
            ("$Base", "~lists:$T.fields", "-"),
        ],
    ),
    // import.run: columns map to fields by title (every title decides which columns make new
    // fields, whose names are generated among the table's names; the last field of a title
    // wins), converted by the field's type; replace deletes every record and its links; upsert
    // reads the key cells and rewrites matching records' cells; a new table's name is generated
    // among the table names. pinned as for csv.import, and the upsert's exact cells.
    r(
        "table.import.run",
        r#"{"fileName":"a.csv","content":"dGl0bGUsbm90ZXMKQSxCCg==","write_mode":"append","schema_mode":"auto","target":{"kind":"existing_table","table_id":"$T"}}"#,
        &[
            ("$T", "~names:calc,~names:est,~names:look,~names:notes,~names:owner,~names:status,~names:summary,~names:tags,~names:title", "-"),
            ("fields($T)", "name,title,type", "-"),
            ("$Owner", "options", "-"),
            ("$Base", "~lists:$T.fields", "-"),
        ],
    ),
    r(
        "table.import.run",
        r#"{"fileName":"a.csv","content":"dGl0bGUsbm90ZXMKQSxCCg==","write_mode":"replace","schema_mode":"auto","target":{"kind":"existing_table","table_id":"$T"}}"#,
        &[
            ("$T", "~names:calc,~names:est,~names:look,~names:notes,~names:owner,~names:status,~names:summary,~names:tags,~names:title", "-"),
            ("fields($T)", "name,title,type", "-"),
            ("$Owner", "options", "-"),
            ("records($T)", "*", "-"),
            ("$Base", "~linkCells:$R1,~linkCells:$R2,~linkCells:$R3,~lists:$T.fields,~rowIds:$T", "-"),
        ],
    ),
    r(
        "table.import.run",
        r#"{"fileName":"a.csv","content":"dGl0bGUsbm90ZXMKT25lLEIK","write_mode":"upsert","unique_by":["title"],"schema_mode":"auto","target":{"kind":"existing_table","table_id":"$T"}}"#,
        &[
            ("$T", "~names:calc,~names:est,~names:look,~names:notes,~names:owner,~names:status,~names:summary,~names:tags,~names:title", "-"),
            ("fields($T)", "name,title,type", "-"),
            ("$Owner", "options", "-"),
            ("$R1", "cells.$Notes,cells.$Title,~owner", "-"),
            ("$R2", "cells.$Title,~owner", "-"),
            ("$R3", "cells.$Title,~owner", "-"),
            ("$Base", "~lists:$T.fields,~rowIds:$T", "-"),
        ],
    ),
    r(
        "table.import.run",
        r#"{"fileName":"a.csv","content":"dGl0bGUsbm90ZXMKQSxCCg==","write_mode":"append","schema_mode":"auto","target":{"kind":"new_table","table_title":"Imported"}}"#,
        &[("$Base", "~tableNames:imported", "-")],
    ),
    // ai.apply: the AI field's settings, its source field (by id or name: its name, title, type)
    // and the source cell the result was generated from; the AI cell is replaced.
    // pinned: the source's `title` (in the prompt), `~names`, `~owner`, and `@R1`'s window (integer rule).
    rw(
        "table.ai.apply",
        r#"{"tableId":"$T","fieldId":"$Summary","recordId":"$R1","results":[{"recordId":"@R1","text":"S"}]}"#,
        &[
            ("$T", "~names:notes", "-"),
            ("$Summary", "aiPrompt,name,options,title,type,~owner", "-"),
            ("$Notes", "name,title,type", "-"),
            ("$R1", "cells.$Notes,cells.$Summary,~owner", "-"),
        ],
        &["$T.records[0..1]"],
    ),
    // attachments.put: a blob only; the table must exist.
    r("table.attachments.put", r#"{"tableId":"$T","name":"a.txt","mimeType":"text/plain","data":"aGk="}"#, &[("$T", "-", "-")]),
    // undo / redo: the session's own steps; they name no object.
    r("table.undo", r#"{}"#, &[]),
    r("table.redo", r#"{}"#, &[]),
    r("edit.undo", r#"{}"#, &[]),
    r("edit.redo", r#"{}"#, &[]),
    // dashboards.upsert replaces name, config and widgets wherever the dashboard is; the source
    // table exists. dashboards.delete: the dashboard goes.
    r(
        "table.dashboards.upsert",
        r#"{"dashboardId":"$D","title":"O2","order":1,"source_table_id":"$T","layout":[],"widgets":[]}"#,
        &[("$D", "*", "-"), ("$T", "-", "-")],
    ),
    r(
        "table.dashboards.upsert",
        r#"{"dashboardId":"d2","title":"New","order":2,"source_table_id":"$T","layout":[],"widgets":[]}"#,
        &[("$T", "-", "-")],
    ),
    r("table.dashboards.delete", r#"{"dashboardId":"$D"}"#, &[("$D", "*", "-")]),
    // Further code paths, written after the first comparison (when the full suite showed widget
    // references), before their own: a widget is stored as given, so a field it names by id need
    // only be there, and one it names by name is the table's name.
    r(
        "table.dashboards.upsert",
        r#"{"dashboardId":"$D","title":"O2","order":1,"source_table_id":"$T","layout":[],"widgets":[{"id":"w","type":"metric","query":{"field_id":"$Est"}}]}"#,
        &[("$D", "*", "-"), ("$T", "-", "-"), ("$Est", "~owner", "-")],
    ),
    r(
        "table.dashboards.upsert",
        r#"{"dashboardId":"$D","title":"O2","order":1,"source_table_id":"$T","layout":[],"widgets":[{"id":"w","type":"metric","query":{"field_name":"est"}}]}"#,
        &[("$D", "*", "-"), ("$T", "~names:est", "-"), ("$Est", "-", "-")],
    ),
];

/// The commands whose declarations still differ from the reading: may only go down.
const DIFFERING_FLOOR: usize = 0;
/// Per path (each row, in each spelling): the rows that differ, those that hold less than the
/// reading in some fact (an object, field, position, window or editor state the reading needs
/// and the declaration lacks), and those that hold more. Each may only go down; less stays 0.
const ROWS_DIFFERING_FLOOR: usize = 0;
const HELD_LESS_FLOOR: usize = 0;
const HELD_MORE_FLOOR: usize = 0;

/// Whether the spelled facts `a` include every fact of `b` (`*`: every fact; `-`: none).
fn includes(a: &str, b: &str) -> bool {
    if a == "*" {
        return true;
    }
    if b == "-" {
        return true;
    }
    if b == "*" {
        return false;
    }
    let a: BTreeSet<&str> = a.split(',').collect();
    b.split(',').all(|fact| a.contains(fact))
}

/// How one path's declaration compares with its reading: (holds less, holds more).
fn classes(got: &Held, expected: &BTreeMap<String, (String, String)>, windows: &BTreeSet<String>, editor: &str) -> (bool, bool) {
    let none = ("-".to_owned(), "-".to_owned());
    let mut less = !got.windows.is_superset(windows) || !includes(&got.editor, if editor.is_empty() { "-" } else { editor });
    let mut more = !windows.is_superset(&got.windows)
        || !includes(if editor.is_empty() { "-" } else { editor }, if got.editor.is_empty() { "-" } else { &got.editor });
    for role in got.holds.keys().chain(expected.keys()).collect::<BTreeSet<_>>() {
        let (g, e) = (got.holds.get(role).unwrap_or(&none), expected.get(role).unwrap_or(&none));
        less |= !includes(&g.0, &e.0) || !includes(&g.1, &e.1);
        more |= !includes(&e.0, &g.0) || !includes(&e.1, &g.1);
    }
    (less, more)
}

/// The window of the list an object is in, at its index (`$T.records[0..1]`): what a local
/// integer address rests on (integers are local projection addresses).
fn window_of(doc: &TableDocument, roles: &[Role], role: &str) -> Option<String> {
    let replica = doc.file().replica();
    let tool = &roles.iter().find(|r| r.role == role)?.tool;
    for parent in replica.objects_of_type("Base").into_iter().chain(replica.objects_of_type("Table")) {
        for list in ["tables", "fields", "views", "records"] {
            if let Ok(items) = replica.list(&parent, list)
                && let Some(at) = items.iter().position(|item| item.as_str() == tool)
            {
                let parent = roles.iter().find(|r| r.tool == parent.as_str()).map_or("?", |r| r.role);
                return Some(format!("{parent}.{list}[{at}..{}]", at + 1));
            }
        }
    }
    None
}

/// The rows' parameters with every table, field, view and record given by its local integer.
fn integers(params: &str, roles: &[Role]) -> (String, Vec<&'static str>) {
    let mut out = params.to_owned();
    let mut named = Vec::new();
    let mut by_length: Vec<&Role> = roles.iter().filter(|r| r.local.is_some()).collect();
    by_length.sort_by_key(|r| std::cmp::Reverse(r.role.len()));
    for role in by_length {
        let quoted = format!("\"{}\"", role.role);
        let local = format!("\"@{}\"", &role.role[1..]);
        if out.contains(&quoted) || out.contains(&local) {
            out = out.replace(&quoted, &local);
            named.push(role.role);
        }
    }
    (out, named)
}

/// Rows that differ, hold less, and hold more (per path), beside the lines and commands.
#[derive(Default, Debug)]
struct Counts {
    differing: usize,
    less: usize,
    more: usize,
}

fn compare(doc: &TableDocument, roles: &[Role], integer: bool, counts: &mut Counts) -> (Vec<String>, BTreeSet<&'static str>) {
    let commands = doc.commands();
    let mut wrong = Vec::new();
    let mut differing = BTreeSet::new();
    for row in ROWS {
        let command = commands.iter().find(|c| c.id == row.command).unwrap_or_else(|| panic!("no command {}", row.command));
        let (text, named) = if integer { integers(row.params, roles) } else { (row.params.to_owned(), Vec::new()) };
        let params = with_roles(&serde_json::from_str(&text).unwrap_or_else(|e| panic!("{text}: {e}")), roles);
        let got = held(doc, command, &params, roles);
        let expected = expanded(row.holds);
        let mut windows: BTreeSet<String> = row.windows.iter().map(|w| (*w).to_owned()).collect();
        windows.extend(named.iter().filter_map(|role| window_of(doc, roles, role)));
        if got.editor != row.editor || got.holds != expected || got.windows != windows {
            let (less, more) = classes(&got, &expected, &windows, row.editor);
            counts.differing += 1;
            counts.less += usize::from(less);
            counts.more += usize::from(more);
            differing.insert(row.command);
            let mut lines = Vec::new();
            for role in got.holds.keys().chain(expected.keys()).collect::<BTreeSet<_>>() {
                let (g, e) = (got.holds.get(role), expected.get(role));
                if g != e {
                    lines.push(format!("    {role}: declared {g:?} reading {e:?}"));
                }
            }
            if got.windows != windows {
                lines.push(format!("    windows: declared {:?} reading {windows:?}", got.windows));
            }
            if got.editor != row.editor {
                lines.push(format!("    editor: declared {} reading {}", got.editor, row.editor));
            }
            wrong.push(format!("{} {text}\n{}", row.command, lines.join("\n")));
        }
    }
    (wrong, differing)
}

#[test]
fn every_write_rests_on_what_its_handler_reads() {
    let (doc, roles) = fixture();
    let commands = doc.commands();
    let mut counts = Counts::default();
    let (mut wrong, mut differing) = compare(&doc, &roles, false, &mut counts);
    let (integer, integer_differing) = compare(&doc, &roles, true, &mut counts);
    wrong.extend(integer.into_iter().map(|line| format!("(integer ids) {line}")));
    differing.extend(integer_differing);
    let rows: BTreeSet<&str> = ROWS.iter().map(|row| row.command).collect();
    let writes: BTreeSet<&str> = commands.iter().filter(|c| c.mutates).map(|c| c.id.as_str()).collect();
    let mut missing = Vec::new();
    for command in writes.difference(&rows) {
        missing.push(format!("{command}: a write command with no reading"));
    }
    for command in rows.difference(&writes) {
        missing.push(format!("{command}: a reading of no write command"));
    }
    println!("{}\n{} rows differ, of {} commands; {} write commands", wrong.join("\n"), wrong.len(), differing.len(), writes.len());
    println!("differing: {differing:?}");
    println!("paths: {} rows in two spellings; differing {}, held less {}, held more {}", ROWS.len() * 2, counts.differing, counts.less, counts.more);
    assert!(missing.is_empty(), "{}", missing.join("\n"));
    // The floor only goes down; at 0 the comparison is exact.
    #[allow(clippy::absurd_extreme_comparisons)]
    let within = differing.len() <= DIFFERING_FLOOR;
    assert!(within, "{} commands differ from the reading, more than {DIFFERING_FLOOR}", differing.len());
    #[allow(clippy::absurd_extreme_comparisons)]
    let floors = counts.differing <= ROWS_DIFFERING_FLOOR && counts.less <= HELD_LESS_FLOOR && counts.more <= HELD_MORE_FLOOR;
    assert!(floors, "{counts:?} above the floors ({ROWS_DIFFERING_FLOOR}, {HELD_LESS_FLOOR}, {HELD_MORE_FLOOR})");
}

/// The raw cell write's premise contract (`model.cell`, `batchOps` of `Record` in
/// `model/schema.json`): `map.set` of `$R1`'s `cells` under `$Notes`' id writes that one cell,
/// is projected only while the field exists, and its value is read through the field's `type`
/// and `options`, so it rests on that cell and on those two facts of
/// `$Notes`: no position, no other cell, not the field's name or title. A key that is no field
/// holds only the cell.
#[test]
fn the_raw_cell_contract_rests_on_its_cell_and_its_field() {
    let (doc, roles) = fixture();
    let tool = |role: &str| roles.iter().find(|r| r.role == role).unwrap().tool.clone();
    let (record, field) = (tool("$R1"), tool("$Notes"));
    let under = doc.under("model.cell", &json!({"target":record,"key":field})).unwrap().unwrap();
    type Facts = (Option<Vec<String>>, Option<Vec<String>>);
    let holds: BTreeMap<String, Facts> =
        under.premises.iter().map(|p| (p.id.as_str().unwrap().to_owned(), (p.fields.clone(), p.positions.clone()))).collect();
    let expected = BTreeMap::from([
        (record.clone(), (Some(vec![format!("cells.{field}")]), Some(vec![]))),
        (field.clone(), (Some(vec!["options".to_owned(), "type".to_owned()]), Some(vec![]))),
    ]);
    assert_eq!(holds, expected);
    assert!(under.objects.is_empty() && under.within.is_empty() && under.covers.is_none());
    let under = doc.under("model.cell", &json!({"target":record,"key":"no-such-field"})).unwrap().unwrap();
    assert_eq!(under.premises.len(), 1);
    assert_eq!(under.premises[0].fields, Some(vec!["cells.no-such-field".to_owned()]));
}

/// Fixed: (`crates/engine/api/src/dispatch.rs`, `Params::view`;
/// `crates/engine/file/src/view_config.rs`, `assert_field_exists`): the view settings
/// (`putFilterTree`, `putSorts`, `putGroups`, `patchField`) parsed `tableId` and dropped it, so a
/// call naming `$T` wrote the settings of `$P`'s view; `patchField` took a field of any table as
/// a column of the view, stored its width and answered a default row.
#[test]
fn a_view_setting_lands_only_on_a_view_of_the_table_it_names() {
    let (mut doc, roles) = fixture();
    let other = with_roles(&json!("@PV"), &roles);
    let est = with_roles(&json!("@Est"), &roles);
    for (command, params) in [
        ("table.views.putSorts", json!({"tableId":1,"viewId":other,"items":[{"field_id":est,"direction":"asc"}]})),
        ("table.views.putGroups", json!({"tableId":1,"viewId":other,"items":[{"field_id":est,"direction":"asc"}]})),
        ("table.views.putFilterTree", json!({"tableId":1,"viewId":other,"tree":null})),
        ("table.views.patchField", json!({"tableId":1,"viewId":other,"fieldId":est,"width":300})),
        ("table.views.patchField", json!({"tableId":1,"viewId":1,"fieldId":with_roles(&json!("@Name"), &roles),"width":300})),
    ] {
        let before = doc.save(None).unwrap();
        assert!(doc.execute(command, params.clone()).is_err(), "{command} {params} wrote another table's view");
        assert_eq!(doc.save(None).unwrap(), before, "{command}");
    }
}

/// The per-path classes count what they say: a missing fact is held less, an extra one more.
#[test]
fn the_path_classes_tell_less_from_more() {
    let held = |holds: &[(&str, &str, &str)]| Held { editor: String::new(), holds: expanded(holds), windows: BTreeSet::new() };
    let reading = expanded(&[("$R1", "cells.$Notes", "-")]);
    let none = BTreeSet::new();
    assert_eq!(classes(&held(&[("$R1", "cells.$Notes", "-")]), &reading, &none, ""), (false, false));
    assert_eq!(classes(&held(&[("$R1", "*", "-")]), &reading, &none, ""), (false, true));
    assert_eq!(classes(&held(&[("$R1", "-", "-")]), &reading, &none, ""), (true, false));
    assert_eq!(classes(&held(&[("$R1", "cells.$Title", "*")]), &reading, &none, ""), (true, true));
    assert_eq!(classes(&held(&[]), &reading, &BTreeSet::from(["$T.records[0..1]".to_owned()]), ""), (true, false));
}
