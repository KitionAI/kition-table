//! Undo scopes (KitModel 1.1 M12a, engine rule E14a): a person's handle undoes only the person's
//! steps and an agent's handle only the agent's, whichever is on top; `undo_in` names the scope.

use kition_kdm::{Origin, UndoScope};
use kition_table_file::{CreateOptions, TableFile, TableSpec};

fn ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|err| panic!("unexpected error: {err}"))
}

fn person() -> Origin {
    Origin::person("user:test")
}

fn agent() -> Origin {
    Origin::agent("agent:test")
}

fn file(dir: &tempfile::TempDir) -> TableFile {
    let mut file = ok(TableFile::create(&dir.path().join("scopes.ktable"), &CreateOptions { title: "Doc".into(), ..CreateOptions::default() }));
    file.set_origin(person());
    file
}

/// Creates a table as `origin` and returns its title.
fn table(file: &mut TableFile, origin: Origin, title: &str) -> String {
    file.set_origin(origin);
    ok(file.create_table(&TableSpec { title: title.into(), ..TableSpec::default() }));
    file.set_origin(person());
    title.to_owned()
}

fn titles(file: &TableFile) -> Vec<String> {
    ok(file.list_tables()).into_iter().map(|table| table.title).collect()
}

#[test]
fn the_persons_undo_keeps_the_agents_edit() {
    let dir = ok(tempfile::tempdir());
    let mut file = file(&dir);
    let mine = table(&mut file, person(), "Mine");
    let theirs = table(&mut file, agent(), "Theirs");
    assert!(ok(file.undo()).0, "the person's handle undoes the person's step");
    assert_eq!(titles(&file), [theirs], "and keeps the agent's step on top of it");
    assert!(!ok(file.undo()).0, "the person has no other step");
    assert!(!titles(&file).contains(&mine));
}

#[test]
fn an_agent_scope_undo_takes_back_the_agents_edit() {
    let dir = ok(tempfile::tempdir());
    let mut file = file(&dir);
    table(&mut file, agent(), "Theirs");
    assert!(!file.can_undo(), "the person has no step");
    assert!(file.can_step_in(UndoScope::Agent, true));
    assert!(ok(file.undo_in(UndoScope::Agent)).0);
    assert!(titles(&file).is_empty());
    assert!(ok(file.redo_in(UndoScope::Agent)).0);
    assert_eq!(titles(&file), ["Theirs"]);
    // An agent's handle undoes its own scope.
    file.set_origin(agent());
    assert!(ok(file.undo()).0);
    assert!(titles(&file).is_empty());
}

#[test]
fn an_agent_undo_under_the_persons_later_step_keeps_the_persons_edit() {
    let dir = ok(tempfile::tempdir());
    let mut file = file(&dir);
    table(&mut file, agent(), "Theirs");
    let mine = table(&mut file, person(), "Mine");
    file.set_origin(agent());
    assert!(ok(file.undo()).0, "the agent's undo takes back its own step");
    assert_eq!(titles(&file), [mine], "and keeps the person's later one");
}
