//! Opening and creating `.ktable` files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kition_kdm::{DocId, ObjectId, Origin, Replica, Version};
use kition_time::{Millis, now_millis};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::kdm::model::BASE;
use crate::kdm::model::{self, BaseSpec};
use crate::kdm::project::{self, TableChange};
use crate::kdm::repair::RepairIndex;
use crate::kdm::{repair, schema, store};
use crate::schema::{INDEXES, TABLES};
use crate::{Result, TableFileError};

pub const FORMAT: &str = "kition-table";
pub const FORMAT_VERSION: i64 = 1;
/// The file's own schema: the file holds its table document's KDM replica, and the relational
/// tables are its projection; every timestamp is INTEGER Unix milliseconds. A file of another
/// version (one written before the schema freeze) is not read: there is no upgrade.
pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Default, Clone)]
pub struct CreateOptions {
    pub title: String,
    pub document_id: String,
    pub description: String,
    pub icon: String,
    pub color: String,
    pub meta_json: String,
    pub created_at: Option<Millis>,
    pub updated_at: Option<Millis>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub path: PathBuf,
    pub document_id: String,
    pub title: String,
    pub format: String,
    pub format_version: i64,
    pub schema_version: i64,
    pub created_at: Millis,
    pub updated_at: Millis,
}

/// An open `.ktable` file: one SQLite connection and the file's KDM replica.
///
/// Every mutation is one transaction of the replica, stored (`kdm::store`) and projected onto
/// the relational tables (`kdm::project`) in one SQL transaction; reads go to the relational
/// tables. See [`crate::kdm::commit`].
pub struct TableFile {
    pub(crate) conn: Connection,
    pub(crate) path: PathBuf,
    pub(crate) replica: Replica,
    pub(crate) origin: Origin,
    /// The stored replica's version: the `since` of the next append.
    pub(crate) since: Version,
    /// The store's head as this handle last read or wrote it. [`TableFile::compact_log`] moves it
    /// through a shared handle.
    pub(crate) head: std::cell::RefCell<store::Head>,
    pub(crate) base: Option<ObjectId>,
    /// The bases and links incremental repairs need, kept from write to write.
    pub(crate) repair_index: repair::RepairIndex,
    /// The file's identity right after this handle's open or last own write.
    pub(crate) identity: Option<FileIdentity>,
    pub(crate) last_change: Option<TableChange>,
    /// A failed write could not reload the replica; the next write reloads it first.
    pub(crate) stale: bool,
    /// Why this handle refuses writes, when it does.
    pub(crate) read_only: Option<String>,
    /// The earliest version the replica's own history reaches: where a replica opened from a
    /// content file starts (it holds no history before its seal), the empty version otherwise.
    pub(crate) floor: Version,
    /// The replica version this handle's session started at (its open, or the stored replica it
    /// adopted): the record history rows (`kition_record_history`) hold what came after it, and
    /// [`TableFile::stored_record_history`] derives what came before it from the replica.
    pub(crate) session_start: Version,
}

/// What tells one file on disk from another, or from itself after a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    dev: u64,
    ino: u64,
    mtime: Option<std::time::SystemTime>,
    size: u64,
}

impl FileIdentity {
    pub(crate) fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        let (dev, ino) = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino())
        };
        #[cfg(not(unix))]
        let (dev, ino) = (0, 0);
        Some(Self { dev, ino, mtime: meta.modified().ok(), size: meta.len() })
    }
}

impl std::fmt::Debug for TableFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableFile").field("path", &self.path).finish()
    }
}

impl TableFile {
    /// Creates a new file; appends `.ktable` when `path` has no extension.
    pub fn create(path: &Path, options: &CreateOptions) -> Result<Self> {
        let mut path = path.to_path_buf();
        if path.as_os_str().is_empty() {
            return Err(TableFileError::invalid("ktable path is required"));
        }
        if path.extension().is_none() {
            path.set_extension("ktable");
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() {
            return Err(TableFileError::Exists);
        }
        let result = (|| {
            let (conn, absolute) = open_sqlite(&path)?;
            Self::create_on(conn, absolute, options)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        result
    }

    /// Creates a new document held in memory, not in a file: [`TableFile::path`] is empty, and
    /// [`TableFile::to_bytes`] answers what [`TableFile::create`] would have written.
    pub fn create_in_memory(options: &CreateOptions) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Self::create_on(conn, PathBuf::new(), options)
    }

    /// Writes a new document into the empty database `conn` (at `path`, empty in memory).
    fn create_on(mut conn: Connection, path: PathBuf, options: &CreateOptions) -> Result<Self> {
        create_schema(&mut conn)?;
        let (doc_id, spec) = initialize_document(&mut conn, &path, options)?;
        let (replica, _) = Replica::create_with(schema(), doc_id, crate::ids::actor(), &Origin::system("create"), |tx| {
            model::create_base(tx, &spec).map_err(|err| kition_kdm::KdmError::Engine(err.to_string()))
        })?;
        {
            let tx = conn.transaction()?;
            let now = now_millis();
            store::save_initial(&tx, &replica, now)?;
            project::rebuild(&tx, &replica, now)?;
            let position = store::stored_position(&tx)?;
            store::set_projected_position(&tx, position)?;
            tx.commit()?;
        }
        Self::attach(conn, path)
    }

    /// Creates a new file whose replica is the document `changes` holds (a full export, or a
    /// snapshot, of another replica: [`Replica::export_changes`] from [`Version::empty`]), under a
    /// new actor: the same KDM document, so the two exchange changes from then on. The relational
    /// tables are projected from it, the document's own `document_id` is its KDM id, and nothing
    /// is an undo step. Appends `.ktable` when `path` has no extension; refuses
    /// ([`TableFileError::Exists`]) a path that exists, and bytes that are no KDM export of a table
    /// document ([`TableFileError::Kdm`]), leaving no file behind.
    pub fn create_from_replica(path: &Path, changes: &[u8]) -> Result<Self> {
        let mut path = path.to_path_buf();
        if path.as_os_str().is_empty() {
            return Err(TableFileError::invalid("ktable path is required"));
        }
        if path.extension().is_none() {
            path.set_extension("ktable");
        }
        // Checked before anything touches the disk: refused bytes must not leave a file.
        let replica = Replica::load(schema(), changes, crate::ids::actor())?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() {
            return Err(TableFileError::Exists);
        }
        let result = (|| {
            let (conn, absolute) = open_sqlite(&path)?;
            Self::from_replica_on(conn, absolute, &replica)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        result
    }

    /// [`TableFile::create_from_replica`] held in memory, not in a file: [`TableFile::path`] is
    /// empty, and [`TableFile::to_bytes`] answers what `create_from_replica` would have written.
    pub fn create_from_replica_in_memory(changes: &[u8]) -> Result<Self> {
        let replica = Replica::load(schema(), changes, crate::ids::actor())?;
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Self::from_replica_on(conn, PathBuf::new(), &replica)
    }

    /// Writes the document `replica` holds into the empty database `conn` (at `path`, empty in
    /// memory): its `document_id` is the replica's KDM id and the tables are projected from it.
    fn from_replica_on(mut conn: Connection, path: PathBuf, replica: &Replica) -> Result<Self> {
        create_schema(&mut conn)?;
        {
            let tx = conn.transaction()?;
            let now = now_millis();
            set_meta(&tx, "document_id", &replica.doc_id().to_string())?;
            set_meta(&tx, "created_at", &now.to_string())?;
            set_meta(&tx, "updated_at", &now.to_string())?;
            store::save_initial(&tx, replica, now)?;
            project::rebuild(&tx, replica, now)?;
            let position = store::stored_position(&tx)?;
            store::set_projected_position(&tx, position)?;
            tx.commit()?;
        }
        Self::attach(conn, path)
    }

    /// Opens an existing file; a projection behind the stored replica is rebuilt. A file of
    /// another schema version, or without a replica, is refused ([`TableFileError::Invalid`]).
    ///
    /// A file that cannot be written (read-only permissions or mount), or whose projection cannot
    /// be brought up to date, opens read-only: it reads as it is, and every write is refused
    /// ([`TableFileError::ReadOnly`]).
    pub fn open(path: &Path) -> Result<Self> {
        if path.as_os_str().is_empty() {
            return Err(TableFileError::invalid("ktable path is required"));
        }
        std::fs::metadata(path)?;
        let (conn, absolute) = open_sqlite(path)?;
        let cannot_write = conn.is_readonly(rusqlite::MAIN_DB)?;
        Self::open_on(conn, absolute, cannot_write)
    }

    /// Opens a `.ktable` file's bytes (what [`TableFile::to_bytes`] answers, or the file on
    /// disk) as a document held in memory, exactly as [`TableFile::open`] opens a file: a
    /// projection behind the stored replica is rebuilt, and a file that cannot be brought up to
    /// date opens read-only. Nothing is written
    /// anywhere; [`TableFile::path`] is empty.
    ///
    /// The bytes are copied page by page into an in-memory database ([`Connection::open_in_memory`],
    /// as [`TableFile::create_in_memory`] holds a new document), not kept as one block of memory
    /// (`sqlite3_deserialize`): such a block is reallocated at twice its size when the document
    /// grows, and refuses to grow past 1 GiB.
    pub fn open_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(TableFileError::invalid("ktable bytes are empty"));
        }
        let conn = memory_copy(bytes)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Self::open_on(conn, PathBuf::new(), false)
    }

    /// The whole database as the bytes of a `.ktable` file: what a host that keeps documents in
    /// memory writes to its storage, and [`TableFile::open_bytes`] reads back.
    ///
    /// The pages are read one at a time (`sqlite_dbpage`, compiled in by `.cargo/config.toml`)
    /// straight into the answer: the bytes `sqlite3_serialize` answers, without its copy of the
    /// whole document, which for a large table is more memory than the page cache leaves.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        database_bytes(&self.conn)
    }

    /// [`TableFile::open`]'s steps once the database is connected.
    fn open_on(conn: Connection, absolute: PathBuf, cannot_write: bool) -> Result<Self> {
        // Checked before anything is written: a database that is no table file of this format
        // (another SQLite file, or a file of another version) is refused untouched.
        check_version(&conn)?;
        if cannot_write {
            let mut file = Self::attach(conn, absolute)?;
            file.read_only = Some("the file cannot be written".into());
            return Ok(file);
        }
        if let Err(err) = refresh_projection(&conn) {
            tracing::warn!(path = %absolute.display(), error = %err, "ktable projection could not be rebuilt; opening read-only");
            let mut file = Self::attach(conn, absolute)?;
            file.read_only = Some(format!("the file could not be brought up to date: {err}"));
            return Ok(file);
        }
        Self::attach(conn, absolute)
    }

    /// Loads the stored replica and the store's head together (one read transaction).
    fn attach(conn: Connection, path: PathBuf) -> Result<Self> {
        let (head, replica) = load_consistent(&conn)?;
        let base = find_base(&conn, &replica)?;
        Ok(Self {
            since: replica.version(),
            identity: FileIdentity::of(&path),
            // Built by the first write that repairs: it reads every object.
            repair_index: RepairIndex::default(),
            floor: floor_of(&replica),
            session_start: replica.version(),
            conn,
            path,
            replica,
            origin: Origin::system("unknown"),
            head: std::cell::RefCell::new(head),
            base,
            last_change: None,
            stale: false,
            read_only: None,
        })
    }

    /// Why this handle refuses writes; `None` when it accepts them.
    pub fn read_only(&self) -> Option<&str> {
        self.read_only.as_deref()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// The file's KDM replica, as of this handle's last read or write of the store.
    pub fn replica(&self) -> &Replica {
        &self.replica
    }

    /// The document's base object (the one its writes go to), when it has one. Read this rather
    /// than looking for the base among the replica's objects, which decodes every record of a
    /// document loaded from a snapshot.
    pub fn base_object(&self) -> Option<&ObjectId> {
        self.base.as_ref()
    }

    /// A read view of the replica that knows the document's base.
    pub(crate) fn model_view(&self) -> crate::kdm::read::ModelView<'_> {
        match &self.base {
            Some(base) => crate::kdm::read::ModelView::with_base(&self.replica, base.clone()),
            None => crate::kdm::read::ModelView::new(&self.replica),
        }
    }

    /// Closes the file after letting SQLite refresh its statistics.
    pub fn close(self) {
        let _ = self.conn.execute_batch("PRAGMA optimize");
    }

    pub fn info(&self) -> Result<Info> {
        let meta = self.meta()?;
        let get = |key: &str| meta.get(key).cloned().unwrap_or_default();
        let document_id = get("document_id");
        let title = self
            .conn
            .query_row("SELECT title FROM kition_documents WHERE id = ?1 AND deleted_at IS NULL", [&document_id], |row| row.get::<_, String>(0))
            .optional()?
            .unwrap_or_default();
        Ok(Info {
            path: self.path.clone(),
            document_id,
            title,
            format: get("format"),
            format_version: atoi(&get("format_version")),
            schema_version: atoi(&get("schema_version")),
            created_at: atoi(&get("created_at")),
            updated_at: atoi(&get("updated_at")),
        })
    }

    pub fn meta(&self) -> Result<BTreeMap<String, String>> {
        let mut statement = self.conn.prepare("SELECT key, value FROM kition_meta")?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        Ok(rows)
    }

    pub fn get_meta(&self, key: &str) -> Result<String> {
        Ok(self.conn.query_row("SELECT value FROM kition_meta WHERE key = ?1", [key], |row| row.get(0))?)
    }

    /// Writes one key of the file's own bookkeeping (`kition_meta`: format, versions, ...).
    /// This is not part of the table document, so it is no KDM transaction.
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.writable()?;
        set_meta(&self.conn, key, value)
    }
}

/// The document's base: the one the projection recorded while it is still a Base, else the first
/// Base object. Finding it by its type reads every object of the replica, which for a table loaded
/// from a snapshot means decoding every record: most of an open's time on a large table.
pub(crate) fn find_base(conn: &Connection, replica: &Replica) -> Result<Option<ObjectId>> {
    if let Some(id) = get_meta(conn, project::BASE_KEY)?.map(ObjectId::from_string)
        && replica.object_type(&id).is_ok_and(|ty| ty == BASE)
    {
        return Ok(Some(id));
    }
    Ok(replica.objects_of_type(BASE).into_iter().next())
}

/// The database `bytes` (a `.ktable` file) as a database in memory, copied page by page (see
/// [`TableFile::open_bytes`]), foreign keys off as the copy leaves them.
pub(crate) fn memory_copy(bytes: &[u8]) -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    {
        let mut source = Connection::open_in_memory()?;
        source.deserialize_read_exact(rusqlite::MAIN_DB, bytes, bytes.len(), true)?;
        // An in-memory database takes a copy only at its own page size.
        let page_size: i64 = source.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        conn.execute_batch(&format!("PRAGMA page_size = {page_size}"))?;
        // The copy allocates its pages one at a time, each with the page cache's header (a
        // few hundred bytes) on top of the page: make room for all of them at once (see
        // `reserve_memory`).
        let pages = bytes.len() / usize::try_from(page_size).unwrap_or(4096).max(512);
        reserve_memory(bytes.len().saturating_add(pages.saturating_mul(512)));
        let copy = rusqlite::backup::Backup::new(&source, &mut conn)?;
        if copy.step(-1)? != rusqlite::backup::StepResult::Done {
            return Err(TableFileError::invalid("ktable bytes could not be read"));
        }
    }
    Ok(conn)
}

/// The whole database `conn` as the bytes of a file, a page at a time (see [`TableFile::to_bytes`]).
pub(crate) fn database_bytes(conn: &Connection) -> Result<Vec<u8>> {
    let pages: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    let size = usize::try_from(pages.saturating_mul(page_size)).map_err(|_| TableFileError::invalid("the ktable database is too large"))?;
    let mut bytes = Vec::with_capacity(size);
    let mut statement = conn.prepare("SELECT data FROM sqlite_dbpage('main') ORDER BY pgno").map_err(|err| {
        TableFileError::invalid(format!("this build's SQLite has no sqlite_dbpage (SQLITE_ENABLE_DBPAGE_VTAB, set in .cargo/config.toml): {err}"))
    })?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        bytes.extend_from_slice(row.get_ref(0)?.as_blob().map_err(|_| TableFileError::invalid("a database page is not a blob"))?);
    }
    Ok(bytes)
}

/// Makes the heap `bytes` larger in one step, ahead of many small allocations that need about as
/// much. In WebAssembly the allocator grows linear memory by 64 KiB whenever it runs out, and once
/// that memory is large the browser answers every growth with a full garbage collection (V8:
/// "external memory pressure"): growing by the hundreds of megabytes a large table's pages take,
/// 64 KiB at a time, cost seconds. A block taken and given back stays with the allocator, which
/// hands it out again.
fn reserve_memory(bytes: usize) {
    drop(std::hint::black_box(Vec::<u8>::with_capacity(bytes)));
}

/// The earliest version `replica`'s history reaches ([`TableFile::floor`]).
pub(crate) fn floor_of(replica: &Replica) -> Version {
    if replica.is_shallow() { replica.version() } else { Version::empty() }
}

/// Opens the SQLite database at `path`; answers the connection and the absolute path.
pub(crate) fn open_sqlite(path: &Path) -> Result<(Connection, PathBuf)> {
    let absolute = std::path::absolute(path)?;
    let conn = Connection::open(&absolute)?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = NORMAL;",
    )?;
    Ok((conn, absolute))
}

/// Rebuilds the projection when it is not at the store's head, under the write lock (positions
/// re-checked and the replica loaded inside it, so a concurrent writer cannot be rolled back).
fn refresh_projection(conn: &Connection) -> Result<()> {
    if store::projection_is_current(conn)? {
        return Ok(());
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    if !store::projection_is_current(&tx)? {
        let replica = load_replica(&tx)?;
        project::rebuild(&tx, &replica, now_millis())?;
        store::set_projected_position(&tx, store::stored_position(&tx)?)?;
    }
    tx.commit()?;
    Ok(())
}

/// The store's head and its replica, read in one transaction so they agree.
pub(crate) fn load_consistent(conn: &Connection) -> Result<(store::Head, Replica)> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred)?;
    let head = store::head(&tx)?;
    let replica = load_replica(&tx)?;
    tx.commit()?;
    Ok((head, replica))
}

/// Loads the stored replica with a new random actor.
pub(crate) fn load_replica(conn: &Connection) -> Result<Replica> {
    store::load(conn, schema(), crate::ids::actor())?.ok_or_else(|| TableFileError::Kdm("the ktable file holds no kdm replica".into()))
}

/// Refuses a database that is no `.ktable` file this build reads: another SQLite file, a file of
/// another schema version, or one without its table document's replica.
pub(crate) fn check_version(conn: &Connection) -> Result<()> {
    let version = schema_version(conn)?;
    if version == 0 {
        return Err(TableFileError::invalid("not a ktable file"));
    }
    validate(conn)?;
    // Versions 2 to 4 were written before the schema freeze, when the file numbered its own
    // versions: they are older, not newer, and not read either.
    if version > SCHEMA_VERSION && !(2..=4).contains(&version) {
        return Err(TableFileError::Newer(version));
    }
    if version != SCHEMA_VERSION || !has_replica(conn)? {
        return Err(TableFileError::invalid(format!(
            "ktable schema version {version} is not read: this kit reads version {SCHEMA_VERSION} files, with their table document"
        )));
    }
    Ok(())
}

pub(crate) fn validate(conn: &Connection) -> Result<()> {
    let format: String = conn.query_row("SELECT value FROM kition_meta WHERE key = 'format'", [], |row| row.get(0))?;
    if format != FORMAT {
        return Err(TableFileError::invalid(format!("invalid ktable format {format:?}")));
    }
    Ok(())
}

/// Whether the file holds a KDM replica (`kition_kdm` exists and has its row).
pub(crate) fn has_replica(conn: &Connection) -> Result<bool> {
    let table: Option<String> =
        conn.query_row("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'kition_kdm'", [], |row| row.get(0)).optional()?;
    if table.is_none() {
        return Ok(false);
    }
    Ok(conn.query_row("SELECT id FROM kition_kdm WHERE id = 1", [], |row| row.get::<_, i64>(0)).optional()?.is_some())
}

pub(crate) fn schema_version(conn: &Connection) -> Result<i64> {
    let has_meta: Option<String> =
        conn.query_row("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'kition_meta'", [], |row| row.get(0)).optional()?;
    if has_meta.is_none() {
        return Ok(0);
    }
    let value: Option<String> = conn.query_row("SELECT value FROM kition_meta WHERE key = 'schema_version'", [], |row| row.get(0)).optional()?;
    Ok(value.map_or(0, |v| atoi(&v)))
}

/// Gives a new file every table (KDM tables included) at the current version.
fn create_schema(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    for (_, create, _) in TABLES {
        tx.execute_batch(&create.replacen("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ", 1))?;
    }
    for index in INDEXES {
        tx.execute_batch(index)?;
    }
    store::ensure_tables(&tx)?;
    set_meta(&tx, "format", FORMAT)?;
    set_meta(&tx, "format_version", &FORMAT_VERSION.to_string())?;
    set_meta(&tx, "schema_version", &SCHEMA_VERSION.to_string())?;
    tx.commit()?;
    Ok(())
}

/// Writes the document row and its bookkeeping; answers the KDM document id and the base the
/// replica starts with. A document id that is not a UUID keeps its string in `kition_meta` and
/// gets a new KDM document id.
fn initialize_document(conn: &mut Connection, path: &Path, options: &CreateOptions) -> Result<(DocId, BaseSpec)> {
    let created = options.created_at.unwrap_or_else(now_millis);
    let updated = options.updated_at.unwrap_or(created);
    let document_id = match options.document_id.trim() {
        "" => crate::ids::uuid().to_string(),
        id => id.to_owned(),
    };
    let title = match options.title.trim() {
        "" => path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
        title => title.to_owned(),
    };
    let meta: Value = match options.meta_json.trim() {
        "" => Value::Null,
        // The document holds JSON: anything else is none.
        raw => serde_json::from_str(raw).unwrap_or(Value::Null),
    };
    let meta_json = if meta.is_null() { "{}".to_owned() } else { meta.to_string() };
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO kition_documents (id, title, description, icon, color, meta_json, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![document_id, title, options.description.trim(), options.icon.trim(), options.color.trim(), meta_json, created, updated],
    )?;
    set_meta(&tx, "document_id", &document_id)?;
    set_meta(&tx, "created_at", &created.to_string())?;
    set_meta(&tx, "updated_at", &updated.to_string())?;
    set_meta(&tx, "schema_version", &SCHEMA_VERSION.to_string())?;
    tx.commit()?;
    let doc_id = DocId::parse(&document_id).unwrap_or_default();
    Ok((doc_id, BaseSpec { title, description: options.description.clone(), icon: options.icon.clone(), color: options.color.clone(), meta }))
}

pub(crate) fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT INTO kition_meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
    Ok(())
}

/// `None` when `key` is not set. Shared with `crate::kdm::store`, which also keys its bookkeeping
/// off `kition_meta`.
pub(crate) fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM kition_meta WHERE key = ?1", [key], |row| row.get(0)).optional()?)
}

pub(crate) fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(names)
}

pub(crate) fn atoi(value: &str) -> i64 {
    let trimmed = value.trim();
    let end = trimmed.char_indices().find(|(i, c)| !(c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))).map_or(trimmed.len(), |(i, _)| i);
    trimmed[..end].parse().unwrap_or(0)
}
