// Copyright 2018-2026 the Deno authors. MIT license.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs::File;
#[cfg(test)]
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use async_trait::async_trait;
use bincode::Options;
use chrono::Utc;
use deno_core::OpState;
use deno_core::futures::stream;
use deno_error::JsErrorBox;
use deno_permissions::OpenAccessKind;
use deno_permissions::PermissionsContainer;
use denokv_proto::*;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

#[path = "native_sum.rs"]
mod sum_operand;
use sum_operand::SumOperand;

const MAGIC: &[u8; 8] = b"DENOKV01";
const DELTA_FRAME: u64 = 1 << 63;
const MAX_FRAME: u64 = 256 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(20);

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
  bytes: Vec<u8>,
  encoding: i64,
  version: Versionstamp,
  expires: Option<i64>,
}
impl Entry {
  fn new(value: KvValue, version: Versionstamp, expires: Option<i64>) -> Self {
    let (bytes, encoding) = encode_value_owned(value);
    Self {
      bytes,
      encoding,
      version,
      expires,
    }
  }
  fn value(&self) -> KvValue {
    decode_value(self.bytes.clone(), self.encoding).unwrap()
  }
  fn output(&self, key: Vec<u8>) -> KvEntry {
    KvEntry {
      key,
      value: self.value(),
      versionstamp: self.version,
    }
  }
}

#[derive(Clone, Serialize, Deserialize)]
struct Message {
  payload: Vec<u8>,
  due: i64,
  keys: Vec<Vec<u8>>,
  backoff: Vec<u32>,
  attempt: usize,
  lease: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct State {
  version: u64,
  next_message: u64,
  entries: BTreeMap<Vec<u8>, Entry>,
  messages: BTreeMap<u64, Message>,
  #[serde(skip)]
  expirations: BTreeSet<(i64, Vec<u8>)>,
  #[serde(skip)]
  undo_entries: BTreeMap<Vec<u8>, Option<Entry>>,
  #[serde(skip)]
  undo_messages: BTreeMap<u64, Option<Message>>,
}
impl State {
  fn stamp(&mut self) -> Result<Versionstamp, JsErrorBox> {
    self.version = self
      .version
      .checked_add(1)
      .ok_or_else(|| error("Version exhausted"))?;
    let mut stamp = [0; 10];
    stamp[..8].copy_from_slice(&self.version.to_be_bytes());
    Ok(stamp)
  }
  fn rebuild_expirations(&mut self) {
    self.expirations = self
      .entries
      .iter()
      .filter_map(|(k, e)| e.expires.map(|t| (t, k.clone())))
      .collect();
  }
  fn expire(&mut self) {
    let now = Utc::now().timestamp_millis();
    while self.expirations.first().is_some_and(|(t, _)| *t <= now) {
      let (_, key) = self.expirations.pop_first().unwrap();
      self.entries.remove(&key);
    }
  }
  fn replace_entry(&mut self, key: Vec<u8>, entry: Option<Entry>) {
    if let Some(old) = self.entries.remove(&key)
      && let Some(t) = old.expires
    {
      self.expirations.remove(&(t, key.clone()));
    }
    if let Some(entry) = entry {
      if let Some(t) = entry.expires {
        self.expirations.insert((t, key.clone()));
      }
      self.entries.insert(key, entry);
    }
  }
  fn set_entry(&mut self, key: Vec<u8>, entry: Option<Entry>) {
    self
      .undo_entries
      .entry(key.clone())
      .or_insert_with(|| self.entries.get(&key).cloned());
    self.replace_entry(key, entry);
  }
  fn message_mut(&mut self, id: u64) -> Option<&mut Message> {
    self
      .undo_messages
      .entry(id)
      .or_insert_with(|| self.messages.get(&id).cloned());
    self.messages.get_mut(&id)
  }
  fn set_message(&mut self, id: u64, message: Option<Message>) {
    self
      .undo_messages
      .entry(id)
      .or_insert_with(|| self.messages.get(&id).cloned());
    if let Some(message) = message {
      self.messages.insert(id, message);
    } else {
      self.messages.remove(&id);
    }
  }
  fn clear_undo(&mut self) {
    self.undo_entries.clear();
    self.undo_messages.clear();
  }
  fn rollback(&mut self, version: u64, next_message: u64) {
    for (key, entry) in std::mem::take(&mut self.undo_entries) {
      self.replace_entry(key, entry);
    }
    for (id, message) in std::mem::take(&mut self.undo_messages) {
      if let Some(message) = message {
        self.messages.insert(id, message);
      } else {
        self.messages.remove(&id);
      }
    }
    self.version = version;
    self.next_message = next_message;
  }
}
fn error(message: impl Into<String>) -> JsErrorBox {
  JsErrorBox::type_error(message.into())
}
fn io_error(e: impl std::fmt::Display) -> JsErrorBox {
  JsErrorBox::generic(e.to_string())
}

// Snapshot frames from the initial implementation remain readable. New frames
// carry only touched keys/messages; the high length bit identifies their format.
#[derive(Serialize, Deserialize)]
struct Delta {
  version: u64,
  next_message: u64,
  entries: BTreeMap<Vec<u8>, Option<Entry>>,
  messages: BTreeMap<u64, Option<Message>>,
}
fn validate_entry(e: &Entry) -> Result<(), JsErrorBox> {
  if !matches!(
    e.encoding,
    VALUE_ENCODING_V8 | VALUE_ENCODING_BYTES | VALUE_ENCODING_LE64
  ) || (e.encoding == VALUE_ENCODING_LE64 && e.bytes.len() != 8)
  {
    return Err(error("Invalid native KV value encoding"));
  }
  Ok(())
}
#[derive(Default)]
struct Store {
  state: State,
  file: Option<File>,
  // End of the last validated frame. Every operation holds the OS lock before
  // checking for another writer's appended data. Unchanged files need no replay.
  valid: u64,
  modified: Option<std::time::SystemTime>,
}
impl Store {
  #[cfg(test)]
  fn load(file: &mut File) -> Result<(State, u64), JsErrorBox> {
    let mut state = State::default();
    let mut valid = 0;
    Self::replay(file, &mut state, &mut valid)?;
    Ok((state, valid))
  }
  fn replay(
    file: &mut File,
    state: &mut State,
    valid: &mut u64,
  ) -> Result<(), JsErrorBox> {
    let size = file.metadata().map_err(io_error)?.len();
    file.seek(SeekFrom::Start(*valid)).map_err(io_error)?;
    if *valid == 0 {
      if size == 0 {
        return Ok(());
      }
      if size < MAGIC.len() as u64 {
        let mut prefix = vec![0; size as usize];
        file.read_exact(&mut prefix).map_err(io_error)?;
        if prefix != MAGIC[..size as usize] {
          return Err(error("Not a native KV database"));
        }
        // The first commit may have stopped while writing the magic header.
        // No complete transaction exists, so a subsequent writer can reset it.
        return Ok(());
      }
      let mut magic = [0; 8];
      file.read_exact(&mut magic).map_err(io_error)?;
      if &magic != MAGIC {
        return Err(error(
          "Not a native KV database (SQLite files require migration)",
        ));
      }
      *valid = 8;
    }
    while *valid < size {
      if size - *valid < 40 {
        break;
      }
      let mut len = [0; 8];
      file.read_exact(&mut len).map_err(io_error)?;
      let tagged_len = u64::from_le_bytes(len);
      let delta = tagged_len & DELTA_FRAME != 0;
      let len = tagged_len & !DELTA_FRAME;
      if len > MAX_FRAME {
        return Err(error("Native KV frame exceeds size limit"));
      }
      let mut checksum = [0; 32];
      file.read_exact(&mut checksum).map_err(io_error)?;
      if size - *valid - 40 < len {
        break;
      }
      let mut bytes = vec![0; len as usize];
      file.read_exact(&mut bytes).map_err(io_error)?;
      if Sha256::digest(&bytes)[..] != checksum {
        return Err(error("Native KV checksum mismatch"));
      }
      let codec = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_FRAME)
        .reject_trailing_bytes();
      if delta {
        let delta: Delta = codec.deserialize(&bytes).map_err(io_error)?;
        for entry in delta.entries.values().flatten() {
          validate_entry(entry)?;
        }
        for (key, entry) in delta.entries {
          state.replace_entry(key, entry);
        }
        for (id, message) in delta.messages {
          if let Some(message) = message {
            state.messages.insert(id, message);
          } else {
            state.messages.remove(&id);
          }
        }
        state.version = delta.version;
        state.next_message = delta.next_message;
      } else {
        let mut snapshot: State =
          codec.deserialize(&bytes).map_err(io_error)?;
        for entry in snapshot.entries.values() {
          validate_entry(entry)?;
        }
        snapshot.rebuild_expirations();
        *state = snapshot;
      }
      *valid += 40 + len;
    }
    Ok(())
  }
  fn transaction<T>(
    &mut self,
    change: bool,
    f: impl FnOnce(&mut State) -> Result<T, JsErrorBox>,
  ) -> Result<T, JsErrorBox> {
    // Take the descriptor out temporarily to allow independent mutable state
    // access. Restore it even on failure so a failed operation cannot turn a
    // persistent database into an in-memory one.
    let mut file = self.file.take();
    let result = if let Some(file) = &mut file {
      let locked = file.lock().map_err(io_error);
      match locked {
        Err(e) => Err(e),
        Ok(()) => {
          let result = self.disk_transaction(file, change, f);
          let unlocked = file.unlock().map_err(io_error);
          result.and_then(|output| unlocked.map(|()| output))
        }
      }
    } else {
      self.state.expire();
      let version = self.state.version;
      let next_message = self.state.next_message;
      let result = f(&mut self.state);
      if result.is_err() {
        self.state.rollback(version, next_message);
      } else {
        self.state.clear_undo();
      }
      result
    };
    self.file = file;
    result
  }
  fn disk_transaction<T>(
    &mut self,
    file: &mut File,
    change: bool,
    f: impl FnOnce(&mut State) -> Result<T, JsErrorBox>,
  ) -> Result<T, JsErrorBox> {
    let metadata = file.metadata().map_err(io_error)?;
    let modified = metadata.modified().ok();
    if metadata.len() < self.valid
      || (metadata.len() == self.valid && modified != self.modified)
    {
      self.valid = 0;
      self.state = State::default();
    }
    if metadata.len() != self.valid {
      Self::replay(file, &mut self.state, &mut self.valid)?;
    }
    self.modified = modified;
    self.state.expire();
    let version = self.state.version;
    let next_message = self.state.next_message;
    let result = (|| {
      let output = f(&mut self.state)?;
      if change
        && (self.state.version != version
          || self.state.next_message != next_message
          || !self.state.undo_entries.is_empty()
          || !self.state.undo_messages.is_empty())
      {
        // Serialize references rather than copying all values again.
        let entries: BTreeMap<_, _> = self
          .state
          .undo_entries
          .keys()
          .map(|k| (k, self.state.entries.get(k)))
          .collect();
        let messages: BTreeMap<_, _> = self
          .state
          .undo_messages
          .keys()
          .map(|id| (id, self.state.messages.get(id)))
          .collect();
        // A tuple has the same bincode layout as Delta's four fields.
        let bytes = bincode::serialize(&(
          self.state.version,
          self.state.next_message,
          entries,
          messages,
        ))
        .map_err(io_error)?;
        if bytes.len() as u64 > MAX_FRAME {
          return Err(error("Native KV transaction exceeds 256 MiB"));
        }
        let start = self.valid;
        let append: Result<(), JsErrorBox> = (|| {
          if metadata.len() != start {
            file.set_len(start).map_err(io_error)?;
          }
          file.seek(SeekFrom::Start(start)).map_err(io_error)?;
          let mut frame = Vec::with_capacity(48 + bytes.len());
          if start == 0 {
            frame.extend_from_slice(MAGIC);
          }
          frame.extend_from_slice(
            &(bytes.len() as u64 | DELTA_FRAME).to_le_bytes(),
          );
          frame.extend_from_slice(&Sha256::digest(&bytes));
          frame.extend_from_slice(&bytes);
          file.write_all(&frame).map_err(io_error)?;
          file.sync_all().map_err(io_error)?;
          self.valid = start + frame.len() as u64;
          self.modified = file.metadata().map_err(io_error)?.modified().ok();
          Ok(())
        })();
        if append.is_err() {
          // A partial/unsynced commit must not become visible to the next
          // transaction. If rollback I/O fails, invalidate the cache for replay.
          if file.set_len(start).and_then(|()| file.sync_all()).is_err() {
            self.valid = 0;
          }
          self.modified = file.metadata().ok().and_then(|m| m.modified().ok());
          append?;
        }
      }
      Ok(output)
    })();
    if result.is_err() {
      self.state.rollback(version, next_message);
    } else {
      self.state.clear_undo();
    }
    result
  }
}

// Resolve each directory through a held descriptor, so replacing a parent
// with a symlink between validation and open cannot redirect the database.
#[cfg(unix)]
fn open_file(path: &std::path::Path) -> Result<File, JsErrorBox> {
  use std::ffi::CString;
  use std::os::fd::AsRawFd;
  use std::os::fd::FromRawFd;
  use std::os::unix::ffi::OsStrExt;
  use std::path::Component;
  let mut dir =
    File::open(if path.is_absolute() { "/" } else { "." }).map_err(io_error)?;
  let parts: Vec<_> = path
    .components()
    .filter(|c| !matches!(c, Component::RootDir | Component::CurDir))
    .collect();
  if parts.is_empty() {
    return Err(error("Invalid KV filename"));
  }
  for (i, part) in parts.iter().enumerate() {
    let name = CString::new(part.as_os_str().as_bytes()).map_err(io_error)?;
    let last = i + 1 == parts.len();
    let flags = libc::O_CLOEXEC
      | libc::O_NOFOLLOW
      | if last {
        libc::O_RDWR | libc::O_CREAT
      } else {
        libc::O_RDONLY | libc::O_DIRECTORY
      };
    // SAFETY: dir is a live descriptor, name is a NUL-terminated string, and
    // the mode argument is supplied for O_CREAT. No borrowed pointer escapes.
    let fd = unsafe {
      libc::openat(dir.as_raw_fd(), name.as_ptr(), flags, 0o600 as libc::mode_t)
    };
    if fd < 0 {
      return Err(io_error(std::io::Error::last_os_error()));
    }
    // SAFETY: openat returned a new descriptor whose ownership is transferred
    // exactly once to File, which closes it on drop.
    let file = unsafe { File::from_raw_fd(fd) };
    if last {
      if !file.metadata().map_err(io_error)?.is_file() {
        return Err(error("KV database must be a regular file"));
      }
      dir.sync_all().map_err(io_error)?;
      return Ok(file);
    }
    dir = file;
  }
  unreachable!()
}

#[cfg(windows)]
#[path = "native_windows.rs"]
mod windows_file;

#[cfg(windows)]
fn open_file(path: &std::path::Path) -> Result<File, JsErrorBox> {
  windows_file::open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_file(_path: &std::path::Path) -> Result<File, JsErrorBox> {
  Err(error(
    "Native KV file storage is unsupported on this platform",
  ))
}

pub struct NativeDbHandler {
  pub default_storage_dir: Option<PathBuf>,
}
impl NativeDbHandler {
  pub fn new(
    default_storage_dir: Option<PathBuf>,
    _versionstamp_rng_seed: Option<u64>,
  ) -> Self {
    Self {
      default_storage_dir,
    }
  }
}
#[derive(Clone)]
pub struct NativeDb {
  store: Arc<Mutex<Store>>,
  closed: Arc<AtomicBool>,
}
impl NativeDb {
  fn transact<T>(
    &self,
    change: bool,
    f: impl FnOnce(&mut State) -> Result<T, JsErrorBox>,
  ) -> Result<T, JsErrorBox> {
    if self.closed.load(Ordering::Acquire) {
      return Err(error("Database closed"));
    }
    self
      .store
      .lock()
      .map_err(|_| error("KV store lock poisoned"))?
      .transaction(change, f)
  }
  async fn transact_async<T: Send + 'static>(
    &self,
    change: bool,
    f: impl FnOnce(&mut State) -> Result<T, JsErrorBox> + Send + 'static,
  ) -> Result<T, JsErrorBox> {
    // In-memory operations perform no I/O and need no worker round trip. If
    // another operation holds the mutex, fall back to a blocking worker rather
    // than blocking the runtime thread.
    if let Ok(mut store) = self.store.try_lock()
      && store.file.is_none()
    {
      if self.closed.load(Ordering::Acquire) {
        return Err(error("Database closed"));
      }
      return store.transaction(change, f);
    }
    let db = self.clone();
    tokio::task::spawn_blocking(move || db.transact(change, f))
      .await
      .map_err(io_error)?
  }
  fn memory() -> Self {
    Self {
      store: Arc::new(Mutex::new(Store {
        state: State::default(),
        file: None,
        ..Store::default()
      })),
      closed: Arc::new(AtomicBool::new(false)),
    }
  }
}
#[async_trait(?Send)]
impl crate::DatabaseHandler for NativeDbHandler {
  type DB = NativeDb;
  async fn open(
    &self,
    state: Rc<RefCell<OpState>>,
    path: Option<String>,
  ) -> Result<NativeDb, JsErrorBox> {
    if path.as_deref() == Some(":memory:") {
      return Ok(NativeDb::memory());
    }
    let path = match path {
      Some(path) => {
        if path.is_empty() {
          return Err(error("Filename cannot be empty"));
        }
        if path.starts_with(':') {
          return Err(error(
            "Filename cannot start with ':' unless prefixed with './'",
          ));
        }
        state
          .borrow()
          .borrow::<PermissionsContainer>()
          .check_open(
            std::borrow::Cow::Owned(PathBuf::from(path)),
            OpenAccessKind::ReadWriteNoFollow,
            Some("Oden.openKv"),
          )
          .map_err(JsErrorBox::from_err)?
          .to_path_buf()
      }
      None if std::env::var("DENO_KV_DB_MODE").as_deref() == Ok("memory") => {
        return Ok(NativeDb::memory());
      }
      None => match &self.default_storage_dir {
        Some(dir) => {
          #[allow(
            clippy::disallowed_methods,
            reason = "runtime storage directory"
          )]
          std::fs::create_dir_all(dir).map_err(io_error)?;
          dir.join("kv.native")
        }
        None => return Ok(NativeDb::memory()),
      },
    };
    if std::env::var("DENO_KV_DB_MODE").as_deref() == Ok("memory") {
      return Ok(NativeDb::memory());
    }
    #[cfg(target_os = "macos")]
    let path = {
      let mut resolved = path.clone();
      // Resolve only fixed OS aliases; user-controlled symlinks remain refused.
      for prefix in [std::path::Path::new("/var"), std::path::Path::new("/tmp")]
      {
        if let Ok(suffix) = path.strip_prefix(prefix) {
          let prefix = deno_path_util::fs::canonicalize_path_maybe_not_exists(
            &sys_traits::impls::RealSys,
            prefix,
          )
          .map_err(io_error)?;
          resolved = prefix.join(suffix);
          break;
        }
      }
      resolved
    };
    let file = tokio::task::spawn_blocking(move || open_file(&path))
      .await
      .map_err(io_error)?
      .map_err(|e| error(format!("unable to open database file: {e}")))?;
    let db = NativeDb {
      store: Arc::new(Mutex::new(Store {
        state: State::default(),
        file: Some(file),
        ..Store::default()
      })),
      closed: Arc::new(AtomicBool::new(false)),
    };
    db.transact_async(false, |_| Ok(())).await?;
    Ok(db)
  }
}

fn sum(
  old: Option<KvValue>,
  value: KvValue,
  min: Vec<u8>,
  max: Vec<u8>,
  clamp: bool,
) -> Result<KvValue, JsErrorBox> {
  if matches!(value, KvValue::U64(_))
    && old.as_ref().is_some_and(|v| !matches!(v, KvValue::U64(_)))
  {
    return Err(error(
      "Failed to perform 'sum' mutation on a non-U64 value in the database",
    ));
  }
  let mut operand =
    SumOperand::parse(&value).map_err(|e| error(e.to_string()))?;
  let min = SumOperand::parse_optional(&KvValue::V8(min))
    .map_err(|e| error(e.to_string()))?;
  let max = SumOperand::parse_optional(&KvValue::V8(max))
    .map_err(|e| error(e.to_string()))?;
  if [&min, &max].iter().any(|v| {
    v.as_ref().is_some_and(|v| {
      std::mem::discriminant(v) != std::mem::discriminant(&operand)
    })
  }) {
    return Err(error("Sum bounds must match operand type"));
  }
  let Some(old) = old else {
    return Ok(operand.encode());
  };
  let old = SumOperand::parse(&old).map_err(|e| error(e.to_string()))?;
  if let (SumOperand::KvU64(_), SumOperand::BigInt(n)) = (&old, &operand)
    && min.is_none()
    && max.is_none()
    && !clamp
    && let Ok(n) = u64::try_from(n.clone())
  {
    operand = SumOperand::KvU64(n);
  }
  let result = match (old, operand) {
    (SumOperand::KvU64(a), SumOperand::KvU64(b))
      if min.is_none() && max.is_none() =>
    {
      SumOperand::KvU64(a.wrapping_add(b))
    }
    (SumOperand::Number(a), SumOperand::Number(b)) => {
      let mut n = a + b;
      if let Some(SumOperand::Number(v)) = min
        && n < v
      {
        if !clamp {
          return Err(error(
            "The result of a Sum operation would exceed its range limit",
          ));
        }
        n = v;
      }
      if let Some(SumOperand::Number(v)) = max
        && n > v
      {
        if !clamp {
          return Err(error(
            "The result of a Sum operation would exceed its range limit",
          ));
        }
        n = v;
      }
      SumOperand::Number(n)
    }
    (SumOperand::BigInt(a), SumOperand::BigInt(b)) => {
      let mut n = a + b;
      if let Some(SumOperand::BigInt(v)) = min
        && n < v
      {
        if !clamp {
          return Err(error(
            "The result of a Sum operation would exceed its range limit",
          ));
        }
        n = v;
      }
      if let Some(SumOperand::BigInt(v)) = max
        && n > v
      {
        if !clamp {
          return Err(error(
            "The result of a Sum operation would exceed its range limit",
          ));
        }
        n = v;
      }
      SumOperand::BigInt(n)
    }
    (a, b) => {
      return Err(error(format!(
        "Cannot sum {} with {}",
        a.variant_name(),
        b.variant_name()
      )));
    }
  };
  Ok(result.encode())
}

#[async_trait(?Send)]
impl Database for NativeDb {
  type QMH = NativeMessage;
  async fn snapshot_read(
    &self,
    requests: Vec<ReadRange>,
    _options: SnapshotReadOptions,
  ) -> Result<Vec<ReadRangeOutput>, JsErrorBox> {
    self
      .transact_async(false, move |s| {
        requests
          .into_iter()
          .map(|r| {
            if r.start > r.end {
              return Err(error("Invalid range"));
            }
            let iter = s.entries.range(r.start..r.end);
            let entries = if r.reverse {
              iter
                .rev()
                .take(r.limit.get() as usize)
                .map(|(k, e)| e.output(k.clone()))
                .collect()
            } else {
              iter
                .take(r.limit.get() as usize)
                .map(|(k, e)| e.output(k.clone()))
                .collect()
            };
            Ok(ReadRangeOutput { entries })
          })
          .collect()
      })
      .await
  }
  async fn atomic_write(
    &self,
    write: AtomicWrite,
  ) -> Result<Option<CommitResult>, JsErrorBox> {
    self
      .transact_async(true, move |s| {
        if write
          .checks
          .iter()
          .any(|c| s.entries.get(&c.key).map(|e| e.version) != c.versionstamp)
        {
          return Ok(None);
        }
        let version = s.stamp()?;
        for m in write.mutations {
          let mut key = m.key;
          let old = s.entries.get(&key).map(Entry::value);
          let value = match m.kind {
            MutationKind::Delete => {
              s.set_entry(key, None);
              continue;
            }
            MutationKind::Set(v) => v,
            MutationKind::SetSuffixVersionstampedKey(v) => {
              key.push(2);
              key
                .extend_from_slice(faster_hex::hex_string(&version).as_bytes());
              key.push(0);
              v
            }
            MutationKind::Sum {
              value,
              min_v8,
              max_v8,
              clamp,
            } => sum(old, value, min_v8, max_v8, clamp)?,
            MutationKind::Min(v) => match (old, v) {
              (None, v @ KvValue::U64(_)) => v,
              (Some(KvValue::U64(a)), KvValue::U64(b)) => {
                KvValue::U64(a.min(b))
              }
              (_, KvValue::U64(_)) => return Err(error("Failed to perform 'min' mutation on a non-U64 value in the database")),
              _ => return Err(error("Failed to perform 'min' mutation on a non-U64 operand")),
            },
            MutationKind::Max(v) => match (old, v) {
              (None, v @ KvValue::U64(_)) => v,
              (Some(KvValue::U64(a)), KvValue::U64(b)) => {
                KvValue::U64(a.max(b))
              }
              (_, KvValue::U64(_)) => return Err(error("Failed to perform 'max' mutation on a non-U64 value in the database")),
              _ => return Err(error("Failed to perform 'max' mutation on a non-U64 operand")),
            },
          };
          s.set_entry(
            key,
            Some(Entry::new(
              value,
              version,
              m.expire_at.map(|t| t.timestamp_millis()),
            )),
          );
        }
        for e in write.enqueues {
          s.next_message = s
            .next_message
            .checked_add(1)
            .ok_or_else(|| error("Queue ID exhausted"))?;
          s.set_message(
            s.next_message,
            Some(Message {
              payload: e.payload,
              due: e.deadline.timestamp_millis(),
              keys: e.keys_if_undelivered,
              backoff: e
                .backoff_schedule
                .unwrap_or_else(|| vec![100, 1000, 5000, 30000, 60000]),
              attempt: 0,
              lease: 0,
            }),
          );
        }
        Ok(Some(CommitResult {
          versionstamp: version,
        }))
      })
      .await
  }
  async fn dequeue_next_message(
    &self,
  ) -> Result<Option<NativeMessage>, JsErrorBox> {
    loop {
      if self.closed.load(Ordering::Acquire) {
        return Ok(None);
      }
      let result = match self
        .transact_async(true, move |s| {
          let now = Utc::now().timestamp_millis();
          let Some((&id, _)) = s
            .messages
            .iter()
            .filter(|(_, m)| m.due <= now)
            .min_by_key(|(_, m)| m.due)
          else {
            return Ok(None);
          };
          let m = s.message_mut(id).unwrap();
          m.lease = m
            .lease
            .checked_add(1)
            .ok_or_else(|| error("Queue lease exhausted"))?;
          m.due = now + 5_000;
          Ok(Some((id, m.lease, m.payload.clone())))
        })
        .await
      {
        Ok(result) => result,
        Err(_) if self.closed.load(Ordering::Acquire) => return Ok(None),
        Err(error) => return Err(error),
      };
      if self.closed.load(Ordering::Acquire) {
        return Ok(None);
      }
      if let Some((id, lease, payload)) = result {
        let db = self.clone();
        let heartbeat = tokio::spawn(async move {
          loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if db.closed.load(Ordering::Acquire) {
              break;
            }
            let renewed = db
              .transact_async(true, move |s| {
                if let Some(m) = s.message_mut(id)
                  && m.lease == lease
                {
                  m.due = Utc::now().timestamp_millis() + 5_000;
                  return Ok(true);
                }
                Ok(false)
              })
              .await;
            if !matches!(renewed, Ok(true)) {
              break;
            }
          }
        });
        return Ok(Some(NativeMessage {
          db: self.clone(),
          id,
          lease,
          payload: RefCell::new(Some(payload)),
          heartbeat,
        }));
      }
      tokio::time::sleep(POLL).await;
    }
  }
  fn watch(&self, keys: Vec<Vec<u8>>) -> WatchStream {
    let db = self.clone();
    Box::pin(stream::unfold(
      (db, keys, None::<Vec<Option<Versionstamp>>>),
      |(db, keys, mut previous)| async move {
        loop {
          if db.closed.load(Ordering::Acquire) {
            return None;
          }
          let read_keys = keys.clone();
          let values = match db
            .transact_async(false, move |s| {
              Ok(
                read_keys
                  .iter()
                  .map(|k| s.entries.get(k).map(|e| e.output(k.clone())))
                  .collect::<Vec<_>>(),
              )
            })
            .await
          {
            Ok(v) => v,
            Err(_) if db.closed.load(Ordering::Acquire) => return None,
            Err(e) => return Some((Err(e), (db, keys, previous))),
          };
          if db.closed.load(Ordering::Acquire) {
            return None;
          }
          let versions: Vec<_> = values
            .iter()
            .map(|e| e.as_ref().map(|e| e.versionstamp))
            .collect();
          if previous.as_ref() != Some(&versions) {
            let output = values
              .into_iter()
              .enumerate()
              .map(|(i, entry)| {
                if previous.as_ref().is_some_and(|p| p[i] == versions[i]) {
                  WatchKeyOutput::Unchanged
                } else {
                  WatchKeyOutput::Changed { entry }
                }
              })
              .collect();
            previous = Some(versions);
            return Some((Ok(output), (db, keys, previous)));
          }
          tokio::time::sleep(POLL).await;
        }
      },
    ))
  }
  fn close(&self) {
    self.closed.store(true, Ordering::Release);
  }
}

pub struct NativeMessage {
  db: NativeDb,
  id: u64,
  lease: u64,
  payload: RefCell<Option<Vec<u8>>>,
  heartbeat: tokio::task::JoinHandle<()>,
}
impl Drop for NativeMessage {
  fn drop(&mut self) {
    self.heartbeat.abort();
  }
}
#[async_trait(?Send)]
impl QueueMessageHandle for NativeMessage {
  async fn take_payload(&mut self) -> Result<Vec<u8>, JsErrorBox> {
    self
      .payload
      .borrow_mut()
      .take()
      .ok_or_else(|| error("Payload already taken"))
  }
  async fn finish(&self, success: bool) -> Result<(), JsErrorBox> {
    self.heartbeat.abort();
    let id = self.id;
    let lease = self.lease;
    self
      .db
      .transact_async(true, move |s| {
        let Some(mut m) = s.messages.get(&id).cloned() else {
          return Ok(());
        };
        if m.lease != lease {
          return Ok(());
        }
        if success {
          s.set_message(id, None);
        } else if let Some(delay) = m.backoff.get(m.attempt) {
          m.due = Utc::now().timestamp_millis() + i64::from(*delay);
          m.attempt += 1;
          m.lease = m
            .lease
            .checked_add(1)
            .ok_or_else(|| error("Queue lease exhausted"))?;
          s.set_message(id, Some(m));
        } else {
          let version = s.stamp()?;
          for key in m.keys {
            s.set_entry(
              key,
              Some(Entry::new(KvValue::V8(m.payload.clone()), version, None)),
            );
          }
          s.set_message(id, None);
        }
        Ok(())
      })
      .await
  }
}

#[cfg(test)]
#[allow(
  clippy::disallowed_methods,
  reason = "tests exercise real files and locks"
)]
mod tests {
  use deno_core::futures::StreamExt;

  use super::*;

  fn disk(path: &std::path::Path) -> NativeDb {
    NativeDb {
      store: Arc::new(Mutex::new(Store {
        state: State::default(),
        file: Some(
          OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap(),
        ),
        ..Store::default()
      })),
      closed: Arc::new(AtomicBool::new(false)),
    }
  }
  fn write(key: &[u8], value: KvValue) -> AtomicWrite {
    AtomicWrite {
      checks: vec![],
      mutations: vec![Mutation {
        key: key.to_vec(),
        kind: MutationKind::Set(value),
        expire_at: None,
      }],
      enqueues: vec![],
    }
  }
  fn get(db: &NativeDb, key: &[u8]) -> Option<Entry> {
    db.transact(false, |s| Ok(s.entries.get(key).cloned()))
      .unwrap()
  }
  fn temp() -> PathBuf {
    std::env::temp_dir().join(format!(
      "native-kv-{}-{}",
      std::process::id(),
      rand::random::<u64>()
    ))
  }

  #[cfg(feature = "sqlite")]
  #[tokio::test]
  async fn default_storage_keeps_sqlite_and_native_separate() {
    use crate::DatabaseHandler;
    let dir = temp();
    let compatible =
      crate::sqlite::SqliteDbHandler::new(Some(dir.clone()), Some(42))
        .open(Rc::new(RefCell::new(OpState::new(None))), None)
        .await
        .unwrap();
    compatible
      .atomic_write(write(b"key", KvValue::U64(1)))
      .await
      .unwrap();
    let native = NativeDbHandler::new(Some(dir.clone()), None)
      .open(Rc::new(RefCell::new(OpState::new(None))), None)
      .await
      .unwrap();
    native
      .atomic_write(write(b"key", KvValue::U64(2)))
      .await
      .unwrap();
    assert!(matches!(
      get(&native, b"key").unwrap().value(),
      KvValue::U64(2)
    ));
    let rows = compatible
      .snapshot_read(
        vec![ReadRange {
          start: b"key".to_vec(),
          end: b"key\0".to_vec(),
          limit: std::num::NonZeroU32::new(1).unwrap(),
          reverse: false,
        }],
        SnapshotReadOptions {
          consistency: Consistency::Strong,
        },
      )
      .await
      .unwrap();
    assert!(matches!(rows[0].entries[0].value, KvValue::U64(1)));
    assert!(dir.join("kv.sqlite3").is_file());
    assert!(dir.join("kv.native").is_file());
    compatible.close();
    native.close();
    drop(compatible);
    drop(native);
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[tokio::test]
  async fn persistence_conflicts_and_rollback() {
    let path = temp();
    let a = disk(&path);
    let first = a
      .atomic_write(write(b"key", KvValue::U64(7)))
      .await
      .unwrap()
      .unwrap();
    let b = disk(&path);
    assert!(matches!(get(&b, b"key").unwrap().value(), KvValue::U64(7)));
    b.atomic_write(write(b"key", KvValue::U64(8)))
      .await
      .unwrap();
    let mut conflict = write(b"other", KvValue::U64(9));
    conflict.checks.push(Check {
      key: b"key".to_vec(),
      versionstamp: Some(first.versionstamp),
    });
    assert!(a.atomic_write(conflict).await.unwrap().is_none());
    assert!(get(&a, b"other").is_none());
    let mut invalid = write(b"other", KvValue::U64(10));
    invalid.mutations.push(Mutation {
      key: b"key".to_vec(),
      kind: MutationKind::Min(KvValue::Bytes(vec![])),
      expire_at: None,
    });
    assert!(a.atomic_write(invalid).await.is_err());
    assert!(get(&b, b"other").is_none());
    drop(a);
    drop(b);
    assert!(matches!(
      get(&disk(&path), b"key").unwrap().value(),
      KvValue::U64(8)
    ));
    std::fs::remove_file(path).unwrap();
  }

  #[tokio::test]
  async fn legacy_snapshot_then_delta_and_bounded_update_size() {
    let path = temp();
    let mut state = State::default();
    let version = state.stamp().unwrap();
    for i in 0..1000_u32 {
      state.entries.insert(
        i.to_be_bytes().to_vec(),
        Entry::new(KvValue::Bytes(vec![1; 256]), version, None),
      );
    }
    let bytes = bincode::serialize(&state).unwrap();
    let mut file = File::create(&path).unwrap();
    file.write_all(MAGIC).unwrap();
    file.write_all(&(bytes.len() as u64).to_le_bytes()).unwrap();
    file.write_all(&Sha256::digest(&bytes)).unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let original_len = std::fs::metadata(&path).unwrap().len();
    let a = disk(&path);
    let b = disk(&path);
    assert!(get(&b, &0_u32.to_be_bytes()).is_some());
    for i in 0..100_u32 {
      a.atomic_write(write(&i.to_be_bytes(), KvValue::Bytes(vec![2; 256])))
        .await
        .unwrap();
    }
    let updated_len = std::fs::metadata(&path).unwrap().len();
    assert!(updated_len - original_len < 100 * 512);
    // An already-open reader replays only the new delta frames.
    assert!(matches!(get(&b, &0_u32.to_be_bytes()).unwrap().value(),
      KvValue::Bytes(v) if v == vec![2; 256]));
    assert!(matches!(get(&b, &999_u32.to_be_bytes()).unwrap().value(),
      KvValue::Bytes(v) if v == vec![1; 256]));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), updated_len);
    drop(a);
    drop(b);
    let reopened = disk(&path);
    assert_eq!(
      reopened.transact(false, |s| Ok(s.entries.len())).unwrap(),
      1000
    );
    assert!(
      matches!(get(&reopened, &99_u32.to_be_bytes()).unwrap().value(),
      KvValue::Bytes(v) if v == vec![2; 256])
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
  }

  #[tokio::test]
  async fn ttl_index_and_undo_survive_failed_multi_key_mutation() {
    for persistent in [false, true] {
      let path = temp();
      let db = if persistent {
        disk(&path)
      } else {
        NativeDb::memory()
      };
      let mut initial = write(b"ttl", KvValue::U64(1));
      initial.mutations[0].expire_at =
        Some(Utc::now() + chrono::Duration::milliseconds(100));
      db.atomic_write(initial).await.unwrap();
      let before = get(&db, b"ttl").unwrap();
      let mut invalid = write(b"ttl", KvValue::U64(2));
      invalid.mutations.push(Mutation {
        key: b"missing".to_vec(),
        kind: MutationKind::Min(KvValue::Bytes(vec![])),
        expire_at: None,
      });
      assert!(db.atomic_write(invalid).await.is_err());
      assert_eq!(get(&db, b"ttl").unwrap().version, before.version);
      tokio::time::sleep(Duration::from_millis(120)).await;
      assert!(get(&db, b"ttl").is_none());
      // A successful overwrite cancels the old expiration deadline.
      let mut initial = write(b"ttl", KvValue::U64(3));
      initial.mutations[0].expire_at =
        Some(Utc::now() + chrono::Duration::milliseconds(30));
      db.atomic_write(initial).await.unwrap();
      db.atomic_write(write(b"ttl", KvValue::U64(4)))
        .await
        .unwrap();
      tokio::time::sleep(Duration::from_millis(40)).await;
      assert!(matches!(get(&db, b"ttl").unwrap().value(), KvValue::U64(4)));
      drop(db);
      if persistent {
        std::fs::remove_file(path).unwrap();
      }
    }
  }

  #[tokio::test]
  async fn delta_queue_state_survives_reopen_and_rollback() {
    let path = temp();
    let a = disk(&path);
    let mut enqueue = write(b"key", KvValue::U64(1));
    enqueue.enqueues.push(Enqueue {
      payload: vec![42],
      deadline: Utc::now(),
      keys_if_undelivered: vec![b"failed".to_vec()],
      backoff_schedule: Some(vec![0]),
    });
    a.atomic_write(enqueue).await.unwrap();
    let mut message = a.dequeue_next_message().await.unwrap().unwrap();
    assert_eq!(message.take_payload().await.unwrap(), vec![42]);
    message.finish(false).await.unwrap();
    drop(message);
    drop(a);
    let b = disk(&path);
    assert!(
      b.transact(true, |s| {
        s.message_mut(1).unwrap().payload = vec![99];
        Err::<(), _>(error("rollback"))
      })
      .is_err()
    );
    let mut message = b.dequeue_next_message().await.unwrap().unwrap();
    assert_eq!(message.take_payload().await.unwrap(), vec![42]);
    message.finish(false).await.unwrap();
    drop(message);
    drop(b);
    let c = disk(&path);
    assert!(matches!(get(&c, b"failed").unwrap().value(),
      KvValue::V8(v) if v == vec![42]));
    assert!(c.transact(false, |s| Ok(s.messages.is_empty())).unwrap());
    drop(c);
    std::fs::remove_file(path).unwrap();
  }

  #[tokio::test]
  async fn interrupted_tail_and_corruption() {
    let path = temp();
    let db = disk(&path);
    db.atomic_write(write(b"key", KvValue::U64(7)))
      .await
      .unwrap();
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&[1, 2, 3]).unwrap();
    assert!(get(&db, b"key").is_some());
    db.atomic_write(write(b"next", KvValue::U64(8)))
      .await
      .unwrap();
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(16)).unwrap();
    file.write_all(&[255; 32]).unwrap();
    assert!(db.transact(false, |_| Ok(())).is_err());
    std::fs::remove_file(path).unwrap();
  }

  #[tokio::test]
  async fn interrupted_first_commit_and_failed_io_preserve_state() {
    let path = temp();
    let original = disk(&path);
    original
      .atomic_write(write(b"key", KvValue::U64(7)))
      .await
      .unwrap();
    drop(original);
    let frame = std::fs::read(&path).unwrap();
    // Interruptions in the magic, frame header, checksum, and payload all
    // precede the commit boundary and must not expose the new value.
    for cut in [1, 7, 8, 12, 39, 48, frame.len() - 1] {
      std::fs::write(&path, &frame[..cut]).unwrap();
      let db = disk(&path);
      assert!(get(&db, b"key").is_none());
      db.atomic_write(write(b"next", KvValue::U64(8)))
        .await
        .unwrap();
      drop(db);
      let db = disk(&path);
      assert!(get(&db, b"key").is_none());
      assert!(matches!(
        get(&db, b"next").unwrap().value(),
        KvValue::U64(8)
      ));
    }
    std::fs::write(&path, &frame).unwrap();
    let mut store = Store {
      file: Some(File::open(&path).unwrap()),
      ..Store::default()
    };
    assert!(
      store
        .transaction(true, |s| {
          let version = s.stamp()?;
          s.set_entry(
            b"key".to_vec(),
            Some(Entry::new(KvValue::U64(99), version, None)),
          );
          Ok(())
        })
        .is_err()
    );
    assert!(store.file.is_some());
    assert!(matches!(
      store
        .transaction(false, |s| {
          Ok(s.entries.get(b"key".as_slice()).unwrap().value())
        })
        .unwrap(),
      KvValue::U64(7)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), frame);
    drop(store);
    std::fs::remove_file(path).unwrap();
  }

  #[tokio::test]
  async fn close_while_queue_and_watch_wait_for_worker() {
    for _ in 0..20 {
      let db = NativeDb::memory();
      let mut next = Box::pin(db.dequeue_next_message());
      {
        let _guard = db.store.lock().unwrap();
        assert!(
          std::future::Future::poll(
            next.as_mut(),
            &mut std::task::Context::from_waker(
              deno_core::futures::task::noop_waker_ref()
            )
          )
          .is_pending()
        );
        db.close();
      }
      assert!(next.await.unwrap().is_none());
    }
    let db = NativeDb::memory();
    let mut watch = db.watch(vec![b"key".to_vec()]);
    let mut next = Box::pin(watch.next());
    {
      let _guard = db.store.lock().unwrap();
      assert!(
        std::future::Future::poll(
          next.as_mut(),
          &mut std::task::Context::from_waker(
            deno_core::futures::task::noop_waker_ref()
          )
        )
        .is_pending()
      );
      db.close();
    }
    assert!(next.await.is_none());
  }

  #[tokio::test]
  async fn ttl_watch_and_queue_retry() {
    let db = NativeDb::memory();
    let mut watch = db.watch(vec![b"key".to_vec()]);
    assert!(matches!(
      &watch.next().await.unwrap().unwrap()[0],
      WatchKeyOutput::Changed { entry: None }
    ));
    db.atomic_write(write(b"key", KvValue::U64(u64::MAX)))
      .await
      .unwrap();
    assert!(matches!(
      &watch.next().await.unwrap().unwrap()[0],
      WatchKeyOutput::Changed { entry: Some(_) }
    ));
    let mut w = write(b"expired", KvValue::U64(1));
    w.mutations[0].expire_at = Some(Utc::now() - chrono::Duration::seconds(1));
    w.mutations.push(Mutation {
      key: b"key".to_vec(),
      kind: MutationKind::Sum {
        value: KvValue::U64(1),
        min_v8: vec![],
        max_v8: vec![],
        clamp: false,
      },
      expire_at: None,
    });
    w.enqueues.push(Enqueue {
      payload: vec![42],
      deadline: Utc::now(),
      keys_if_undelivered: vec![b"failed".to_vec()],
      backoff_schedule: Some(vec![0]),
    });
    db.atomic_write(w).await.unwrap();
    assert!(get(&db, b"expired").is_none());
    assert!(matches!(get(&db, b"key").unwrap().value(), KvValue::U64(0)));
    let mut message = db.dequeue_next_message().await.unwrap().unwrap();
    assert_eq!(message.take_payload().await.unwrap(), vec![42]);
    message.finish(false).await.unwrap();
    db.dequeue_next_message()
      .await
      .unwrap()
      .unwrap()
      .finish(false)
      .await
      .unwrap();
    assert!(
      matches!(get(&db, b"failed").unwrap().value(), KvValue::V8(v) if v == vec![42])
    );
    db.close();
    assert!(watch.next().await.is_none());
    assert!(db.dequeue_next_message().await.unwrap().is_none());
  }
}

#[cfg(test)]
#[allow(
  clippy::disallowed_methods,
  reason = "tests exercise real files and locks"
)]
mod process_tests {
  use super::*;

  #[test]
  fn concurrent_process_commits() {
    const ENV: &str = "DENO_NATIVE_KV_PROCESS_TEST_PATH";
    if let Some(path) = std::env::var_os(ENV) {
      let mut store = Store {
        state: State::default(),
        file: Some(
          OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap(),
        ),
        ..Store::default()
      };
      for _ in 0..20 {
        store
          .transaction(true, |s| {
            let version = s.stamp()?;
            let previous = s
              .entries
              .get(b"counter".as_slice())
              .map(|e| match e.value() {
                KvValue::U64(value) => value,
                _ => unreachable!(),
              })
              .unwrap_or(0);
            s.set_entry(
              b"counter".to_vec(),
              Some(Entry::new(KvValue::U64(previous + 1), version, None)),
            );
            Ok(())
          })
          .unwrap();
      }
      return;
    }
    let path = std::env::temp_dir().join(format!(
      "native-kv-process-{}-{}",
      std::process::id(),
      rand::random::<u64>()
    ));
    let exe = std::env::current_exe().unwrap();
    let mut children = Vec::new();
    for _ in 0..3 {
      children.push(
        std::process::Command::new(&exe)
          .arg("--exact")
          .arg("native::process_tests::concurrent_process_commits")
          .env(ENV, &path)
          .spawn()
          .unwrap(),
      );
    }
    for mut child in children {
      assert!(child.wait().unwrap().success());
    }
    let mut file = File::open(&path).unwrap();
    let state = Store::load(&mut file).unwrap().0;
    assert_eq!(state.version, 60);
    assert!(matches!(
      state.entries.get(b"counter".as_slice()).unwrap().value(),
      KvValue::U64(60)
    ));
    std::fs::remove_file(path).unwrap();
  }
}
