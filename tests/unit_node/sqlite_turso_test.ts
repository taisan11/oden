// Copyright 2026 the Oden authors. MIT license.
import sqlite, { backup, DatabaseSync, StatementSync } from "node:sqlite";
import { createRequire } from "node:module";
import { Database as OdenDatabase } from "oden:sqlite";
import { assert, assertEquals, assertThrows } from "../unit/test_util.ts";

type TagStore = {
  size: number;
  capacity: number;
  run(strings: TemplateStringsArray, ...values: unknown[]): { changes: number };
  get(
    strings: TemplateStringsArray,
    ...values: unknown[]
  ): Record<string, unknown> | undefined;
  iterate(
    strings: TemplateStringsArray,
    ...values: unknown[]
  ): IterableIterator<Record<string, unknown>>;
  clear(): void;
};
type Extras = {
  createTagStore(capacity?: number): TagStore;
  serialize(): Uint8Array;
  deserialize(bytes: Uint8Array, options?: { readOnly?: boolean }): void;
  setAuthorizer(callback: (() => number) | null): void;
  readonly limits: unknown;
  enableDefensive(enabled: boolean): void;
};
const extended = (db: DatabaseSync) => db as DatabaseSync & Extras;
const expected = (record: Record<string, unknown>) => ({
  ...record,
  __proto__: null,
});

Deno.test("[node/sqlite Turso] ESM and CommonJS share exports", () => {
  const require = createRequire(import.meta.url);
  assertEquals(require("node:sqlite").DatabaseSync, DatabaseSync);
  assertEquals(sqlite.DatabaseSync, DatabaseSync);
  // @ts-ignore testing the intentionally illegal constructor
  assertThrows(() => new StatementSync(), TypeError);
});

Deno.test(
  { permissions: { run: true } },
  async function sqliteAdaptersUseProtectedIntrinsics() {
    const code = `import { DatabaseSync } from "node:sqlite";
import { Database } from "oden:sqlite";
const changes = [];
const define = Object.defineProperty;
function poison(prototype, key) {
  changes.push([prototype, key, Object.getOwnPropertyDescriptor(prototype, key)]);
  define(prototype, key, { configurable: true, value() { throw new Error("poisoned intrinsic"); } });
}
poison(Array.prototype, "map");
poison(Array.prototype, "slice");
poison(String.prototype, "includes");
poison(String.prototype, "startsWith");
poison(Map.prototype, "get");
poison(Map.prototype, "set");
poison(Map.prototype, "entries");
poison(WeakMap.prototype, "get");
poison(WeakMap.prototype, "set");
poison(Array.prototype, Symbol.iterator);
try {
  using node = new DatabaseSync(":memory:");
  using oden = new Database(":memory:", {});
  const tag = node.createTagStore(1);
  if (tag.get\`SELECT \${42} AS value\`.value !== 42) throw new Error("node binding failed");
  tag.get\`SELECT \${1} AS other\`;
  tag.clear();
  if (oden.query("SELECT ? AS value").get(42).value !== 42) throw new Error("oden binding failed");
  oden.clearQueryCache();
} finally {
  for (let i = changes.length - 1; i >= 0; i--) {
    const change = changes[i];
    define(change[0], change[1], change[2]);
  }
}`;
    const { code: status, stderr } = await new Deno.Command(Deno.execPath(), {
      args: ["eval", code],
      stderr: "piped",
    }).output();
    assertEquals(status, 0, new TextDecoder().decode(stderr));
  },
);

Deno.test("[node/sqlite Turso] connection lifecycle invalidates old statements", () => {
  const db = new DatabaseSync(":memory:", { open: false });
  assertEquals(db.isOpen, false);
  assertThrows(() => db.exec("SELECT 1"));
  db.open();
  assertThrows(() => db.open());
  const statement = db.prepare("SELECT 1 AS value");
  assertEquals(statement.get(), expected({ value: 1 }));
  db.close();
  assertThrows(() => statement.all());
  db.open();
  assertThrows(() => statement.get());
  assertEquals(db.prepare("SELECT 2 AS value").get(), expected({ value: 2 }));
  db.close();
});

Deno.test("[node/sqlite Turso] mixed bindings and fresh bindings on each call", () => {
  using db = new DatabaseSync(":memory:");
  const statement = db.prepare("SELECT ? AS a, $named AS b, ? AS c");
  assertEquals(
    statement.get({ named: "日本語" }, 1, 2),
    expected({ a: 1, b: "日本語", c: 2 }),
  );
  assertEquals(statement.get(), expected({ a: null, b: null, c: null }));
  assertThrows(() => statement.get({ missing: 1 }));
  statement.setAllowUnknownNamedParameters(true);
  assertEquals(
    statement.get({ named: "ok", missing: 1 }, 3),
    expected({ a: 3, b: "ok", c: null }),
  );
  statement.setAllowBareNamedParameters(false);
  statement.setAllowUnknownNamedParameters(false);
  assertThrows(() => statement.get({ named: "invalid" }));
  assertEquals(
    statement.get({ $named: "explicit" }),
    expected({ a: null, b: "explicit", c: null }),
  );
});

Deno.test("[node/sqlite Turso] named-parameter ambiguity and binding type checks", () => {
  using db = new DatabaseSync(":memory:");
  const statement = db.prepare("SELECT $x AS a, :x AS b");
  assertThrows(() => statement.get({ x: 1 }));
  assertEquals(statement.get({ $x: 1, ":x": 2 }), expected({ a: 1, b: 2 }));
  const positional = db.prepare("SELECT ? AS value");
  assertThrows(() => positional.get(true as never), TypeError);
  assertThrows(() => positional.get(undefined as never), TypeError);
  assertThrows(() => positional.get(9223372036854775808n), RangeError);
  assertEquals(positional.get(null), expected({ value: null }));
});

Deno.test("[node/sqlite Turso] bigint reading and mutation metadata", () => {
  using db = new DatabaseSync(":memory:");
  db.exec("CREATE TABLE data(id INTEGER PRIMARY KEY, value INTEGER)");
  const insert = db.prepare("INSERT INTO data(value) VALUES (?)");
  assertEquals(insert.run(9223372036854775807n), {
    changes: 1,
    lastInsertRowid: 1,
  });
  const select = db.prepare("SELECT value FROM data");
  assertThrows(() => select.get(), RangeError);
  select.setReadBigInts(true);
  assertEquals(select.get(), expected({ value: 9223372036854775807n }));
  insert.setReadBigInts(true);
  assertEquals(insert.run(-9223372036854775808n), {
    changes: 1n,
    lastInsertRowid: 2n,
  });
});

Deno.test("[node/sqlite Turso] native column metadata and expanded SQL", () => {
  using db = new DatabaseSync(":memory:");
  db.exec("CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT)");
  db.prepare("INSERT INTO users VALUES (?, ?)").run(1, "O'Reilly");
  const statement = db.prepare(
    "SELECT id, name AS label, length(name) AS n FROM users WHERE id = ?",
  );
  assertEquals(statement.columns(), [
    {
      name: "id",
      column: "id",
      table: "users",
      database: "main",
      type: "INTEGER",
    },
    {
      name: "label",
      column: "name",
      table: "users",
      database: "main",
      type: "TEXT",
    },
    { name: "n", column: null, table: null, database: null, type: null },
  ]);
  assertEquals(statement.get(1), expected({ id: 1, label: "O'Reilly", n: 8 }));
  assert(statement.expandedSQL.endsWith("id = 1"));
  assertEquals(db.location(), null);
});

Deno.test("[node/sqlite Turso] tag-store cache and stale iterator safety", () => {
  using db = new DatabaseSync(":memory:");
  db.exec(
    "CREATE TABLE data(value INTEGER); INSERT INTO data VALUES (1), (2), (3)",
  );
  const sql = extended(db).createTagStore(2);
  const first = sql
    .iterate`SELECT value FROM data WHERE value >= ${1} ORDER BY value`;
  assertEquals(first.next().value, expected({ value: 1 }));
  const second = sql
    .iterate`SELECT value FROM data WHERE value >= ${2} ORDER BY value`;
  assertThrows(() => first.next(), Error, "invalidated");
  first.return?.();
  assertEquals(second.next().value, expected({ value: 2 }));
  assertEquals(second.next().value, expected({ value: 3 }));
  assertEquals(second.next().done, true);
  assertEquals(sql.size, 1);
  sql.clear();
  assertEquals(sql.size, 0);
});

Deno.test("[node/sqlite Turso] foreign-key enforcement is configurable", () => {
  using enforced = new DatabaseSync(":memory:");
  enforced.exec(
    "CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(id INTEGER REFERENCES parent(id))",
  );
  assertThrows(() => enforced.exec("INSERT INTO child VALUES (10)"));
  using disabled = new DatabaseSync(":memory:", {
    enableForeignKeyConstraints: false,
  });
  disabled.exec(
    "CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(id INTEGER REFERENCES parent(id)); INSERT INTO child VALUES (10)",
  );
  assertEquals(
    disabled.prepare("SELECT id FROM child").get(),
    expected({ id: 10 }),
  );
});

Deno.test("[node/sqlite Turso] restore replaces the connection atomically", () => {
  using db = new DatabaseSync(":memory:");
  db.exec("CREATE TABLE data(value INTEGER); INSERT INTO data VALUES (42)");
  const statement = db.prepare("SELECT * FROM data");
  assertThrows(() => extended(db).deserialize(new Uint8Array([1, 2, 3])));
  assertEquals(statement.get(), expected({ value: 42 }));
  const snapshot = extended(db).serialize();
  extended(db).deserialize(snapshot, { readOnly: true });
  assertThrows(() => statement.get());
  assertEquals(db.prepare("SELECT * FROM data").get(), expected({ value: 42 }));
  assertThrows(() => db.exec("INSERT INTO data VALUES (43)"));
});

Deno.test("[node/sqlite Turso] unsupported C APIs fail explicitly", () => {
  using db = new DatabaseSync(":memory:");
  const actions = [
    () => db.function("user_function", () => 1),
    () => db.aggregate("user_aggregate", { start: 0, step: (acc) => acc }),
    () => db.createSession(),
    () => db.applyChangeset(new Uint8Array()),
    () => db.loadExtension("not-a-library"),
    () => db.enableLoadExtension(true),
    () => extended(db).setAuthorizer(() => 0),
    () => extended(db).limits,
    () => extended(db).enableDefensive(true),
    () => new DatabaseSync(":memory:", { allowExtension: true }),
    () => new DatabaseSync(":memory:", { defensive: false } as never),
    () => new DatabaseSync(":memory:", { limits: {} } as never),
  ];
  for (const action of actions) {
    const error = assertThrows(action) as Error & { code: string };
    assertEquals(error.code, "ERR_SQLITE_UNSUPPORTED");
  }
  db.enableLoadExtension(false);
  assertEquals(db.prepare("SELECT 1 AS value").get(), expected({ value: 1 }));
});

Deno.test(
  { permissions: { read: true, write: true } },
  async function nodeSqliteTursoSharedFileAndBackup() {
    const directory = Deno.makeTempDirSync();
    const filename = `${directory}/shared.db`;
    const copied = `${directory}/copy.db`;
    try {
      using node = new DatabaseSync(filename);
      using oden = new OdenDatabase(filename);
      node.exec("CREATE TABLE data(value TEXT)");
      node.prepare("INSERT INTO data VALUES (?)").run("node");
      assertEquals(oden.query("SELECT value FROM data").get(), {
        value: "node",
      });
      oden.run("INSERT INTO data VALUES (?)", "oden");
      assertEquals(
        node.prepare("SELECT value FROM data ORDER BY rowid").all(),
        [expected({ value: "node" }), expected({ value: "oden" })],
      );
      assertEquals(node.location(), filename);
      await backup(node, copied, { rate: 1 });
      using restored = new DatabaseSync(copied, { readOnly: true });
      assertEquals(
        restored.prepare("SELECT value FROM data ORDER BY rowid").all(),
        node.prepare("SELECT value FROM data ORDER BY rowid").all(),
      );
      const original = Deno.readFileSync(copied);
      try {
        await backup(node, copied);
        throw new Error("Expected existing-target rejection");
      } catch (error) {
        assertEquals(
          (error as Error & { code: string }).code,
          "ERR_SQLITE_UNSUPPORTED",
        );
      }
      assertEquals(Deno.readFileSync(copied), original);
    } finally {
      Deno.removeSync(directory, { recursive: true });
    }
  },
);
