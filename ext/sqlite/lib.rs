// Copyright 2026 the Oden authors. MIT license.

use std::borrow::Cow;
use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::path::Path;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;

use deno_core::OpState;
use deno_core::Resource;
use deno_core::ResourceId;
use deno_core::ResourceTable;
use deno_core::op2;
use deno_error::JsErrorBox;
use deno_permissions::OpenAccessKind;
use deno_permissions::PermissionsContainer;
use serde::Deserialize;
use serde::Serialize;
use turso_core::Connection;
use turso_core::Database;
use turso_core::DatabaseOpts;
use turso_core::Dialect;
use turso_core::IOExt;
use turso_core::LimboError;
use turso_core::Numeric;
use turso_core::OpenFlags;
use turso_core::SqliteDialect;
use turso_core::Statement;
use turso_core::Value;
use turso_core::io::IO;

deno_core::extension!(
  oden_sqlite,
  ops = [
    op_oden_sqlite_open,
    op_oden_sqlite_close,
    op_oden_sqlite_prepare,
    op_oden_sqlite_start,
    op_oden_sqlite_next,
    op_oden_sqlite_reset,
    op_oden_sqlite_run,
    op_oden_sqlite_changes,
    op_oden_sqlite_in_transaction,
    op_oden_sqlite_expanded_sql,
    op_oden_sqlite_serialize,
    op_oden_sqlite_deserialize,
    op_oden_sqlite_release,
    op_oden_sqlite_configure,
    op_oden_sqlite_statement_info,
    op_oden_sqlite_backup,
  ],
  lazy_loaded_esm = ["oden:sqlite" = "sqlite.js"],
  lazy_loaded_js = ["node_sqlite.js"],
);

// Node's sqlite objects are GC-managed, rather than user-visible I/O resources.
// Keep their handles in a private table, while both APIs use the same engine,
// permissions, statements and operations below.
const NODE_RESOURCE: u32 = 1 << 31;
#[derive(Default)]
struct NodeResources(ResourceTable);

fn add_resource<T: Resource>(
  state: &mut OpState,
  resource: Rc<T>,
  internal: bool,
) -> ResourceId {
  if internal {
    if state.try_borrow::<NodeResources>().is_none() {
      state.put(NodeResources::default());
    }
    state.borrow_mut::<NodeResources>().0.add_rc(resource) | NODE_RESOURCE
  } else {
    state.resource_table.add_rc(resource)
  }
}

fn get_resource<T: Resource>(
  state: &OpState,
  rid: ResourceId,
) -> Result<Rc<T>, JsErrorBox> {
  if rid & NODE_RESOURCE != 0 {
    state
      .try_borrow::<NodeResources>()
      .ok_or_else(|| JsErrorBox::generic("Invalid SQLite handle"))?
      .0
      .get::<T>(rid & !NODE_RESOURCE)
      .map_err(JsErrorBox::from_err)
  } else {
    state
      .resource_table
      .get::<T>(rid)
      .map_err(JsErrorBox::from_err)
  }
}

fn take_resource<T: Resource>(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<Rc<T>, JsErrorBox> {
  if rid & NODE_RESOURCE != 0 {
    state
      .borrow_mut::<NodeResources>()
      .0
      .take::<T>(rid & !NODE_RESOURCE)
      .map_err(JsErrorBox::from_err)
  } else {
    state
      .resource_table
      .take::<T>(rid)
      .map_err(JsErrorBox::from_err)
  }
}

#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[error("{0}")]
#[property("code" = self.code())]
#[property("errno" = self.errno())]
pub struct SqliteError(#[from] LimboError);

impl SqliteError {
  fn errno(&self) -> i32 {
    match self.0 {
      LimboError::Busy | LimboError::StatementsInProgress(_) => 5,
      LimboError::BusySnapshot => 517,
      LimboError::TableLocked => 6,
      LimboError::ReadOnly => 8,
      LimboError::Interrupt => 9,
      LimboError::Corrupt(_) => 11,
      LimboError::DatabaseFull => 13,
      LimboError::Constraint(_) => 19,
      LimboError::ForeignKeyConstraint(_) => 787,
      LimboError::TooBig => 18,
      LimboError::NotADB => 26,
      _ => 1,
    }
  }

  fn code(&self) -> &'static str {
    match self.errno() {
      5 => "SQLITE_BUSY",
      517 => "SQLITE_BUSY_SNAPSHOT",
      6 => "SQLITE_LOCKED",
      8 => "SQLITE_READONLY",
      9 => "SQLITE_INTERRUPT",
      11 => "SQLITE_CORRUPT",
      13 => "SQLITE_FULL",
      18 => "SQLITE_TOOBIG",
      19 => "SQLITE_CONSTRAINT",
      787 => "SQLITE_CONSTRAINT_FOREIGNKEY",
      26 => "SQLITE_NOTADB",
      _ => "SQLITE_ERROR",
    }
  }
}

// Check every native file open, including WAL files and SQL ATTACH/VACUUM
// destinations. Keep SQL extension loading disabled on every connection.
struct PermissionIo {
  inner: Arc<dyn IO>,
  permissions: PermissionsContainer,
  main_path: String,
}

impl turso_core::io::Clock for PermissionIo {
  fn current_time_wall_clock(&self) -> turso_core::io::clock::WallClockInstant {
    self.inner.current_time_wall_clock()
  }

  fn current_time_monotonic(&self) -> turso_core::io::clock::MonotonicInstant {
    self.inner.current_time_monotonic()
  }
}

impl PermissionIo {
  fn checked_path(
    &self,
    path: &str,
    readonly: bool,
  ) -> turso_core::Result<String> {
    self
      .permissions
      .check_open(
        Cow::Borrowed(Path::new(path)),
        if readonly {
          OpenAccessKind::ReadNoFollow
        } else {
          OpenAccessKind::ReadWriteNoFollow
        },
        Some("oden:sqlite"),
      )
      .map(|p| p.into_path().to_string_lossy().into_owned())
      .map_err(|e| LimboError::InvalidArgument(e.to_string()))
  }
}

impl IO for PermissionIo {
  fn open_file(
    &self,
    path: &str,
    flags: OpenFlags,
    direct: bool,
  ) -> turso_core::Result<Arc<dyn turso_core::File>> {
    let path = self.checked_path(path, flags.contains(OpenFlags::ReadOnly))?;
    // Turso does not expose an O_NOFOLLOW open flag. Refuse existing symlink
    // files instead of letting a WAL alias escape the permission boundary.
    refuse_symlinks(Path::new(&path))?;
    // `create: false` applies to the database, not to its WAL. Existing
    // SQLite files may have no WAL yet (e.g. copied or checkpointed files).
    let flags = if path == format!("{}-wal", self.main_path)
      && !flags.contains(OpenFlags::ReadOnly)
    {
      flags | OpenFlags::Create
    } else {
      flags
    };
    self.inner.open_file(&path, flags, direct)
  }

  fn remove_file(&self, path: &str) -> turso_core::Result<()> {
    let path = self.checked_path(path, false)?;
    self.inner.remove_file(&path)
  }

  fn file_id(&self, path: &str) -> turso_core::Result<turso_core::io::FileId> {
    let path = self.checked_path(path, true)?;
    self.inner.file_id(&path)
  }

  fn step(&self) -> turso_core::Result<()> {
    self.inner.step()
  }
}

fn refuse_symlinks(path: &Path) -> turso_core::Result<()> {
  for component in path.ancestors() {
    if std::fs::symlink_metadata(component)
      .is_ok_and(|m| m.file_type().is_symlink())
    {
      return Err(LimboError::InvalidArgument(
        "unable to open database file: symbolic links are not supported".into(),
      ));
    }
  }
  Ok(())
}

struct DatabaseResource {
  connection: Arc<Connection>,
  statements: RefCell<Vec<(ResourceId, Weak<StatementResource>)>>,
  readonly: bool,
}

impl DatabaseResource {
  fn close_connection(&self) -> Result<(), SqliteError> {
    for statement in self
      .statements
      .borrow_mut()
      .drain(..)
      .filter_map(|(_, s)| s.upgrade())
    {
      statement.statement.borrow_mut().take();
    }
    self.connection.close()?;
    Ok(())
  }
}

impl Resource for DatabaseResource {
  fn name(&self) -> Cow<'_, str> {
    "odenSqliteDatabase".into()
  }
  fn close(self: Rc<Self>) {
    let _ = self.close_connection();
  }
}

struct StatementResource {
  database: Rc<DatabaseResource>,
  statement: RefCell<Option<Statement>>,
  owned: bool,
}

impl Resource for StatementResource {
  fn name(&self) -> Cow<'_, str> {
    "odenSqliteStatement".into()
  }
  fn close(self: Rc<Self>) {
    self.statement.borrow_mut().take();
  }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum SqlValue {
  Null,
  Integer(String),
  Real(f64),
  Text(String),
  Blob(Vec<u8>),
}

impl SqlValue {
  fn into_value(self) -> Result<Value, JsErrorBox> {
    Ok(match self {
      Self::Null => Value::Null,
      Self::Integer(v) => Value::from_i64(v.parse().map_err(|_| {
        JsErrorBox::range_error("SQLite integers must fit in signed 64 bits")
      })?),
      Self::Real(v) => Value::from_f64(v),
      Self::Text(v) => Value::build_text(v),
      Self::Blob(v) => Value::from_blob(v),
    })
  }
}

fn row_values(statement: &Statement) -> Result<Vec<SqlValue>, SqliteError> {
  let row = statement.row().expect("step returned a row");
  (0..statement.num_columns())
    .map(|i| {
      Ok(match row.get_value(i) {
        Value::Null => SqlValue::Null,
        Value::Numeric(Numeric::Integer(v)) => SqlValue::Integer(v.to_string()),
        Value::Numeric(Numeric::Float(v)) => SqlValue::Real((*v).into()),
        Value::Text(v) => SqlValue::Text(v.as_str().into()),
        Value::Blob(v) => SqlValue::Blob(v.to_vec()),
      })
    })
    .collect()
}

fn with_statement<T>(
  state: &OpState,
  rid: ResourceId,
  f: impl FnOnce(&mut Statement) -> Result<T, JsErrorBox>,
) -> Result<T, JsErrorBox> {
  let resource = get_resource::<StatementResource>(state, rid)?;
  if resource.database.connection.is_closed() {
    return Err(JsErrorBox::generic("Database is closed"));
  }
  let mut guard = resource.statement.borrow_mut();
  let statement = guard
    .as_mut()
    .ok_or_else(|| JsErrorBox::generic("Statement has been finalized"))?;
  check_statement(state, &resource.database, statement)?;
  f(statement)
}

fn check_statement(
  state: &OpState,
  database: &DatabaseResource,
  statement: &Statement,
) -> Result<(), JsErrorBox> {
  // Turso's process-wide database registry can share a writable database
  // between connections. Enforce each handle's readonly option separately.
  if database.readonly && !statement.get_program().is_readonly() {
    return Err(sql_error(LimboError::ReadOnly));
  }
  if state
    .borrow::<PermissionsContainer>()
    .check_has_all_permissions(Path::new(database.connection.db_file_path()))
    .is_err()
  {
    let (command, _) = SqliteDialect
      .parse(statement.get_sql())
      .map_err(sql_error)?;
    if matches!(
      command,
      Some(turso_parser::ast::Cmd::Stmt(
        turso_parser::ast::Stmt::Attach { .. }
      ))
    ) {
      return Err(JsErrorBox::generic(
        "ATTACH DATABASE requires --allow-all with the Turso backend",
      ));
    }
  }
  Ok(())
}

fn sql_error(e: LimboError) -> JsErrorBox {
  JsErrorBox::from_err(SqliteError(e))
}

#[op2]
pub fn op_oden_sqlite_open(
  state: &mut OpState,
  #[string] filename: String,
  readonly: bool,
  create: bool,
  internal: bool,
) -> Result<ResourceId, JsErrorBox> {
  let (path, io): (String, Arc<dyn IO>) = if filename == ":memory:" {
    (filename, Arc::new(turso_core::MemoryIO::new()))
  } else {
    let permissions = state.borrow::<PermissionsContainer>().clone();
    let path = permissions
      .check_open(
        Cow::Borrowed(Path::new(&filename)),
        if readonly {
          OpenAccessKind::ReadNoFollow
        } else {
          OpenAccessKind::ReadWriteNoFollow
        },
        Some("oden:sqlite"),
      )
      .map_err(JsErrorBox::from_err)?
      .into_path()
      .to_string_lossy()
      .into_owned();
    let io = Arc::new(PermissionIo {
      inner: Database::io_for_path(&path).map_err(sql_error)?,
      permissions,
      main_path: path.clone(),
    });
    refuse_symlinks(Path::new(&path)).map_err(sql_error)?;
    (path, io)
  };
  let flags = if readonly {
    OpenFlags::ReadOnly
  } else if create {
    OpenFlags::Create
  } else {
    OpenFlags::None
  };
  let allow_attach = state
    .borrow::<PermissionsContainer>()
    .check_has_all_permissions(Path::new(&path))
    .is_ok();
  let db = Database::open_file_with_flags(
    io,
    &path,
    flags,
    DatabaseOpts::new().with_attach(allow_attach),
    None,
    Arc::new(SqliteDialect),
  )
  .map_err(sql_error)?;
  let connection = db.connect().map_err(sql_error)?;
  connection.set_load_extension_enabled(false);
  Ok(add_resource(
    state,
    Rc::new(DatabaseResource {
      connection,
      statements: RefCell::new(Vec::new()),
      readonly,
    }),
    internal,
  ))
}

#[op2]
pub fn op_oden_sqlite_release(state: &mut OpState, rid: ResourceId) {
  if get_resource::<DatabaseResource>(state, rid).is_ok() {
    let _ = close_database(state, rid, false);
  } else if let Ok(resource) = take_resource::<StatementResource>(state, rid) {
    resource.close();
  }
}

#[op2]
pub fn op_oden_sqlite_configure(
  state: &mut OpState,
  rid: ResourceId,
  foreign_keys: bool,
  double_quoted_strings: bool,
  timeout: u32,
) -> Result<(), JsErrorBox> {
  let resource = get_resource::<DatabaseResource>(state, rid)?;
  resource.connection.set_foreign_keys_enabled(foreign_keys);
  resource.connection.set_dqs_dml(double_quoted_strings);
  resource
    .connection
    .set_busy_timeout(std::time::Duration::from_millis(timeout.into()));
  Ok(())
}

#[derive(Serialize)]
pub struct ColumnInfo {
  name: String,
  column: Option<String>,
  table: Option<String>,
  database: Option<String>,
  #[serde(rename = "type")]
  declared_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeStatementInfo {
  source_sql: String,
  parameters: Vec<Option<String>>,
  columns: Vec<ColumnInfo>,
}

#[op2]
#[serde]
pub fn op_oden_sqlite_statement_info(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<NodeStatementInfo, JsErrorBox> {
  with_statement(state, rid, |statement| {
    let program = statement.get_program();
    let columns = (0..statement.num_columns())
      .map(|i| {
        let mut info = ColumnInfo {
          name: statement.get_column_name(i).into_owned(),
          column: None,
          table: None,
          database: None,
          declared_type: statement.get_column_decltype(i),
        };
        if let Some(result) = program.result_columns.get(i) {
          let origin = match &result.expr {
            turso_parser::ast::Expr::Column { table, column, .. } => program
              .table_references
              .find_table_by_internal_id(*table)
              .map(|(_, table_ref)| {
                (
                  program
                    .table_references
                    .find_joined_table_by_internal_id(*table)
                    .map(|t| t.database_id),
                  table_ref,
                  table_ref
                    .get_column_at(*column)
                    .and_then(|c| c.name.clone()),
                )
              }),
            turso_parser::ast::Expr::RowId { table, .. } => program
              .table_references
              .find_table_by_internal_id(*table)
              .map(|(_, table_ref)| {
                let name =
                  if let turso_core::schema::Table::BTree(t) = table_ref {
                    t.get_rowid_alias_column().and_then(|(_, c)| {
                      info.declared_type =
                        (!c.ty_str.is_empty()).then(|| c.ty_str.clone());
                      c.name.clone()
                    })
                  } else {
                    None
                  };
                (
                  program
                    .table_references
                    .find_joined_table_by_internal_id(*table)
                    .map(|t| t.database_id),
                  table_ref,
                  name,
                )
              }),
            _ => None,
          };
          if let Some((db, table, column)) = origin {
            info.table = Some(table.get_name().into());
            info.column = column;
            info.database = match db {
              Some(0) => Some("main".into()),
              Some(1) => Some("temp".into()),
              _ => None,
            };
          }
        }
        info
      })
      .collect();
    Ok(NodeStatementInfo {
      source_sql: statement.get_sql().into(),
      parameters: (1..=statement.parameters_count())
        .map(|i| statement.parameters().name(NonZeroUsize::new(i).unwrap()))
        .collect(),
      columns,
    })
  })
}

#[op2]
pub fn op_oden_sqlite_backup(
  state: &mut OpState,
  rid: ResourceId,
  #[string] destination: String,
  rate: u32,
) -> Result<u32, JsErrorBox> {
  if rate == 0 {
    return Err(JsErrorBox::range_error("backup rate must be positive"));
  }
  if !get_resource::<DatabaseResource>(state, rid)?
    .connection
    .get_auto_commit()
  {
    return Err(sql_error(LimboError::Busy));
  }
  let path = state
    .borrow::<PermissionsContainer>()
    .check_open(
      Cow::Borrowed(Path::new(&destination)),
      OpenAccessKind::WriteNoFollow,
      Some("node:sqlite.backup"),
    )
    .map_err(JsErrorBox::from_err)?
    .into_path();
  refuse_symlinks(&path).map_err(sql_error)?;
  let bytes = serialize_database(state, rid)?;
  // Only create a fresh destination: replacing a live SQLite file could leave
  // its WAL inconsistent. Existing-destination backup is explicitly unsupported.
  let mut file = std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(&path)
    .map_err(JsErrorBox::from_err)?;
  use std::io::Write;
  let page_size = if bytes.len() >= 18 {
    let size = u16::from_be_bytes(bytes[16..18].try_into().unwrap());
    if size == 1 {
      65536
    } else {
      usize::from(size).max(512)
    }
  } else {
    4096
  };
  let result = (|| {
    for chunk in bytes.chunks((rate as usize).saturating_mul(page_size)) {
      file.write_all(chunk)?;
    }
    file.sync_all()
  })();
  if let Err(error) = result {
    drop(file);
    let _ = std::fs::remove_file(&path);
    return Err(JsErrorBox::from_err(error));
  }
  Ok(if bytes.len() >= 32 {
    u32::from_be_bytes(bytes[28..32].try_into().unwrap())
  } else {
    0
  })
}

#[op2]
pub fn op_oden_sqlite_close(
  state: &mut OpState,
  rid: ResourceId,
  throw_on_error: bool,
) -> Result<(), JsErrorBox> {
  close_database(state, rid, throw_on_error)
}

fn close_database(
  state: &mut OpState,
  rid: ResourceId,
  throw_on_error: bool,
) -> Result<(), JsErrorBox> {
  let database = get_resource::<DatabaseResource>(state, rid)?;
  if throw_on_error
    && database.statements.borrow().iter().any(|(_, weak)| {
      weak
        .upgrade()
        .is_some_and(|s| !s.owned && s.statement.borrow().is_some())
    })
  {
    return Err(sql_error(LimboError::Busy));
  }
  let statements: Vec<_> = database
    .statements
    .borrow()
    .iter()
    .map(|(rid, _)| *rid)
    .collect();
  database.close_connection().map_err(JsErrorBox::from_err)?;
  for rid in statements {
    if let Ok(resource) = take_resource::<StatementResource>(state, rid) {
      resource.close();
    }
  }
  take_resource::<DatabaseResource>(state, rid)?;
  Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementInfo {
  rid: ResourceId,
  column_names: Vec<String>,
  declared_types: Vec<Option<String>>,
  parameters: Vec<Option<String>>,
  readonly: bool,
}

#[op2]
#[serde]
pub fn op_oden_sqlite_prepare(
  state: &mut OpState,
  rid: ResourceId,
  #[string] sql: String,
  owned: bool,
) -> Result<StatementInfo, JsErrorBox> {
  let database = get_resource::<DatabaseResource>(state, rid)?;
  let statement = database.connection.prepare(&sql).map_err(sql_error)?;
  check_statement(state, &database, &statement)?;
  let readonly = statement.get_program().is_readonly();
  let column_names = (0..statement.num_columns())
    .map(|i| statement.get_column_name(i).into_owned())
    .collect();
  let declared_types = (0..statement.num_columns())
    .map(|i| statement.get_column_decltype(i).map(|s| s.to_string()))
    .collect();
  let parameters = (1..=statement.parameters_count())
    .map(|i| statement.parameters().name(NonZeroUsize::new(i).unwrap()))
    .collect();
  let resource = Rc::new(StatementResource {
    database: database.clone(),
    statement: RefCell::new(Some(statement)),
    owned,
  });
  let rid = add_resource(state, resource.clone(), rid & NODE_RESOURCE != 0);
  let mut statements = database.statements.borrow_mut();
  statements.retain(|(_, s)| s.strong_count() > 0);
  statements.push((rid, Rc::downgrade(&resource)));
  Ok(StatementInfo {
    rid,
    column_names,
    declared_types,
    parameters,
    readonly,
  })
}

#[op2]
pub fn op_oden_sqlite_start(
  state: &mut OpState,
  rid: ResourceId,
  #[serde] values: Vec<SqlValue>,
) -> Result<(), JsErrorBox> {
  // Convert all parameters before modifying bindings, so conversion failure
  // cannot leave a partially bound prepared statement behind.
  let values: Vec<Value> = values
    .into_iter()
    .map(SqlValue::into_value)
    .collect::<Result<_, _>>()?;
  with_statement(state, rid, |statement| {
    if values.len() != statement.parameters_count() {
      return Err(JsErrorBox::type_error(
        "Incorrect number of SQLite bindings",
      ));
    }
    statement.reset().map_err(sql_error)?;
    for (i, value) in values.into_iter().enumerate() {
      statement
        .bind_at(NonZeroUsize::new(i + 1).unwrap(), value)
        .map_err(sql_error)?;
    }
    Ok(())
  })
}

#[op2]
#[serde]
pub fn op_oden_sqlite_next(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<Option<Vec<SqlValue>>, JsErrorBox> {
  with_statement(state, rid, |statement| {
    let has_row = statement
      .run_one_step_blocking(|| Ok(()), || Ok(()))
      .map_err(sql_error)?
      .is_some();
    if has_row {
      row_values(statement)
        .map(Some)
        .map_err(JsErrorBox::from_err)
    } else {
      Ok(None)
    }
  })
}

#[op2]
pub fn op_oden_sqlite_reset(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<(), JsErrorBox> {
  with_statement(state, rid, |statement| statement.reset().map_err(sql_error))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
  changes: i64,
  last_insert_rowid: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum RunBindings {
  Positional(Vec<SqlValue>),
  Named(Vec<(String, SqlValue)>),
}

impl RunBindings {
  fn bind(
    &self,
    statement: &mut Statement,
    strict: bool,
  ) -> Result<(), JsErrorBox> {
    let count = statement.parameters_count();
    if count == 0 {
      return Ok(());
    }
    match self {
      Self::Positional(values) => {
        if values.len() != count {
          return Err(JsErrorBox::type_error(
            "Incorrect number of SQLite bindings",
          ));
        }
        for (i, value) in values.iter().enumerate() {
          statement
            .bind_at(
              NonZeroUsize::new(i + 1).unwrap(),
              value.clone().into_value()?,
            )
            .map_err(sql_error)?;
        }
      }
      Self::Named(values) => {
        for i in 1..=count {
          let index = NonZeroUsize::new(i).unwrap();
          let name = statement.parameters().name(index);
          let value = name.as_ref().and_then(|name| {
            values.iter().find(|(key, _)| key == name).or_else(|| {
              if strict {
                values.iter().find(|(key, _)| key == &name[1..])
              } else {
                None
              }
            })
          });
          if value.is_none() && strict {
            return Err(JsErrorBox::type_error(format!(
              "Missing SQLite binding {}",
              name.unwrap_or_else(|| i.to_string())
            )));
          }
          statement
            .bind_at(
              index,
              value
                .map(|(_, v)| v.clone())
                .unwrap_or(SqlValue::Null)
                .into_value()?,
            )
            .map_err(sql_error)?;
        }
      }
    }
    Ok(())
  }
}

fn changes(connection: &Connection) -> Changes {
  Changes {
    changes: connection.changes(),
    last_insert_rowid: connection.last_insert_rowid().to_string(),
  }
}

#[op2]
#[serde]
pub fn op_oden_sqlite_changes(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<Changes, JsErrorBox> {
  let database = get_resource::<DatabaseResource>(state, rid)?;
  Ok(changes(&database.connection))
}

#[op2]
#[serde]
pub fn op_oden_sqlite_run(
  state: &mut OpState,
  rid: ResourceId,
  #[string] sql: String,
  #[serde] bindings: Option<RunBindings>,
  strict: bool,
) -> Result<Changes, JsErrorBox> {
  let database = get_resource::<DatabaseResource>(state, rid)?;
  for statement in database.connection.query_runner(sql.as_bytes()) {
    if let Some(mut statement) = statement.map_err(sql_error)? {
      check_statement(state, &database, &statement)?;
      if let Some(bindings) = &bindings {
        bindings.bind(&mut statement, strict)?;
      }
      statement.run_ignore_rows().map_err(sql_error)?;
    }
  }
  Ok(changes(&database.connection))
}

#[op2]
pub fn op_oden_sqlite_in_transaction(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<bool, JsErrorBox> {
  let database = get_resource::<DatabaseResource>(state, rid)?;
  Ok(!database.connection.get_auto_commit())
}

#[op2]
#[string]
pub fn op_oden_sqlite_expanded_sql(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<String, JsErrorBox> {
  with_statement(state, rid, |statement| Ok(statement.expanded_sql()))
}

#[op2]
#[buffer]
pub fn op_oden_sqlite_serialize(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<Vec<u8>, JsErrorBox> {
  serialize_database(state, rid)
}

fn serialize_database(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<Vec<u8>, JsErrorBox> {
  let database = get_resource::<DatabaseResource>(state, rid)?;
  let connection = &database.connection;
  // Read through the pager so WAL and uncommitted pages are included. An
  // explicit read transaction pins a consistent snapshot across all pages.
  let own_transaction = connection.get_auto_commit();
  if own_transaction {
    connection.execute("BEGIN").map_err(sql_error)?;
  }
  let result = (|| {
    connection
      .prepare("SELECT * FROM sqlite_schema")?
      .run_ignore_rows()?;
    let pager = connection.get_pager();
    let count = pager
      .io
      .block(|| pager.with_header(|h| h.database_size.get()))?;
    let page_size = connection.get_page_size().get() as usize;
    let mut bytes = Vec::new();
    bytes
      .try_reserve_exact(count as usize * page_size)
      .map_err(|_| LimboError::TooBig)?;
    for index in 1..=count {
      let (page, completion) =
        pager.io.block(|| pager.read_page(index.into()))?;
      if let Some(completion) = completion {
        pager.io.wait_for_completion(completion)?;
      }
      let content = page.get_contents();
      let buffer = content.buffer().ok_or_else(|| {
        LimboError::InternalError("Page has no buffer".into())
      })?;
      bytes.extend_from_slice(buffer.as_slice());
    }
    Ok::<_, LimboError>(bytes)
  })();
  if own_transaction {
    let end = connection.execute("ROLLBACK");
    if result.is_ok() {
      end.map_err(sql_error)?;
    }
  }
  result.map_err(sql_error)
}

#[op2]
pub fn op_oden_sqlite_deserialize(
  state: &mut OpState,
  #[buffer] bytes: &[u8],
  readonly: bool,
  internal: bool,
) -> Result<ResourceId, JsErrorBox> {
  // SQLite serializes a never-written database as an empty image.
  if !bytes.is_empty()
    && (bytes.len() < 100 || &bytes[..16] != b"SQLite format 3\0")
  {
    return Err(sql_error(LimboError::NotADB));
  }
  let io: Arc<dyn IO> = Arc::new(turso_core::MemoryIO::new());
  let file = io
    .open_file(":memory:", OpenFlags::Create, false)
    .map_err(sql_error)?;
  let completion = file
    .pwrite(
      0,
      Arc::new(turso_core::Buffer::new(bytes.to_vec())),
      turso_core::Completion::new_write(|_| {}),
    )
    .map_err(sql_error)?;
  io.wait_for_completion(completion).map_err(sql_error)?;
  let db = Database::open_file_with_flags(
    io,
    ":memory:",
    if readonly && !bytes.is_empty() {
      OpenFlags::ReadOnly
    } else {
      OpenFlags::Create
    },
    DatabaseOpts::new(),
    None,
    Arc::new(SqliteDialect),
  )
  .map_err(sql_error)?;
  let connection = db.connect().map_err(sql_error)?;
  connection.set_load_extension_enabled(false);
  Ok(add_resource(
    state,
    Rc::new(DatabaseResource {
      connection,
      statements: RefCell::new(Vec::new()),
      readonly,
    }),
    internal,
  ))
}
