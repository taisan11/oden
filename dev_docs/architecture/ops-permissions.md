# Extensions・Ops・権限チェック

Oden の Web API・`Deno.*` API・組込みモジュールの多くは、JavaScript と Rust の間にある **op** を通して実装されています。ここでは拡張を読む方法と、変更時に壊してはいけない境界を説明します。

## 3 つの重要な要素

- **Extension** — Rust op・JS モジュール・状態初期化をまとめて runtime に登録する単位。`deno_core::extension!` を検索します。
- **Op** — JS から呼ばれる Rust 処理。`#[op2]` で生成されたバインディングを通します。
- **OpState / ResourceTable** — Worker に結びついた状態・サービス・native resource の管理。`ResourceId` が JS 側と Rust 側の handle をつなぎます。

```text
JavaScript の公開 API / ESM
     │  core.ops.op_...(...)
     ▼
deno_core op binding (libs/ops)
     │
     ▼
Rust の #[op2] 関数 ── OpState ── PermissionsContainer
     │
     ├─ ResourceTable (ファイル / DB / ソケット等の handle)
     └─ OS / native ライブラリ / ネットワーク
```

代表的な実装は [`ext/fs/30_fs.js`](https://github.com/taisan11/oden/blob/main/ext/fs/30_fs.js)、[`ext/sqlite/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs)、[`libs/ops/`](https://github.com/taisan11/oden/tree/main/libs/ops) にあります。

## 具体例: `oden:sqlite` がつながる場所

[`ext/sqlite/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) では次の構成が定義されています（概念的な抜粋）。

```rust
deno_core::extension!(
  oden_sqlite,
  ops = [
    op_oden_sqlite_open,
    op_oden_sqlite_close,
    op_oden_sqlite_prepare,
    // ...
  ],
  lazy_loaded_esm = ["oden:sqlite" = "sqlite.js"],
  lazy_loaded_js = ["node_sqlite.js"],
);
```

利用者が `import { Database } from "oden:sqlite"` と書くと、登録済み ESM の JS アダプターが Rust op を呼びます。`node:sqlite` 側の JS アダプターも同じ Rust バックエンドを利用します。これは **JS API と DB エンジンの責務を分けた設計**です。

データベースの handle は `ResourceTable` で管理し、Node 用には非公開 handle table を使う分岐があります。具体的には `NodeResources` と `NODE_RESOURCE` を同ファイルで探してください。

## Worker が extension を登録するタイミング

[`runtime/worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker.rs) は `JsRuntime` を準備し、`deno_web`、`deno_fetch`、`deno_fs`、`deno_net`、`deno_node` などの extension を登録します。続いて `OpState` にサービス・`PermissionsContainer`・feature 設定等を入れ、bootstrap の JS が API を初期化します。

runtime 本体・Web Worker・snapshot では登録方法が異なる場合があります。新しい API を加えるときは「メイン Worker では動くが Web Worker では不明な module になる」状態がないか確認します。

## 権限判定は Rust 側のセキュリティ境界

[`runtime/permissions/`](https://github.com/taisan11/oden/tree/main/runtime/permissions) は読み取り・書き込み・ネットワーク・環境変数・プロセス実行などの許可を扱います。CLI 引数を読むだけでなく、**実際に機密操作をする時点**で判定する必要があります。

例として、SQLite のファイル保存には読み書き権限が必要で、`:memory:` の利用はファイルへの権限を必要としません。[`ext/sqlite/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) の `PermissionIo` はファイル I/O と権限確認を結びつけます。SQL 文が内部でファイルを増やす場合も想定して実装を読みます。

| 機能 | 調べる場所 |
| --- | --- |
| 許可フラグ・権限オプションの解釈 | `cli/args/`、`libs/cli_parser/` |
| 権限そのものの表現と判定 | `runtime/permissions/` |
| OS のファイルへのアクセス | `ext/fs/`、`ext/io/` |
| ネットワークと TLS | `ext/net/`、`ext/fetch/` |
| サブプロセス・環境変数 | `ext/process/`、`ext/os/` |
| 独自 DB の権限付き I/O | `ext/sqlite/lib.rs` |

JS 上で操作ボタンを無効化する、API を隠す、といった処理は権限チェックの代替ではありません。

## Op を追加・変更するときのチェックリスト

1. 対応する `ext/*` crate の JS と Rust の両実装を探す。`core.ops` で呼ばれる関数名を手掛かりにする。
2. `#[op2]` 関数の引数形式（文字列、serde、buffer 等）、同期/非同期、戻り値・エラー変換を確認する。
3. ファイル・ネットワーク・プロセス等に触れる場合、Rust 側に適切な権限チェックを実装する。
4. handle を保持するなら `ResourceTable` への追加・取得・解放、異常時の close、GC との相互作用を確認する。
5. `extension!` の ops・ESM・初期化登録を更新する。Worker / Web Worker / snapshot への組込みも点検する。
6. JS API の宣言ファイルと型解決、Node 互換性に影響する場合はそのアダプターも更新する。
7. 許可・拒否の両ケース、同期/非同期、繰り返し実行・解放処理について unit/spec テストを追加する。

**落とし穴:** op の名前だけを JS に公開しても、型宣言・組込み module・bootstrap・snapshot 側の準備が足りなければ利用できません。拡張を追加した場合は起動失敗や権限昇格が起きないかも回帰テストします。

関連: [Runtime の概説](/architecture/runtime) · [SQLite の内部](/features/sqlite) · [新機能の実装手順](/development/adding-features)
