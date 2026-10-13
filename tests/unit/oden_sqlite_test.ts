// Copyright 2026 the Oden authors. MIT license.
import Database, { constants, SQLiteError, Statement } from "oden:sqlite";
import { Buffer } from "node:buffer";
import { assert, assertEquals, assertThrows } from "./test_util.ts";

Deno.test(function odenSqliteBasic() {
  using db = new Database();
  assert(db instanceof Database);
  assertEquals(db.filename, ":memory:");
  assertEquals(db.inTransaction, false);
  db.exec(
    "CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items(name) VALUES ('first');",
  );
  const insert = db.query("INSERT INTO items(name) VALUES (?)");
  assert(insert instanceof Statement);
  assertEquals(insert.run("日本語"), { changes: 1, lastInsertRowid: 2 });
  const select = db.query<{ id: number; name: string }>(
    "SELECT * FROM items ORDER BY id",
  );
  assertEquals(select.columnNames, ["id", "name"]);
  assertEquals(select.paramsCount, 0);
  assertEquals(select.get(), { id: 1, name: "first" });
  assertEquals(select.values(), [[1, "first"], [2, "日本語"]]);
  assertEquals([...select], [{ id: 1, name: "first" }, {
    id: 2,
    name: "日本語",
  }]);
  assertEquals(select.declaredTypes, ["INTEGER", "TEXT"]);
  assertEquals(select.columnTypes, ["INTEGER", "TEXT"]);
  assertEquals(db.query("SELECT * FROM items WHERE id = 999").get(), null);
});

Deno.test(function odenSqliteBindings() {
  using db = new Database();
  using statement = db.prepare<unknown>("SELECT ? AS a, ? AS b", [1, "two"]);
  assertEquals(statement.get(), { a: 1, b: "two" });
  assertEquals(statement.get([3, 4]), { a: 3, b: 4 });
  assertEquals(statement.get(), { a: 3, b: 4 });
  assertEquals(statement.get(true, null), { a: 1, b: null });
  assertEquals(
    db.query("SELECT $x AS a, $x AS b, :y AS c").get({ $x: "repeat", ":y": 5 }),
    { a: "repeat", b: "repeat", c: 5 },
  );
  assertEquals(db.query("SELECT $x AS a").get({ x: 42 }), { a: null });
  assertThrows(() => statement.get(1));
  assertThrows(() => statement.get(undefined as never, 1), TypeError);
  assertEquals(statement.get(), { a: 1, b: null });
  assertEquals(db.query("SELECT ?3 AS value").get(0, 1, 7), { value: 7 });
  const quoted = db.query("SELECT '?' AS literal, /* $fake */ ? AS actual");
  assertEquals(quoted.paramsCount, 1);
  assertEquals(quoted.get("ok"), { literal: "?", actual: "ok" });
});

Deno.test(function odenSqliteBoundBatches() {
  using db = new Database(":memory:", { strict: true });
  assertEquals(
    db.exec(
      "CREATE TABLE data(value INTEGER); INSERT INTO data VALUES ($value); INSERT INTO data VALUES ($value)",
      { value: 7 },
    ),
    { changes: 1, lastInsertRowid: 2 },
  );
  assertEquals(db.query("SELECT value FROM data").values(), [[7], [7]]);
  db.run("INSERT INTO data VALUES (?); INSERT INTO data VALUES (?)", [9]);
  assertEquals(db.query("SELECT value FROM data").values(), [[7], [7], [9], [
    9,
  ]]);
});

Deno.test(function odenSqliteStrict() {
  using db = new Database(":memory:", { strict: true });
  const statement = db.query("SELECT $name AS name, :age AS age");
  assertEquals(statement.get({ name: "oden", age: 1 }), {
    name: "oden",
    age: 1,
  });
  assertThrows(
    () => statement.get({ name: "missing" }),
    Error,
    "Missing SQLite binding",
  );
  assertEquals(statement.get(), { name: "oden", age: 1 });
  assertEquals(statement.get({ $name: "explicit", ":age": 2 }), {
    name: "explicit",
    age: 2,
  });
});

Deno.test(function odenSqliteBigIntAndBlob() {
  using db = new Database(":memory:", { safeIntegers: true });
  db.run(
    "CREATE TABLE data(id INTEGER PRIMARY KEY, value INTEGER, blob BLOB, real REAL)",
  );
  assertEquals(
    db.run("INSERT INTO data VALUES (?, ?, ?, ?)", [
      9223372036854775807n,
      -9223372036854775808n,
      new Uint8Array([0, 255, 1]),
      1.5,
    ]),
    { changes: 1, lastInsertRowid: 9223372036854775807n },
  );
  assertEquals(db.query("SELECT * FROM data").get(), {
    id: 9223372036854775807n,
    value: -9223372036854775808n,
    blob: new Uint8Array([0, 255, 1]),
    real: 1.5,
  });
  assertThrows(
    () => db.query("SELECT ?").get(9223372036854775808n),
    RangeError,
  );
  const buffer = new Uint8Array([10, 20, 30, 40]);
  assertEquals(db.query("SELECT ? AS b").get(buffer.subarray(1, 3)), {
    b: new Uint8Array([20, 30]),
  });
  assertEquals(db.query("SELECT ? AS b").get(Buffer.from([1, 2])), {
    b: new Uint8Array([1, 2]),
  });
  assertEquals(db.query("SELECT '日本語' AS text, 1 AS n").raw(), [[
    new TextEncoder().encode("日本語"),
    1n,
  ]]);
});

Deno.test(function odenSqliteClassMapping() {
  using db = new Database();
  class Item {
    name!: string;
    constructor() {
      throw new Error("Constructor must not run");
    }
    get upper() {
      return this.name.toUpperCase();
    }
  }
  const statement = db.query("SELECT 'oden' AS name").as(Item);
  const item = statement.get();
  assert(item instanceof Item);
  assertEquals(item.upper, "ODEN");
  assert(statement.all()[0] instanceof Item);
  assert(statement.iterate().next().value instanceof Item);
  assertEquals(
    db.query("SELECT 1 AS '__proto__'").get(),
    JSON.parse('{"__proto__":1}'),
  );
});

Deno.test(function odenSqliteTransactions() {
  using db = new Database();
  db.run("CREATE TABLE items(value TEXT UNIQUE)");
  const insert = db.query("INSERT INTO items VALUES (?)");
  const insertMany = db.transaction(
    function (this: { tag: string }, values: string[]) {
      assert(db.inTransaction);
      for (const value of values) insert.run(value);
      return this.tag;
    },
  );
  assertEquals(insertMany.call({ tag: "done" }, ["a", "b"]), "done");
  assertEquals(insertMany.database, db);
  assertEquals(insertMany.deferred.default, insertMany);
  assertThrows(
    () => insertMany.immediate.call({ tag: "x" }, ["c", "a"]),
    SQLiteError,
  );
  assertEquals(db.query("SELECT value FROM items").values(), [["a"], ["b"]]);
  const inner = db.transaction(() => {
    insert.run("inner");
    throw new Error("rollback inner");
  });
  db.transaction(() => {
    insert.run("outer");
    assertThrows(inner, Error, "rollback inner");
    assert(db.inTransaction);
    insert.run("after");
  }).exclusive();
  assertEquals(db.query("SELECT value FROM items").values(), [["a"], ["b"], [
    "outer",
  ], ["after"]]);
  assertEquals(db.inTransaction, false);
});

Deno.test(function odenSqliteRejectsAsyncTransaction() {
  using db = new Database();
  assertThrows(
    db.transaction(() => Promise.resolve(1)),
    TypeError,
    "synchronous",
  );
  assertEquals(db.inTransaction, false);
});

Deno.test(function odenSqliteCacheAndLifecycle() {
  const db = new Database();
  const first = db.query("SELECT 1 AS value");
  assertEquals(db.query("SELECT 1 AS value"), first);
  first.finalize();
  const replacement = db.query("SELECT 1 AS value");
  assert(replacement !== first);
  assertThrows(() => first.get());
  db.clearQueryCache();
  assert(replacement.isFinalized);
  using prepared = db.prepare("SELECT 2 AS value");
  assertThrows(() => db.close(true), SQLiteError);
  assertEquals(prepared.get(), { value: 2 });
  prepared.finalize();
  db.close(true);
  db.close();
  assertThrows(() => db.query("SELECT 1"), Error, "closed");
  assertThrows(() => replacement.all());
});

Deno.test(function odenSqliteIteratorCleanup() {
  using db = new Database();
  const statement = db.query("SELECT 1 AS value UNION ALL SELECT 2");
  const iterator = statement.iterate();
  assertEquals(iterator.next().value, { value: 1 });
  assertThrows(() => statement.get(), Error, "already iterating");
  iterator.return?.(undefined);
  assertEquals(statement.all(), [{ value: 1 }, { value: 2 }]);
  for (const _ of statement) break;
  assertEquals(statement.get(), { value: 1 });
});

Deno.test(function odenSqliteLruCache() {
  const limit = Database.MAX_QUERY_CACHE_SIZE;
  Database.MAX_QUERY_CACHE_SIZE = 2;
  try {
    using db = new Database();
    const first = db.query("SELECT 1 AS value");
    const second = db.query("SELECT 2 AS value");
    assertEquals(db.query("SELECT 1 AS value"), first);
    db.query("SELECT 3 AS value");
    assertEquals(db.query("SELECT 1 AS value"), first);
    assert(db.query("SELECT 2 AS value") !== second);
    assertEquals(second.get(), { value: 2 });
    db.clearQueryCache();
    assertEquals(second.get(), { value: 2 });
  } finally {
    Database.MAX_QUERY_CACHE_SIZE = limit;
  }
});

Deno.test(function odenSqliteErrorsAndMetadata() {
  using db = new Database();
  db.run("CREATE TABLE items(value INTEGER UNIQUE)");
  db.run("INSERT INTO items VALUES (1)");
  const error = assertThrows(
    () => db.run("INSERT INTO items VALUES (1)"),
    SQLiteError,
  );
  assertEquals(error.code, "SQLITE_CONSTRAINT");
  assertEquals(error.errno, 19);
  assertEquals(error.name, "SQLiteError");
  assertThrows(() => db.prepare("SELEC invalid"), SQLiteError);
  assertThrows(() => db.query(""));
  assertThrows(() => db.query("SELECT 1\0;"), TypeError);
  using statement = db.prepare("SELECT ? AS value, '$x' AS literal", [
    "O'Reilly",
  ]);
  assertEquals(
    statement.toString(),
    "SELECT 'O''Reilly' AS value, '$x' AS literal",
  );
  assertEquals(statement.columnTypes, ["TEXT", "TEXT"]);
  assertThrows(
    () => db.query("INSERT INTO items VALUES (2)").columnTypes,
    Error,
    "read-only",
  );
  assertEquals(db.query("SELECT count(*) AS n FROM items").get(), { n: 1 });
});

Deno.test(function odenSqliteSerialization() {
  using db = Database.open(":memory:", { strict: true, safeIntegers: true });
  db.exec("CREATE TABLE data(id INTEGER, name TEXT, b BLOB)");
  db.run("INSERT INTO data VALUES (?, ?, ?)", [
    9223372036854775807n,
    "日本語",
    new Uint8Array([0, 1, 255]),
  ]);
  const bytes = db.serialize();
  assert(Buffer.isBuffer(bytes));
  assertEquals(bytes.subarray(0, 16).toString(), "SQLite format 3\0");
  using restored = Database.deserialize(bytes, {
    safeIntegers: true,
    strict: true,
  });
  assertEquals(
    restored.query("SELECT * FROM data").values(),
    db.query("SELECT * FROM data").values(),
  );
  restored.run("INSERT INTO data(id) VALUES (2)");
  assertEquals(db.query("SELECT count(*) AS n FROM data").get(), { n: 1n });
  using readonly = Database.deserialize(bytes, true);
  assertThrows(
    () => readonly.run("INSERT INTO data(id) VALUES (3)"),
    SQLiteError,
  );
  using constructed = new Database(bytes);
  assertEquals(constructed.query("SELECT name FROM data").get(), {
    name: "日本語",
  });
  assertThrows(
    () => Database.deserialize(new Uint8Array([0, 1, 2])),
    SQLiteError,
  );
});

Deno.test(function odenSqliteSerializationInTransaction() {
  using db = new Database();
  db.exec("CREATE TABLE data(value INTEGER); INSERT INTO data VALUES (1)");
  db.run("BEGIN");
  db.run("INSERT INTO data VALUES (2)");
  using snapshot = Database.deserialize(db.serialize());
  db.run("ROLLBACK");
  assertEquals(snapshot.query("SELECT value FROM data").values(), [[1], [2]]);
  assertEquals(db.query("SELECT value FROM data").values(), [[1]]);
});

Deno.test(function odenSqliteEmptySerialization() {
  using db = new Database();
  using restored = Database.deserialize(db.serialize());
  assertEquals(restored.query("SELECT name FROM sqlite_schema").all(), []);
});

Deno.test(
  { permissions: { read: true, write: true } },
  function odenSqlitePersistenceAndReadonly() {
    const dir = Deno.makeTempDirSync();
    const path = `${dir}/database.sqlite`;
    try {
      const db = new Database(path);
      db.run("CREATE TABLE items(value TEXT)");
      db.run("INSERT INTO items VALUES (?)", "persisted");
      db.close();
      try {
        Deno.removeSync(`${path}-wal`);
      } catch (error) {
        if (!(error instanceof Deno.errors.NotFound)) throw error;
      }
      using writable = new Database(path, { readwrite: true });
      assertEquals(writable.query("SELECT * FROM items").get(), {
        value: "persisted",
      });
      using reopened = new Database(path, constants.SQLITE_OPEN_READONLY);
      assertEquals(reopened.query("SELECT * FROM items").get(), {
        value: "persisted",
      });
      assertThrows(
        () => reopened.run("INSERT INTO items VALUES ('no')"),
        SQLiteError,
      );
      assertThrows(
        () => new Database(`${dir}/absent.sqlite`, { readwrite: true }),
        SQLiteError,
      );
    } finally {
      Deno.removeSync(dir, { recursive: true });
    }
  },
);

Deno.test(
  { permissions: { read: false, write: false } },
  function odenSqlitePermissions() {
    assertThrows(
      () => new Database("/tmp/oden-sqlite-denied.db"),
      Deno.errors.NotCapable,
    );
    using db = new Database();
    assertEquals(db.query("SELECT 1 AS value").get(), { value: 1 });
    assertThrows(
      () => db.run("ATTACH '/tmp/oden-sqlite-denied.db' AS other"),
      SQLiteError,
    );
    assertThrows(
      () => db.query("SELECT load_extension('/tmp/denied.so')").get(),
      SQLiteError,
    );
  },
);

Deno.test(function odenSqliteOptionsAndUnsupportedApis() {
  assertThrows(() => new Database(":memory:", { readonly: true }));
  assertThrows(
    () => new Database(":memory:", { readOnly: true } as never),
    TypeError,
  );
  using db = new Database("", { create: true });
  assertEquals(db.filename, "");
  assertThrows(() => db.loadExtension("anything"), Error, "Turso");
  assertThrows(() => db.fileControl(1, 0), Error, "Turso");
  assertThrows(() => Database.setCustomSQLite("anything"), Error, "Turso");
});

Deno.test(function odenSqliteDefaultAndConflictingOptions() {
  for (
    const options of [
      {},
      { readonly: false },
      { readwrite: false, create: false },
      { readonly: false, readwrite: false, create: false },
      { strict: false, safeIntegers: false },
    ]
  ) {
    using db = new Database(":memory:", options);
    db.run("CREATE TABLE data(value INTEGER); INSERT INTO data VALUES (42)");
    assertEquals(db.query("SELECT value FROM data").get(), { value: 42 });
  }
  for (
    const options of [
      { readonly: true, create: true },
      { readonly: true, readwrite: true },
    ]
  ) {
    assertThrows(
      () => new Database(":memory:", options),
      TypeError,
      "conflicts",
    );
  }
});

Deno.test(
  { permissions: { read: true, write: true } },
  function odenSqliteFilenameWhitespaceAndDefaultCreation() {
    const directory = Deno.makeTempDirSync();
    try {
      for (const name of [" leading.db", "trailing.db ", " "]) {
        const filename = `${directory}/${name}`;
        {
          using db = new Database(filename, {});
          assertEquals(db.filename, filename);
          db.run("CREATE TABLE data(value TEXT)");
          db.run("INSERT INTO data VALUES (?)", name);
        }
        assert(Deno.statSync(filename).isFile);
        using restored = new Database(filename, { readonly: true });
        assertEquals(restored.query("SELECT value FROM data").get(), {
          value: name,
        });
        assertThrows(() => restored.run("INSERT INTO data VALUES ('write')"));
      }
      assertThrows(
        () => Deno.statSync(`${directory}/leading.db`),
        Deno.errors.NotFound,
      );
      assertThrows(
        () => Deno.statSync(`${directory}/trailing.db`),
        Deno.errors.NotFound,
      );
    } finally {
      Deno.removeSync(directory, { recursive: true });
    }
  },
);

Deno.test(async function odenSqliteWorker() {
  const code = `import { Database } from "oden:sqlite";
    using db = new Database(":memory:", { safeIntegers: true });
    postMessage(db.query("SELECT 42 AS value").get());`;
  const worker = new Worker(
    `data:application/javascript,${encodeURIComponent(code)}`,
    { type: "module" },
  );
  try {
    const result = await new Promise((resolve, reject) => {
      worker.onmessage = (event) => resolve(event.data);
      worker.onerror = (event) => {
        event.preventDefault();
        reject(new Error(event.message));
      };
    });
    assertEquals(result, { value: 42n });
  } finally {
    worker.terminate();
  }
});
