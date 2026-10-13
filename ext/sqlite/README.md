# oden:sqlite

The shared, synchronous **Turso Rust database engine** (`turso_core` 0.8.3)
behind Oden's `oden:sqlite` and `node:sqlite` modules. `oden:sqlite` follows
[`bun:sqlite`](https://bun.sh/reference/bun/sqlite); `node:sqlite` provides
Node's `DatabaseSync`/`StatementSync` API. Both call the same Rust engine,
permission checks, prepared statements, and serialization code. There is no
SQLite C fallback for these modules.

```ts
import { Database } from "oden:sqlite";

using db = new Database("app.sqlite", { create: true, strict: true });
db.exec("CREATE TABLE IF NOT EXISTS users(id INTEGER PRIMARY KEY, name TEXT)");

const insert = db.query("INSERT INTO users(name) VALUES ($name)");
const insertMany = db.transaction((names: string[]) => {
  for (const name of names) insert.run({ name });
});
insertMany(["Alice", "日本語"]);

console.log(
  db.query<{ id: number; name: string }>("SELECT * FROM users").all(),
);
```

Run file databases with `oden run --allow-read --allow-write app.ts`. In-memory
databases (`new Database()`, `""`, `":memory:"`) need no filesystem permissions.
Native file opens, including WAL files, use Oden's permission checks. `ATTACH`
requires `--allow-all`. SQL extension loading is disabled.

## Supported API

| API                                                               | Behavior                                                                                                                                                          |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Database`, `Database.open`                                       | Memory/file databases, `readonly`, `readwrite`, `create`, `strict`, `safeIntegers`; common numeric open flags                                                     |
| `prepare`, `query`                                                | Prepared statements; `query` uses a bounded LRU cache (`MAX_QUERY_CACHE_SIZE`, default 20)                                                                        |
| `run`, `exec`                                                     | Execute SQL batches; return `{ changes, lastInsertRowid }`; bind parameters to each statement that has parameters                                                 |
| `transaction`                                                     | Synchronous callback, rollback on exceptions, nested savepoints, `.default`, `.deferred`, `.immediate`, `.exclusive`, `.database`; preserves `this` and arguments |
| `serialize`, `deserialize`, binary constructor                    | SQLite-format `Buffer` snapshots of `main`, including WAL and uncommitted changes; restore into an independent memory database                                    |
| `close`, `clearQueryCache`, `Symbol.dispose`                      | Explicit resource cleanup; `close(true)` reports unfinalized `prepare()` statements                                                                               |
| `Statement.get`, `all`, `values`, `raw`, `run`                    | Objects, arrays, raw UTF-8 text bytes, or change counts; `get()` returns `null` when no row exists                                                                |
| `iterate`, `Symbol.iterator`                                      | Incremental synchronous iteration with cursor cleanup on early return                                                                                             |
| `Statement.as`                                                    | Rows inherit from a class prototype; constructors are not called                                                                                                  |
| `columnNames`, `paramsCount`, `columnTypes`, `declaredTypes`      | Native statement metadata; actual types from the first row, declared types after execution                                                                        |
| `toString`, `toJSON`, `finalize`, `isFinalized`, `Symbol.dispose` | Expanded SQL and idempotent statement disposal                                                                                                                    |
| `SQLiteError`, `constants`, default export                        | Error class and SQLite flags; default export is `Database`                                                                                                        |

Bindings accept strings, numbers, booleans, null, signed 64-bit BigInts and
typed arrays (including Node Buffers). BLOBs become `Uint8Array`. With
`safeIntegers`, integer results and `lastInsertRowid` become BigInts. Positional
bindings accept variadic arguments or an array. Named bindings use their SQL
prefixes; `strict` also accepts bare names and rejects missing named values.
Calls without bindings reuse the previous bindings. No SQL string substitution
is used for execution.

## Compatibility limits

This provides Bun's JavaScript API on Turso, rather than Bun's SQLite C engine.
Turso's SQL support and concurrency behavior apply; it is not a promise of full
SQLite or Bun behavioral equivalence.

- `loadExtension`, `fileControl`, and `setCustomSQLite` throw explicit
  unsupported errors. The exported `native` object provides backend/version
  information; it does not expose SQLite C bindings. Handles are Oden resource
  IDs, not C pointers.
- SQLite C VFS/URI/shared-cache open flags and `SQLITE_PREPARE_NO_VTAB` are
  unsupported. Persistent/normalize prepare hints are accepted without changing
  Turso's preparation behavior. Serialization currently supports `main` only.
- SQLite error codes are mapped from Turso errors. Parser byte offsets and some
  extended constraint codes are unavailable (`byteOffset` is `-1`).
- Transaction callbacks must finish synchronously. Returning a Promise throws
  and rolls back; asynchronous callbacks cannot be kept inside this synchronous
  transaction API.
- Native paths must not be symbolic links. WAL companion files also need
  filesystem permissions. Serialization may allocate an entire database image.

## node:sqlite

```ts
import { DatabaseSync } from "node:sqlite";

using db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE data(id INTEGER PRIMARY KEY, value TEXT)");
db.prepare("INSERT INTO data(value) VALUES (?)").run("hello");
console.log(db.prepare("SELECT * FROM data").all());
```

The Node adapter preserves:

- `DatabaseSync` construction, delayed `open`, `close`, disposal, `isOpen`,
  `isTransaction`, `location`, SQL `exec`, and `prepare`.
- Named and positional bindings in the same call; bare-name and unknown-name
  controls. Bindings reset between calls. Rows have null prototypes; an empty
  `get()` returns `undefined`. Integer reads reject unsafe JS numbers unless
  `setReadBigInts(true)` is enabled. BLOB values are `Uint8Array`.
- `StatementSync` `get`/`all`/`run`/`iterate`, `columns`, `sourceSQL`,
  `expandedSQL`, `setReadBigInts`, and array-return controls. Iterators hold
  their statement and expose iterator helpers; rebinding invalidates old cursors
  safely.
- `createTagStore`, bounded statement caching, and parameterized SQL templates.
- Foreign-key, double-quoted-string, readonly, bigint, and busy-timeout options.
- Oden's existing `serialize`/`deserialize` methods. Invalid images are rejected
  before replacing the original connection. Restoration invalidates old
  statements.
- ESM/CommonJS imports and workers. Node objects use a private GC-managed handle
  table, maintaining the existing Node API's resource-sanitizer behavior.

`backup()` returns a Promise and copies a consistent `main` image into a **new
destination file**. Existing destinations and other schemas are unsupported; no
existing file is truncated. `rate` controls write-batch size. Copying is
synchronous inside the Promise-producing wrapper, and progress is reported at
completion.

`function`, `aggregate`, `createSession`, `applyChangeset`, `loadExtension`,
enabling extension loading, `setAuthorizer`, `limits`, and defensive-mode
controls throw `ERR_SQLITE_UNSUPPORTED`. These SQLite C API features have not
been bridged to this Turso backend. `allowExtension: true`, constructor
`limits`, and disabling defensive mode are also rejected. Standard SQL built-in
functions and aggregates are provided by Turso.

SQLite SQL support follows Turso, including its limitations on PRAGMAs, virtual
tables and experimental SQL features. Native column metadata currently reports
database origins for `main`/`temp`; other schema origins may be `null`. Runtime
SQL errors expose Node's `ERR_SQLITE_ERROR`, `errcode`, and `errstr`, with
messages from Turso.

The extension is registered in main workers, web workers, and snapshot creation.
The CLI type checker and language server resolve `oden:sqlite` to
[`sqlite.d.ts`](sqlite.d.ts). `oden types` and `oden sync-types` also include
the module declaration, and the bundler preserves `oden:sqlite` as a runtime
import.

## Validation

```sh
cargo check -p oden_sqlite --features deno_core/v8
cargo build -p deno --bin oden
target/debug/oden test --config tests/config/oden.json --allow-all tests/unit/oden_sqlite_test.ts
target/debug/oden test --config tests/config/oden.json --allow-all --no-check tests/unit_node/sqlite_test.ts tests/unit_node/sqlite_turso_test.ts
```
