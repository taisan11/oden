// Copyright 2026 the Oden authors. MIT license.
import { core } from "ext:core/mod.js";
import * as ops from "ext:core/ops";
import { Buffer } from "node:buffer";

const token = Symbol("oden:sqlite constructor");
const resources = new FinalizationRegistry((rid) => core.tryClose(rid));

export class SQLiteError extends Error {
  constructor(message, code = "SQLITE_ERROR", errno = 1) {
    super(message);
    this.name = "SQLiteError";
    this.code = code;
    this.errno = errno;
    this.byteOffset = -1;
  }
}

function call(op, ...args) {
  try {
    return op(...args);
  } catch (error) {
    if (typeof error.code === "string" && error.code.startsWith("SQLITE_")) {
      throw new SQLiteError(error.message, error.code, error.errno);
    }
    throw error;
  }
}

function unsupported(name) {
  throw new Error(
    `${name} is not supported by the Turso backend of oden:sqlite`,
  );
}

function sqlString(sql) {
  if (typeof sql !== "string") throw new TypeError("SQL must be a string");
  if (sql.trim().length === 0) throw new Error("SQL query cannot be empty");
  if (sql.includes("\0")) {
    throw new TypeError("SQL must not contain null bytes");
  }
  return sql;
}

function encode(value) {
  if (value === null) return { type: "null" };
  switch (typeof value) {
    case "string":
      return { type: "text", value };
    case "boolean":
      return { type: "integer", value: value ? "1" : "0" };
    case "bigint":
      if (value < -9223372036854775808n || value > 9223372036854775807n) {
        throw new RangeError("SQLite integers must fit in signed 64 bits");
      }
      return { type: "integer", value: String(value) };
    case "number":
      if (Number.isNaN(value)) return { type: "null" };
      return Number.isInteger(value) && value >= -9223372036854775808 &&
          value < 9223372036854775808
        ? { type: "integer", value: BigInt(value).toString() }
        : { type: "real", value };
    case "object":
      if (ArrayBuffer.isView(value)) {
        return {
          type: "blob",
          value: Array.from(
            new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
          ),
        };
      }
  }
  throw new TypeError(
    "SQLite bindings must be string, number, bigint, boolean, null, or a typed array",
  );
}

function decode(value, safeIntegers, raw = false) {
  switch (value.type) {
    case "null":
      return null;
    case "integer":
      return safeIntegers ? BigInt(value.value) : Number(value.value);
    case "blob":
      return new Uint8Array(value.value);
    case "text":
      return raw ? new TextEncoder().encode(value.value) : value.value;
    default:
      return value.value;
  }
}

function normalizeOptions(options) {
  let flags = 6;
  let strict = false;
  let safeIntegers = false;
  if (typeof options === "number") {
    if (!Number.isInteger(options)) {
      throw new TypeError("SQLite open flags must be an integer");
    }
    // Turso has no SQLite C VFS or shared-cache modes.
    if (
      options &
      ~(1 | 2 | 4 | 0x80 | 0x8000 | 0x10000 | 0x40000 | 0x1000000 | 0x2000000)
    ) {
      unsupported("These SQLite open flags");
    }
    flags = options;
  } else if (options !== undefined) {
    if (!options || typeof options !== "object") {
      throw new TypeError("Database options must be an object or integer");
    }
    if ("readOnly" in options) {
      throw new TypeError('Misspelled option "readOnly" should be "readonly"');
    }
    for (
      const name of [
        "readonly",
        "readwrite",
        "create",
        "strict",
        "safeIntegers",
      ]
    ) {
      if (options[name] !== undefined && typeof options[name] !== "boolean") {
        throw new TypeError(`${name} must be a boolean`);
      }
    }
    flags = options.readonly ? 1 : 0;
    if (options.create) flags = 6;
    if (options.readwrite) flags |= 2;
    if (flags === 0 && ("strict" in options || "safeIntegers" in options)) {
      flags = 6;
    }
    strict = options.strict ?? false;
    safeIntegers = options.safeIntegers ?? false;
  }
  if (!(flags & 3) || (flags & 3) === 3) {
    throw new TypeError("Choose readonly or readwrite SQLite open flags");
  }
  return {
    readonly: !!(flags & 1),
    create: !!(flags & 4),
    memory: !!(flags & 0x80),
    strict,
    safeIntegers,
  };
}

export class Statement {
  #database;
  #rid;
  #info;
  #bindings;
  #prototype = Object.prototype;
  #active = false;
  #executed = false;
  #columnTypes = null;

  constructor(key, database, info) {
    if (key !== token) {
      throw new TypeError(
        "Statements are created by Database.prepare() or Database.query()",
      );
    }
    this.#database = database;
    this.#info = info;
    this.#rid = info.rid;
    this.#bindings = info.parameters.map(() => ({ type: "null" }));
    resources.register(this, info.rid, this);
  }

  get isFinalized() {
    return this.#rid === undefined || this.#database.closed;
  }
  get native() {
    return this.#rid;
  }
  get columnNames() {
    this.#check();
    return this.#info.columnNames.slice();
  }
  get paramsCount() {
    this.#check();
    return this.#info.parameters.length;
  }
  get declaredTypes() {
    this.#check();
    return this.#executed ? this.#info.declaredTypes.slice() : null;
  }
  get columnTypes() {
    this.#check();
    if (!this.#info.readonly) {
      throw new Error("columnTypes is only available for read-only statements");
    }
    if (this.#columnTypes === null) {
      // Probe without changing the user's bindings or active cursor.
      const probe = this.#database.prepare(this.toString());
      try {
        const row = probe.get();
        this.#columnTypes = row === null
          ? this.#info.columnNames.map(() => "NULL")
          : probe.#columnTypes;
      } finally {
        probe.finalize();
      }
    }
    return this.#columnTypes?.slice() ?? null;
  }

  #check() {
    if (this.isFinalized) {
      throw new Error("Statement has been finalized or database is closed");
    }
  }

  #bind(args) {
    if (this.#bindings.length === 0) return this.#bindings;
    if (args.length === 0) return this.#bindings;
    let values = args;
    if (args.length === 1 && Array.isArray(args[0])) values = args[0];
    else if (
      args.length === 1 && args[0] !== null && typeof args[0] === "object" &&
      !ArrayBuffer.isView(args[0])
    ) {
      const object = args[0];
      const strict = this.#database.strict;
      values = this.#info.parameters.map((name, i) => {
        let key = name;
        if (
          name !== null && !Object.hasOwn(object, name) && strict &&
          Object.hasOwn(object, name.slice(1))
        ) key = name.slice(1);
        if (key === null || !Object.hasOwn(object, key)) {
          if (strict) {
            throw new Error(`Missing SQLite binding ${name ?? i + 1}`);
          }
          return null;
        }
        return object[key];
      });
    }
    if (values.length !== this.#bindings.length) {
      throw new Error(
        `Expected ${this.#bindings.length} bindings, got ${values.length}`,
      );
    }
    return values.map(encode);
  }

  #start(args) {
    this.#check();
    if (this.#active) throw new Error("Statement is already iterating");
    const bindings = this.#bind(args);
    call(ops.op_oden_sqlite_start, this.#rid, bindings);
    this.#bindings = bindings;
    this.#active = true;
    this.#executed = true;
    this.#columnTypes = null;
  }

  #next(raw = false) {
    const row = call(ops.op_oden_sqlite_next, this.#rid);
    if (row === null) return null;
    if (this.#columnTypes === null) {
      this.#columnTypes = row.map((v) =>
        ({
          integer: "INTEGER",
          real: "FLOAT",
          text: "TEXT",
          blob: "BLOB",
          null: "NULL",
        })[v.type]
      );
    }
    return row.map((v) => decode(v, this.#database.safeIntegers, raw));
  }

  #reset() {
    this.#active = false;
    if (!this.isFinalized) call(ops.op_oden_sqlite_reset, this.#rid);
  }

  #object(values) {
    const row = Object.create(this.#prototype);
    for (let i = 0; i < values.length; i++) {
      Object.defineProperty(row, this.#info.columnNames[i], {
        value: values[i],
        enumerable: true,
        configurable: true,
        writable: true,
      });
    }
    return row;
  }

  get(...args) {
    this.#start(args);
    try {
      const values = this.#next();
      // Complete writes with RETURNING before resetting the cursor.
      if (!this.#info.readonly && values !== null) {
        while (this.#next() !== null) { /* drain */ }
      }
      return values === null ? null : this.#object(values);
    } finally {
      this.#reset();
    }
  }

  all(...args) {
    return Array.from(this.iterate(...args));
  }

  values(...args) {
    return this.#values(args, false);
  }
  raw(...args) {
    return this.#values(args, true);
  }
  #values(args, raw) {
    this.#start(args);
    try {
      const rows = [];
      let row;
      while ((row = this.#next(raw)) !== null) rows.push(row);
      return rows;
    } finally {
      this.#reset();
    }
  }

  run(...args) {
    this.#start(args);
    try {
      while (this.#next() !== null) { /* discard */ }
      return this.#database._changes();
    } finally {
      this.#reset();
    }
  }

  *iterate(...args) {
    this.#start(args);
    try {
      let row;
      while ((row = this.#next()) !== null) yield this.#object(row);
    } finally {
      this.#reset();
    }
  }

  [Symbol.iterator]() {
    return this.iterate();
  }
  as(Class) {
    this.#check();
    if (typeof Class !== "function" || !Class.prototype) {
      throw new TypeError("Expected a class constructor");
    }
    this.#prototype = Class.prototype;
    return this;
  }
  finalize() {
    if (this.#rid !== undefined) {
      core.tryClose(this.#rid);
      resources.unregister(this);
      this.#rid = undefined;
      this.#active = false;
    }
  }
  [Symbol.dispose]() {
    this.finalize();
  }
  toString() {
    this.#check();
    return call(ops.op_oden_sqlite_expanded_sql, this.#rid);
  }
  toJSON() {
    return {
      sql: this.toString(),
      isFinalized: this.isFinalized,
      paramsCount: this.paramsCount,
      columnNames: this.columnNames,
    };
  }
  // Used only by Database.prepare to bind without executing the statement.
  _bind(params) {
    const bindings = this.#bind([params]);
    call(ops.op_oden_sqlite_start, this.#rid, bindings);
    this.#bindings = bindings;
  }
}

export class Database {
  static MAX_QUERY_CACHE_SIZE = 20;
  #rid;
  #cache = new Map();
  #options;
  #savepoint = 0;

  constructor(filename = ":memory:", options) {
    this.#options = normalizeOptions(options);
    if (
      ArrayBuffer.isView(filename) || filename instanceof ArrayBuffer ||
      filename instanceof SharedArrayBuffer
    ) {
      const bytes = ArrayBuffer.isView(filename)
        ? new Uint8Array(
          filename.buffer,
          filename.byteOffset,
          filename.byteLength,
        )
        : new Uint8Array(filename);
      this.filename = ":memory:";
      this.#rid = call(
        ops.op_oden_sqlite_deserialize,
        bytes,
        this.#options.readonly,
        false,
      );
      resources.register(this, this.#rid, this);
      return;
    }
    if (typeof filename !== "string" || filename.includes("\0")) {
      throw new TypeError(
        "Database filename must be a string without null bytes",
      );
    }
    filename = filename.trim();
    this.filename = filename;
    const path = this.#options.memory || filename === ""
      ? ":memory:"
      : filename;
    if (path === ":memory:" && this.#options.readonly) {
      throw new Error("Cannot open an anonymous database in read-only mode");
    }
    this.#rid = call(
      ops.op_oden_sqlite_open,
      path,
      this.#options.readonly,
      this.#options.create,
      false,
    );
    resources.register(this, this.#rid, this);
  }

  static open(filename, options) {
    return new Database(filename, options);
  }
  get handle() {
    return this.#rid;
  }
  get closed() {
    return this.#rid === undefined;
  }
  get strict() {
    return this.#options.strict;
  }
  get safeIntegers() {
    return this.#options.safeIntegers;
  }
  get inTransaction() {
    this.#check();
    return call(ops.op_oden_sqlite_in_transaction, this.#rid);
  }
  get [Symbol.for("Bun.Database.cache.count")]() {
    return this.#cache.size;
  }
  #check() {
    if (this.closed) throw new Error("Database is closed");
  }

  prepare(sql, params, flags = 0) {
    return this.#prepare(sql, params, flags, false);
  }
  #prepare(sql, params, flags, owned) {
    this.#check();
    if (!Number.isInteger(flags) || flags < 0 || flags & ~3) {
      unsupported("These SQLite prepare flags");
    }
    const statement = new Statement(
      token,
      this,
      call(ops.op_oden_sqlite_prepare, this.#rid, sqlString(sql), owned),
    );
    try {
      if (params !== undefined) statement._bind(params);
    } catch (error) {
      statement.finalize();
      throw error;
    }
    return statement;
  }

  query(sql) {
    this.#check();
    sqlString(sql);
    let statement = this.#cache.get(sql);
    this.#cache.delete(sql);
    if (statement === undefined || statement.isFinalized) {
      statement = this.#prepare(sql, undefined, 1, true);
    }
    if (Database.MAX_QUERY_CACHE_SIZE > 0) {
      while (this.#cache.size >= Database.MAX_QUERY_CACHE_SIZE) {
        this.#cache.delete(this.#cache.keys().next().value);
      }
      this.#cache.set(sql, statement);
    }
    return statement;
  }

  clearQueryCache() {
    for (const statement of this.#cache.values()) statement.finalize();
    this.#cache.clear();
  }

  _changes() {
    this.#check();
    const result = call(ops.op_oden_sqlite_changes, this.#rid);
    result.lastInsertRowid = this.safeIntegers
      ? BigInt(result.lastInsertRowid)
      : Number(result.lastInsertRowid);
    return result;
  }

  run(sql, ...bindings) {
    this.#check();
    sqlString(sql);
    let encoded = null;
    if (bindings.length > 0) {
      const first = bindings[0];
      if (
        bindings.length === 1 && first !== null && typeof first === "object" &&
        !Array.isArray(first) && !ArrayBuffer.isView(first)
      ) {
        encoded = {
          type: "named",
          value: Object.entries(first).map((
            [name, value],
          ) => [name, encode(value)]),
        };
      } else {
        const values = bindings.length === 1 && Array.isArray(first)
          ? first
          : bindings;
        encoded = { type: "positional", value: values.map(encode) };
      }
    }
    call(ops.op_oden_sqlite_run, this.#rid, sql, encoded, this.strict);
    return this._changes();
  }

  transaction(fn) {
    if (typeof fn !== "function") {
      throw new TypeError("Expected a transaction callback");
    }
    function wrap(mode, database) {
      return function (...args) {
        const nested = database.inTransaction;
        const savepoint = `oden_sqlite_${++database.#savepoint}`;
        database.run(nested ? `SAVEPOINT ${savepoint}` : `BEGIN ${mode}`);
        try {
          const result = Reflect.apply(fn, this, args);
          if (result && typeof result.then === "function") {
            throw new TypeError(
              "SQLite transaction callbacks must be synchronous",
            );
          }
          database.run(nested ? `RELEASE ${savepoint}` : "COMMIT");
          return result;
        } catch (error) {
          if (database.inTransaction) {
            database.run(nested ? `ROLLBACK TO ${savepoint}` : "ROLLBACK");
            if (nested) database.run(`RELEASE ${savepoint}`);
          }
          throw error;
        }
      };
    }
    const variants = {
      default: wrap("DEFERRED", this),
      deferred: wrap("DEFERRED", this),
      immediate: wrap("IMMEDIATE", this),
      exclusive: wrap("EXCLUSIVE", this),
      database: this,
    };
    for (
      const variant of [
        variants.default,
        variants.deferred,
        variants.immediate,
        variants.exclusive,
      ]
    ) {
      for (const [name, value] of Object.entries(variants)) {
        Object.defineProperty(variant, name, {
          value,
          enumerable: name === "database",
        });
      }
    }
    return variants.default;
  }

  close(throwOnError = false) {
    if (typeof throwOnError !== "boolean") {
      throw new TypeError("throwOnError must be a boolean");
    }
    if (this.closed) return;
    call(ops.op_oden_sqlite_close, this.#rid, throwOnError);
    this.#cache.clear();
    resources.unregister(this);
    this.#rid = undefined;
  }
  [Symbol.dispose]() {
    this.close(true);
  }
  serialize(name = "main") {
    this.#check();
    if (name !== "main") unsupported("Serializing a schema other than main");
    return Buffer.from(call(ops.op_oden_sqlite_serialize, this.#rid));
  }
  static deserialize(bytes, options = false) {
    return new Database(
      bytes,
      typeof options === "boolean"
        ? { readonly: options, strict: false }
        : options,
    );
  }
  loadExtension() {
    this.#check();
    unsupported("Database.loadExtension()");
  }
  fileControl() {
    this.#check();
    unsupported("Database.fileControl()");
  }
  static setCustomSQLite() {
    unsupported("Database.setCustomSQLite()");
  }
}

Database.prototype.exec = Database.prototype.run;

export const constants = Object.freeze({
  SQLITE_OPEN_READONLY: 1,
  SQLITE_OPEN_READWRITE: 2,
  SQLITE_OPEN_CREATE: 4,
  SQLITE_OPEN_DELETEONCLOSE: 8,
  SQLITE_OPEN_EXCLUSIVE: 16,
  SQLITE_OPEN_AUTOPROXY: 32,
  SQLITE_OPEN_URI: 64,
  SQLITE_OPEN_MEMORY: 128,
  SQLITE_OPEN_MAIN_DB: 256,
  SQLITE_OPEN_TEMP_DB: 512,
  SQLITE_OPEN_TRANSIENT_DB: 1024,
  SQLITE_OPEN_MAIN_JOURNAL: 2048,
  SQLITE_OPEN_TEMP_JOURNAL: 4096,
  SQLITE_OPEN_SUBJOURNAL: 8192,
  SQLITE_OPEN_SUPER_JOURNAL: 16384,
  SQLITE_OPEN_NOMUTEX: 32768,
  SQLITE_OPEN_FULLMUTEX: 65536,
  SQLITE_OPEN_SHAREDCACHE: 131072,
  SQLITE_OPEN_PRIVATECACHE: 262144,
  SQLITE_OPEN_WAL: 524288,
  SQLITE_OPEN_NOFOLLOW: 16777216,
  SQLITE_OPEN_EXRESCODE: 33554432,
  SQLITE_PREPARE_PERSISTENT: 1,
  SQLITE_PREPARE_NORMALIZE: 2,
  SQLITE_PREPARE_NO_VTAB: 4,
  SQLITE_DESERIALIZE_READONLY: 4,
  SQLITE_FCNTL_LOCKSTATE: 1,
  SQLITE_FCNTL_GET_LOCKPROXYFILE: 2,
  SQLITE_FCNTL_SET_LOCKPROXYFILE: 3,
  SQLITE_FCNTL_LAST_ERRNO: 4,
  SQLITE_FCNTL_SIZE_HINT: 5,
  SQLITE_FCNTL_CHUNK_SIZE: 6,
  SQLITE_FCNTL_FILE_POINTER: 7,
  SQLITE_FCNTL_SYNC_OMITTED: 8,
  SQLITE_FCNTL_WIN32_AV_RETRY: 9,
  SQLITE_FCNTL_PERSIST_WAL: 10,
  SQLITE_FCNTL_OVERWRITE: 11,
  SQLITE_FCNTL_VFSNAME: 12,
  SQLITE_FCNTL_POWERSAFE_OVERWRITE: 13,
  SQLITE_FCNTL_PRAGMA: 14,
  SQLITE_FCNTL_BUSYHANDLER: 15,
  SQLITE_FCNTL_TEMPFILENAME: 16,
  SQLITE_FCNTL_MMAP_SIZE: 18,
  SQLITE_FCNTL_TRACE: 19,
  SQLITE_FCNTL_HAS_MOVED: 20,
  SQLITE_FCNTL_SYNC: 21,
  SQLITE_FCNTL_COMMIT_PHASETWO: 22,
  SQLITE_FCNTL_WIN32_SET_HANDLE: 23,
  SQLITE_FCNTL_WAL_BLOCK: 24,
  SQLITE_FCNTL_ZIPVFS: 25,
  SQLITE_FCNTL_RBU: 26,
  SQLITE_FCNTL_VFS_POINTER: 27,
  SQLITE_FCNTL_JOURNAL_POINTER: 28,
  SQLITE_FCNTL_WIN32_GET_HANDLE: 29,
  SQLITE_FCNTL_PDB: 30,
  SQLITE_FCNTL_BEGIN_ATOMIC_WRITE: 31,
  SQLITE_FCNTL_COMMIT_ATOMIC_WRITE: 32,
  SQLITE_FCNTL_ROLLBACK_ATOMIC_WRITE: 33,
  SQLITE_FCNTL_LOCK_TIMEOUT: 34,
  SQLITE_FCNTL_DATA_VERSION: 35,
  SQLITE_FCNTL_SIZE_LIMIT: 36,
  SQLITE_FCNTL_CKPT_DONE: 37,
  SQLITE_FCNTL_RESERVE_BYTES: 38,
  SQLITE_FCNTL_CKPT_START: 39,
  SQLITE_FCNTL_EXTERNAL_READER: 40,
  SQLITE_FCNTL_CKSM_FILE: 41,
  SQLITE_FCNTL_RESET_CACHE: 42,
});
export const native = Object.freeze({ backend: "turso", version: "0.8.3" });
export default Database;
