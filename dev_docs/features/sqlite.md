# Turso SQLite の実装と互換性

Oden の `oden:sqlite` と `node:sqlite` は、同じ [`ext/sqlite`](https://github.com/taisan11/oden/tree/main/ext/sqlite) crate にある **`turso_core` ベースの同期 Rust backend** を使用します。SQLite C ライブラリへのフォールバックはありません。

> [!IMPORTANT]
> `oden:sqlite` は Bun に似た JavaScript API、`node:sqlite` は Node API を提供するアダプターです。どちらも SQLite / Bun / Node の全挙動の互換性を保証するものではありません。

## 共通 Rust 処理までの経路

```text
import "oden:sqlite"             import "node:sqlite"
        │                               │
  ext/sqlite/sqlite.js       ext/sqlite/node_sqlite.js
        └───────────────┬───────────────┘
                        ↓
          ext/sqlite/lib.rs (#[op2])
      open / prepare / step / reset / run
              ↓ PermissionsContainer
       turso_core (Rust SQLite engine)
              ↓
         memory / DB files + WAL
```

`ext/sqlite/lib.rs` の `deno_core::extension!` は `op_oden_sqlite_open`、`op_oden_sqlite_prepare`、`op_oden_sqlite_run`、`op_oden_sqlite_serialize` 等を登録します。Bun 風と Node 風では、戻り値・引数名の処理・リソース管理に差がありますが、SQL エンジン自体は共通です。

## `oden:sqlite`（Bun 風）

```ts
import { Database } from "oden:sqlite";

using db = new Database(":memory:");
db.exec("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)");
const insert = db.query("INSERT INTO items (name) VALUES (?)");
db.transaction(() => {
  insert.run("one");
  insert.run("two");
})();
console.log(db.query("SELECT * FROM items").all());
```

`Database`、`query` / `prepare`、`run`、`all`、`get`、`iterate`、同期 `transaction`、`serialize` / `deserialize` を用意します。`query` は上限付き LRU cache（既定で最大 20 件）を利用し、`prepare` とはキャッシュの扱いが異なります。

## `node:sqlite`（Node 風）

```ts
import { DatabaseSync } from "node:sqlite";

using db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE items (value TEXT)");
db.prepare("INSERT INTO items VALUES (?)").run("hello");
console.log(db.prepare("SELECT value FROM items").get());
```

Node adapter は `DatabaseSync` / `StatementSync` に加え、`createTagStore` や一部の `backup` 処理を実装します。`get()` で行がなければ `undefined`、Bun 風の `oden:sqlite` では `null` になるなど、違いに注意してください。

## 権限・トランザクション

| 対象 | 動作 |
| --- | --- |
| `:memory:` / 空の DB 名 | ファイル権限不要 |
| ファイル DB / WAL | Rust 側で読み書き権限を確認 |
| `ATTACH` | `--allow-all` を要求 |
| JS callback の `transaction` | **同期関数のみ**。Promise を返すと rollback して失敗 |
| SQL extension load | 明示的に非対応 |

```sh
# file.sqlite を読み書きする例
oden run --allow-read --allow-write app.ts
```

`safeIntegers` や `setReadBigInts(true)` の有無は、64-bit integer の JS 表現にも影響します。BLOB は `Uint8Array` として扱われます。

## 主な非互換部分

- `loadExtension`、C extension・独自 VFS・一部 SQLite C API フラグは使用不可です。
- Node 側の `function`、`aggregate`、`createSession`、`applyChangeset`、`setAuthorizer` などは明示的に非対応です。
- `node:sqlite` の `backup()` は **新規ファイルへの `main` のバックアップのみ**で、Promise を返しますがコピー処理自体は同期的です。
- `serialize` は `main` schema を対象とします。
- SQL 文や PRAGMA・仮想テーブル等の対応範囲は Turso の実装に依存します。
- `oden:sqlite` / `node:sqlite` の DB と **`Deno.openKv` の Deno KV ファイル**は API も用途も別です。

## 型と Worker、テスト

`oden:sqlite` は [`ext/sqlite/sqlite.d.ts`](https://github.com/taisan11/oden/blob/main/ext/sqlite/sqlite.d.ts) で型を提供し、CLI の型検査・LSP・bundler にも統合されています。extension は MainWorker・WebWorker・snapshot に登録されています。

```sh
cargo check -p oden_sqlite --features deno_core/v8
cargo build --locked -p deno --bin oden
./target/debug/oden test --config tests/config/oden.json --allow-all tests/unit/oden_sqlite_test.ts
./target/debug/oden test --config tests/config/oden.json --allow-all --no-check tests/unit_node/sqlite_test.ts tests/unit_node/sqlite_turso_test.ts
```

追加・変更にあたっては JS adapter と `ext/sqlite/lib.rs` の両方をテストしてください。詳細な対応一覧は [`ext/sqlite/README.md`](https://github.com/taisan11/oden/blob/main/ext/sqlite/README.md) が一次資料です。
