// Copyright 2026 the Oden authors. MIT license.
(function () {
  const { core } = __bootstrap;
  const ops = core.ops;
  const dbs = new WeakMap();
  const statements = new WeakMap();
  const stores = new WeakMap();
  const token = Symbol("node:sqlite constructor");
  const resources = new FinalizationRegistry((rid) =>
    ops.op_oden_sqlite_release(rid)
  );

  function error(message, code, Class = Error) {
    const result = new Class(message);
    result.code = code;
    return result;
  }
  function invalidType(message) {
    return error(message, "ERR_INVALID_ARG_TYPE", TypeError);
  }
  function invalidValue(message) {
    return error(message, "ERR_INVALID_ARG_VALUE", TypeError);
  }
  function invalidState(message = "database is not open") {
    return error(message, "ERR_INVALID_STATE");
  }
  function unsupported(feature) {
    throw error(
      `${feature} is not supported by the Turso backend of node:sqlite`,
      "ERR_SQLITE_UNSUPPORTED",
    );
  }
  function call(op, ...args) {
    try {
      return op(...args);
    } catch (cause) {
      if (cause.code?.startsWith("SQLITE_")) {
        const result = error(
          cause.message.replace(/^Parse error: /, ""),
          "ERR_SQLITE_ERROR",
        );
        result.errcode = cause.errno;
        result.errstr = ({
          1: "SQL logic error",
          5: "database is locked",
          6: "database table is locked",
          8: "attempt to write a readonly database",
          9: "interrupted",
          11: "database disk image is malformed",
          13: "database or disk is full",
          19: "constraint failed",
          26: "file is not a database",
        })[cause.errno & 255] ?? "SQL logic error";
        throw result;
      }
      if (
        [
          "NotFound",
          "PermissionDenied",
          "AlreadyExists",
          "IsADirectory",
          "NotADirectory",
          "WriteZero",
          "UnexpectedEof",
        ].includes(cause.name) ||
        ["ENOENT", "EACCES", "EPERM", "EEXIST", "EISDIR", "ENOTDIR", "EIO"]
          .includes(cause.code)
      ) {
        const result = error(cause.message, "ERR_SQLITE_ERROR");
        result.errcode = 14;
        result.errstr = "unable to open database file";
        throw result;
      }
      throw cause;
    }
  }
  function boolean(value, name) {
    if (typeof value !== "boolean") {
      throw invalidType(`${name} must be a boolean`);
    }
    return value;
  }
  function string(value, name) {
    if (typeof value !== "string") {
      throw invalidType(`${name} must be a string`);
    }
    if (value.includes("\0")) {
      throw invalidValue(`${name} must not contain null bytes`);
    }
    return value;
  }
  function object(value, name) {
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw invalidType(`${name} must be an object`);
    }
    return value;
  }
  function path(value) {
    if (value instanceof Uint8Array) value = new TextDecoder().decode(value);
    else if (
      typeof value === "object" && value !== null && value instanceof URL
    ) {
      value = core.createLazyLoader("node:url")().fileURLToPath(value);
    }
    return string(value, "path");
  }
  function options(value = {}) {
    object(value, "options");
    const parsed = {};
    for (
      const [name, fallback] of Object.entries({
        open: true,
        readOnly: false,
        readBigInts: false,
        returnArrays: false,
        allowBareNamedParameters: true,
        allowUnknownNamedParameters: false,
        enableForeignKeyConstraints: true,
        enableDoubleQuotedStringLiterals: false,
        allowExtension: false,
        defensive: true,
      })
    ) {
      parsed[name] = value[name] === undefined
        ? fallback
        : boolean(value[name], name);
    }
    parsed.timeout = value.timeout ?? 0;
    if (
      !Number.isInteger(parsed.timeout) || parsed.timeout < 0 ||
      parsed.timeout > 4294967295
    ) {
      throw error(
        "timeout must be a non-negative uint32",
        "ERR_OUT_OF_RANGE",
        RangeError,
      );
    }
    if (parsed.allowExtension) unsupported("SQLite extension loading");
    if (!parsed.defensive) unsupported("Disabling SQLite defensive mode");
    if (value.limits !== undefined) unsupported("SQLite runtime limits");
    return parsed;
  }
  function databaseState(db, requireOpen = true) {
    const state = dbs.get(db);
    if (!state) throw invalidType("Expected a DatabaseSync instance");
    if (requireOpen && state.rid === undefined) throw invalidState();
    return state;
  }
  function configure(rid, opts) {
    call(
      ops.op_oden_sqlite_configure,
      rid,
      opts.enableForeignKeyConstraints,
      opts.enableDoubleQuotedStringLiterals,
      opts.timeout,
    );
  }
  function encode(value) {
    if (value === null) return { type: "null" };
    if (typeof value === "string") return { type: "text", value };
    if (typeof value === "bigint") {
      if (value < -9223372036854775808n || value > 9223372036854775807n) {
        throw error(
          "SQLite integer is out of range",
          "ERR_OUT_OF_RANGE",
          RangeError,
        );
      }
      return { type: "integer", value: String(value) };
    }
    if (typeof value === "number") {
      if (Number.isNaN(value)) return { type: "null" };
      return Number.isInteger(value) && value >= -9223372036854775808 &&
          value < 9223372036854775808
        ? { type: "integer", value: String(BigInt(value)) }
        : { type: "real", value };
    }
    if (ArrayBuffer.isView(value)) {
      return {
        type: "blob",
        value: Array.from(
          new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
        ),
      };
    }
    throw invalidType("Provided value cannot be bound to a SQLite parameter");
  }
  function decode(value, bigints) {
    if (value.type === "null") return null;
    if (value.type === "blob") return new Uint8Array(value.value);
    if (value.type !== "integer") return value.value;
    const integer = BigInt(value.value);
    if (bigints) return integer;
    if (integer > 9007199254740991n || integer < -9007199254740991n) {
      throw error(
        `Value is too large to be represented as a JavaScript number: ${integer}`,
        "ERR_OUT_OF_RANGE",
        RangeError,
      );
    }
    return Number(integer);
  }
  function statementState(statement) {
    const state = statements.get(statement);
    if (!state) throw invalidType("Expected a StatementSync instance");
    const db = databaseState(state.db);
    if (state.rid === undefined || state.databaseGeneration !== db.generation) {
      throw invalidState("statement has been finalized");
    }
    return state;
  }
  function bindings(state, args) {
    const parameters = state.info.parameters;
    const values = parameters.map(() => ({ type: "null" }));
    let named;
    if (
      args.length && args[0] !== null && typeof args[0] === "object" &&
      !ArrayBuffer.isView(args[0])
    ) named = args.shift();
    if (named !== undefined) {
      for (const key of Object.keys(named)) {
        if (key.includes("\0")) {
          throw invalidValue("Parameter name must not contain null bytes");
        }
        const indices = [];
        for (let i = 0; i < parameters.length; i++) {
          const name = parameters[i];
          if (
            name === key ||
            (state.bare && name !== null && !/^[?$@:]/.test(key) &&
              name.slice(1) === key)
          ) indices.push(i);
        }
        if (indices.length > 1) {
          throw error(
            `Cannot create bare named parameter '${key}' because of conflicting names`,
            "ERR_SQLITE_ERROR",
          );
        }
        if (!indices.length) {
          if (!state.unknown) {
            throw error(`Unknown named parameter '${key}'`, "ERR_SQLITE_ERROR");
          }
          continue;
        }
        values[indices[0]] = encode(named[key]);
      }
    }
    let argument = 0;
    for (let i = 0; i < parameters.length && argument < args.length; i++) {
      if (parameters[i] === null || parameters[i].startsWith("?")) {
        values[i] = encode(args[argument++]);
      }
    }
    if (argument !== args.length) {
      throw error("column index out of range", "ERR_SQLITE_ERROR");
    }
    return values;
  }
  function start(statement, args) {
    const state = statementState(statement);
    const values = bindings(state, args);
    call(ops.op_oden_sqlite_start, state.rid, values);
    state.cursor++;
    return state;
  }
  function reset(state) {
    call(ops.op_oden_sqlite_reset, state.rid);
  }
  function row(state, values) {
    const decoded = values.map((value) => decode(value, state.bigints));
    if (state.arrays) return decoded;
    const result = Object.create(null);
    for (let i = 0; i < decoded.length; i++) {
      result[state.info.columns[i].name] = decoded[i];
    }
    return result;
  }
  function releaseStatement(statement) {
    const state = statements.get(statement);
    if (state?.rid !== undefined) {
      ops.op_oden_sqlite_release(state.rid);
      resources.unregister(statement);
      state.rid = undefined;
      state.cursor++;
    }
  }

  class StatementSync {
    constructor(key, db, info) {
      if (key !== token) {
        throw error(
          "Illegal constructor",
          "ERR_ILLEGAL_CONSTRUCTOR",
          TypeError,
        );
      }
      const opts = databaseState(db).options;
      const metadata = call(ops.op_oden_sqlite_statement_info, info.rid);
      statements.set(this, {
        db,
        rid: info.rid,
        info: metadata,
        readonly: info.readonly,
        databaseGeneration: databaseState(db).generation,
        cursor: 0,
        bare: opts.allowBareNamedParameters,
        unknown: opts.allowUnknownNamedParameters,
        bigints: opts.readBigInts,
        arrays: opts.returnArrays,
      });
      resources.register(this, info.rid, this);
    }
    get sourceSQL() {
      return statementState(this).info.sourceSql;
    }
    get expandedSQL() {
      return call(ops.op_oden_sqlite_expanded_sql, statementState(this).rid);
    }
    columns() {
      return call(ops.op_oden_sqlite_statement_info, statementState(this).rid)
        .columns;
    }
    get(...args) {
      const state = start(this, args);
      try {
        const values = call(ops.op_oden_sqlite_next, state.rid);
        if (!state.readonly && values !== null) {
          while (call(ops.op_oden_sqlite_next, state.rid) !== null) {
            /* complete RETURNING writes */
          }
        }
        return values === null ? undefined : row(state, values);
      } finally {
        reset(state);
      }
    }
    all(...args) {
      const state = start(this, args);
      try {
        const result = [];
        let values;
        while ((values = call(ops.op_oden_sqlite_next, state.rid)) !== null) {
          result.push(row(state, values));
        }
        return result;
      } finally {
        reset(state);
      }
    }
    run(...args) {
      const state = start(this, args);
      try {
        while (
          call(ops.op_oden_sqlite_next, state.rid) !== null
        ) { /* discard rows */ }
        const result = call(
          ops.op_oden_sqlite_changes,
          databaseState(state.db).rid,
        );
        return {
          changes: state.bigints ? BigInt(result.changes) : result.changes,
          lastInsertRowid: state.bigints
            ? BigInt(result.lastInsertRowid)
            : Number(result.lastInsertRowid),
        };
      } finally {
        reset(state);
      }
    }
    iterate(...args) {
      statementState(this);
      const prototype = globalThis.Iterator?.prototype ?? Object.prototype;
      const state = start(this, args);
      const generation = state.cursor;
      let done = false;
      const end = () => {
        if (!done) {
          done = true;
          if (
            generation === state.cursor &&
            dbs.get(state.db).rid !== undefined &&
            state.databaseGeneration === dbs.get(state.db).generation
          ) reset(state);
        }
        return { __proto__: null, done: true, value: null };
      };
      return Object.setPrototypeOf({
        next: () => {
          if (done) return { __proto__: null, done: true, value: null };
          statementState(this);
          if (generation !== state.cursor) {
            throw invalidState("iterator has been invalidated");
          }
          try {
            const values = call(ops.op_oden_sqlite_next, state.rid);
            return values === null
              ? end()
              : { __proto__: null, done: false, value: row(state, values) };
          } catch (cause) {
            end();
            throw cause;
          }
        },
        return: end,
        throw: (cause) => {
          end();
          throw cause;
        },
        [Symbol.iterator]() {
          return this;
        },
        [Symbol.dispose]: end,
      }, prototype);
    }
    setAllowBareNamedParameters(enabled) {
      statementState(this).bare = boolean(enabled, "enabled");
    }
    setAllowUnknownNamedParameters(enabled) {
      statementState(this).unknown = boolean(enabled, "enabled");
    }
    setReadBigInts(enabled) {
      statementState(this).bigints = boolean(enabled, "enabled");
    }
    setReturnArrays(enabled) {
      statementState(this).arrays = boolean(enabled, "enabled");
    }
  }

  class SQLTagStore {
    constructor(key, db, capacity) {
      if (key !== token) {
        throw error(
          "Illegal constructor",
          "ERR_ILLEGAL_CONSTRUCTOR",
          TypeError,
        );
      }
      stores.set(this, { db, capacity, cache: new Map() });
    }
    get capacity() {
      return stores.get(this).capacity;
    }
    get size() {
      return stores.get(this).cache.size;
    }
    get db() {
      return stores.get(this).db;
    }
    clear() {
      const state = stores.get(this);
      for (const statement of state.cache.values()) releaseStatement(statement);
      state.cache.clear();
    }
    #statement(strings, values) {
      const state = stores.get(this);
      databaseState(state.db);
      if (!Array.isArray(strings) || strings.length !== values.length + 1) {
        throw invalidType("Expected a template strings array");
      }
      const text = strings.map((part) => string(part, "template string")).join(
        "?",
      );
      let statement = state.cache.get(text);
      state.cache.delete(text);
      if (statement === undefined) statement = state.db.prepare(text);
      if (state.cache.size >= state.capacity) {
        state.cache.delete(state.cache.keys().next().value);
      }
      state.cache.set(text, statement);
      return statement;
    }
    run(strings, ...values) {
      return this.#statement(strings, values).run(...values);
    }
    get(strings, ...values) {
      return this.#statement(strings, values).get(...values);
    }
    all(strings, ...values) {
      return this.#statement(strings, values).all(...values);
    }
    iterate(strings, ...values) {
      return this.#statement(strings, values).iterate(...values);
    }
  }

  class DatabaseSync {
    constructor(filename, opts) {
      const state = {
        path: path(filename),
        options: options(opts),
        rid: undefined,
        generation: 0,
      };
      dbs.set(this, state);
      if (state.options.open) this.open();
    }
    get isOpen() {
      return databaseState(this, false).rid !== undefined;
    }
    get isTransaction() {
      return call(ops.op_oden_sqlite_in_transaction, databaseState(this).rid);
    }
    open() {
      const state = databaseState(this, false);
      if (state.rid !== undefined) {
        throw invalidState("database is already open");
      }
      const rid = call(
        ops.op_oden_sqlite_open,
        state.path === "" ? ":memory:" : state.path,
        state.options.readOnly,
        !state.options.readOnly,
        true,
      );
      try {
        configure(rid, state.options);
      } catch (cause) {
        ops.op_oden_sqlite_release(rid);
        throw cause;
      }
      state.rid = rid;
      state.generation++;
      resources.register(this, rid, this);
    }
    close() {
      const state = databaseState(this);
      call(ops.op_oden_sqlite_close, state.rid, false);
      resources.unregister(this);
      state.rid = undefined;
      state.generation++;
    }
    [Symbol.dispose]() {
      if (this.isOpen) this.close();
    }
    exec(sql) {
      call(
        ops.op_oden_sqlite_run,
        databaseState(this).rid,
        string(sql, "sql"),
        null,
        false,
      );
    }
    prepare(sql) {
      const info = call(
        ops.op_oden_sqlite_prepare,
        databaseState(this).rid,
        string(sql, "sql"),
        false,
      );
      try {
        return new StatementSync(token, this, info);
      } catch (cause) {
        ops.op_oden_sqlite_release(info.rid);
        throw cause;
      }
    }
    createTagStore(capacity = 1000) {
      databaseState(this);
      if (!Number.isInteger(capacity) || capacity <= 0) {
        throw error(
          "capacity must be a positive integer",
          "ERR_OUT_OF_RANGE",
          RangeError,
        );
      }
      return new SQLTagStore(token, this, capacity);
    }
    location(dbName = "main") {
      string(dbName, "dbName");
      const statement = this.prepare("PRAGMA database_list");
      try {
        statement.setReturnArrays(false);
        const result = statement.all().find((r) => r.name === dbName);
        return result?.file && !result.file.startsWith(":memory:")
          ? result.file
          : null;
      } finally {
        releaseStatement(statement);
      }
    }
    serialize(dbName = "main") {
      const state = databaseState(this);
      if (string(dbName, "dbName") !== "main") {
        unsupported("Serializing a schema other than main");
      }
      return call(ops.op_oden_sqlite_serialize, state.rid);
    }
    deserialize(data, opts = {}) {
      const state = databaseState(this);
      if (!(data instanceof Uint8Array)) {
        throw invalidType("data must be a Uint8Array");
      }
      object(opts, "options");
      if (
        opts.dbName !== undefined && string(opts.dbName, "dbName") !== "main"
      ) unsupported("Deserializing a schema other than main");
      const readonly = opts.readOnly === undefined
        ? false
        : boolean(opts.readOnly, "readOnly");
      const rid = call(ops.op_oden_sqlite_deserialize, data, readonly, true);
      try {
        configure(rid, state.options);
      } catch (cause) {
        ops.op_oden_sqlite_release(rid);
        throw cause;
      }
      call(ops.op_oden_sqlite_close, state.rid, false);
      resources.unregister(this);
      state.rid = rid;
      state.generation++;
      resources.register(this, rid, this);
    }
    enableLoadExtension(allow) {
      databaseState(this);
      if (boolean(allow, "allow")) unsupported("SQLite extension loading");
    }
    loadExtension(filename) {
      databaseState(this);
      string(filename, "path");
      unsupported("DatabaseSync.loadExtension()");
    }
    function(name, opts, callback) {
      databaseState(this);
      string(name, "name");
      if (typeof opts === "function" && callback === undefined) callback = opts;
      else object(opts, "options");
      if (typeof callback !== "function") {
        throw invalidType("function callback must be a function");
      }
      unsupported("JavaScript SQL functions");
    }
    aggregate(name, opts) {
      databaseState(this);
      string(name, "name");
      object(opts, "options");
      unsupported("JavaScript SQL aggregates");
    }
    createSession(opts = {}) {
      databaseState(this);
      object(opts, "options");
      unsupported("SQLite sessions");
    }
    applyChangeset(data) {
      databaseState(this);
      if (!(data instanceof Uint8Array)) {
        throw invalidType("changeset must be a Uint8Array");
      }
      unsupported("SQLite changesets");
    }
    setAuthorizer(callback) {
      databaseState(this);
      if (callback !== null && typeof callback !== "function") {
        throw invalidType("authorizer must be a function or null");
      }
      unsupported("SQLite authorizers");
    }
    get limits() {
      databaseState(this);
      return unsupported("SQLite runtime limits");
    }
    enableDefensive(active) {
      databaseState(this);
      boolean(active, "active");
      unsupported("SQLite defensive-mode controls");
    }
  }
  Object.defineProperty(DatabaseSync.prototype, Symbol.for("sqlite-type"), {
    value: "node:sqlite",
    configurable: true,
  });

  function backup(sourceDb, filename, opts = {}) {
    const state = databaseState(sourceDb);
    const destination = path(filename);
    object(opts, "options");
    if (opts.source !== undefined && string(opts.source, "source") !== "main") {
      unsupported("Backing up a schema other than main");
    }
    if (opts.target !== undefined && string(opts.target, "target") !== "main") {
      unsupported("Backing up to a schema other than main");
    }
    if (
      opts.rate !== undefined &&
      (!Number.isInteger(opts.rate) || opts.rate <= 0 || opts.rate > 4294967295)
    ) {
      throw error(
        "rate must be a positive integer",
        "ERR_OUT_OF_RANGE",
        RangeError,
      );
    }
    if (opts.progress !== undefined && typeof opts.progress !== "function") {
      throw invalidType("progress must be a function");
    }
    return backupExec(state.rid, destination, opts);
  }

  // Execute eagerly like Node's synchronous-native backup wrapper, while runtime
  // errors reject its Promise and argument validation above remains synchronous.
  // deno-lint-ignore require-await
  async function backupExec(rid, destination, opts) {
    let pages;
    try {
      pages = ops.op_oden_sqlite_backup(rid, destination, opts.rate ?? 100);
    } catch (cause) {
      if (cause.name === "AlreadyExists" || cause.code === "EEXIST") {
        unsupported("Backup into an existing destination");
      }
      // Map native SQLite and filesystem errors through the same Node error path.
      call(() => {
        throw cause;
      });
    }
    opts.progress?.({ totalPages: pages, remainingPages: 0 });
    return pages;
  }

  const constants = {
    SQLITE_CHANGESET_OMIT: 0,
    SQLITE_CHANGESET_REPLACE: 1,
    SQLITE_CHANGESET_ABORT: 2,

    SQLITE_CHANGESET_DATA: 1,
    SQLITE_CHANGESET_NOTFOUND: 2,
    SQLITE_CHANGESET_CONFLICT: 3,
    SQLITE_CHANGESET_CONSTRAINT: 4,
    SQLITE_CHANGESET_FOREIGN_KEY: 5,

    SQLITE_OK: 0,
    SQLITE_DENY: 1,
    SQLITE_IGNORE: 2,
    SQLITE_CREATE_INDEX: 1,
    SQLITE_CREATE_TABLE: 2,
    SQLITE_CREATE_TEMP_INDEX: 3,
    SQLITE_CREATE_TEMP_TABLE: 4,
    SQLITE_CREATE_TEMP_TRIGGER: 5,
    SQLITE_CREATE_TEMP_VIEW: 6,
    SQLITE_CREATE_TRIGGER: 7,
    SQLITE_CREATE_VIEW: 8,
    SQLITE_DELETE: 9,
    SQLITE_DROP_INDEX: 10,
    SQLITE_DROP_TABLE: 11,
    SQLITE_DROP_TEMP_INDEX: 12,
    SQLITE_DROP_TEMP_TABLE: 13,
    SQLITE_DROP_TEMP_TRIGGER: 14,
    SQLITE_DROP_TEMP_VIEW: 15,
    SQLITE_DROP_TRIGGER: 16,
    SQLITE_DROP_VIEW: 17,
    SQLITE_INSERT: 18,
    SQLITE_PRAGMA: 19,
    SQLITE_READ: 20,
    SQLITE_SELECT: 21,
    SQLITE_TRANSACTION: 22,
    SQLITE_UPDATE: 23,
    SQLITE_ATTACH: 24,
    SQLITE_DETACH: 25,
    SQLITE_ALTER_TABLE: 26,
    SQLITE_REINDEX: 27,
    SQLITE_ANALYZE: 28,
    SQLITE_CREATE_VTABLE: 29,
    SQLITE_DROP_VTABLE: 30,
    SQLITE_FUNCTION: 31,
    SQLITE_SAVEPOINT: 32,
    SQLITE_COPY: 0,
    SQLITE_RECURSIVE: 33,
  };
  return { DatabaseSync, StatementSync, backup, constants };
})();
