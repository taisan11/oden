# Turso SQLite の実装を読む

Oden の `oden:sqlite` と `node:sqlite` は、[`ext/sqlite/`](https://github.com/taisan11/oden/tree/main/ext/sqlite) にある **共通の Turso Rust バックエンド**を使います。2 つの名前空間の API が似ていても、JavaScript 側のアダプターは別々です。

## 層ごとの役割

```text
          JS/TS application
             │        │
  import "oden:sqlite"  import "node:sqlite"
             │        │
      sqlite.js         node_sqlite.js
       (Bun 寄り)        (Node API)
             └────┬────┘
                  ▼
         ext/sqlite/lib.rs
       (op_oden_sqlite_* 群)
                  │
       permission-aware I/O
                  │
                  ▼
               turso_core
           (Rust SQL backend)
```

| 実装 | 見るべき責務 |
| --- | --- |
| [`ext/sqlite/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) | `extension!` 登録、op、接続・statement、権限付き I/O、serialize |
| [`ext/sqlite/sqlite.js`](https://github.com/taisan11/oden/blob/main/ext/sqlite/sqlite.js) | `Database`、`Statement`、transaction などの Bun 風 JS API |
| [`ext/sqlite/node_sqlite.js`](https://github.com/taisan11/oden/blob/main/ext/sqlite/node_sqlite.js) | Node の `DatabaseSync`、`StatementSync` 等への変換 |
| [`ext/sqlite/sqlite.d.ts`](https://github.com/taisan11/oden/blob/main/ext/sqlite/sqlite.d.ts) | `oden:sqlite` の TS API 定義 |
| [`ext/sqlite/README.md`](https://github.com/taisan11/oden/blob/main/ext/sqlite/README.md) | 現行 API と既知の互換性制限 |

## `oden:sqlite` の基本使用例

```ts
import { Database } from "oden:sqlite";

using db = new Database(":memory:");
db.exec("CREATE TABLE messages(id INTEGER PRIMARY KEY, text TEXT)");
const insert = db.query("INSERT INTO messages(text) VALUES (?)");
insert.run("こんにちは");
console.log(db.query("SELECT * FROM messages").all());
```

ファイルを開く場合は、たとえば `oden run --allow-read --allow-write app.ts` のように権限を渡します。`:memory:` ではファイルへの許可を必要としません。操作は同期 API です。

## `node:sqlite` の使用例

```ts
import { DatabaseSync } from "node:sqlite";

using db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE items(value TEXT)");
db.prepare("INSERT INTO items VALUES (?)").run("hello");
console.log(db.prepare("SELECT * FROM items").all());
```

Node 互換アダプターは既存の `DatabaseSync` / `StatementSync` の形を提供し、行の返却形式、数値・BLOB の扱い、statement の寿命、SQL template store などを独自に調整します。「同じ Turso を呼ぶ」ことと「JS の戻り値・例外が同じ」ことは別です。

## Connection と Prepared Statement の寿命

1. JS のコンストラクターが database の open op を呼ぶ。
2. Rust 側で Turso の connection と I/O を準備し、handle を生成する。
3. `query` / `prepare` では statement handle を作成する。
4. `run`、`get`、`all`、`iterate` は bind・step・reset 等の op を組み合わせる。
5. `close` や `Symbol.dispose` で resource を解放する。古い statement を使用した場合は正しく失敗する必要がある。

[`ext/sqlite/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) の `op_oden_sqlite_open`、`op_oden_sqlite_prepare`、`op_oden_sqlite_run`、`op_oden_sqlite_close` を追うと一連の処理を確認できます。Node オブジェクトは GC 管理用の private table を持つため、`ResourceId` の扱いをそのまま公開 API と同一視しないでください。

## セキュリティとファイル I/O

[`PermissionIo`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) は Turso が発生させるファイル I/O を Oden の許可判定と接続します。DB 本体だけでなく WAL などの追加ファイルも対象です。ネイティブパスのシンボリックリンクは禁止されています。

`ATTACH` は現行実装で `--allow-all` を必要とします。SQLite 拡張を任意にロードする API は提供されません。**アプリ側で DB ファイルを開く許可を独自に判定してから op を呼ぶだけでは不十分**で、最終的なファイル操作の Rust 側チェックを維持してください。

## 互換性の境界

| 項目 | 現状の整理 |
| --- | --- |
| エンジン | SQLite C ライブラリではなく `turso_core` |
| `oden:sqlite` | Bun に近い同期 `Database` API |
| `node:sqlite` | Node の `DatabaseSync` / `StatementSync` の対応範囲を実装 |
| 拡張ロード・C VFS API | 未対応。C API の完全互換ではない |
| シリアライズ | `main` schema のみを対象とする制約 |
| トランザクション | Bun 型コールバックは同期完了が必要。Promise は拒否・rollback |
| `node:sqlite` の `backup` | Promise を返すが、説明上は内部コピーが同期に行われる。既存の宛先を上書きしない |
| SQL 構文 | Turso の実装状況に依存。すべての SQLite PRAGMA・仮想テーブルを保証しない |

**重要:** 正確な制限・例外コード・BigInt の扱いは [`ext/sqlite/README.md`](https://github.com/taisan11/oden/blob/main/ext/sqlite/README.md) が一次情報です。Node の API で動いたことだけで Bun 風 API に同じ契約があるとは判断しないでください。

## 変更時の検証

```sh
cargo check -p oden_sqlite --features deno_core/v8
cargo build -p deno --bin oden
target/debug/oden test --config tests/config/oden.json --allow-all tests/unit/oden_sqlite_test.ts
target/debug/oden test --config tests/config/oden.json --allow-all --no-check tests/unit_node/sqlite_test.ts tests/unit_node/sqlite_turso_test.ts
```

このコマンドは [SQLite README](https://github.com/taisan11/oden/blob/main/ext/sqlite/README.md) にある検証例です。権限まわりを変更した場合は `--allow-all` だけでなく、アクセス拒否・読み取りのみ・メモリー DB の各ケースも追加して確認してください。

関連: [Ops・権限](/architecture/ops-permissions) · [独自機能の概説](/features/oden)
