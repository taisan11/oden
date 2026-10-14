# Extensions・Ops・権限境界

Oden の Web API やファイル操作は、JavaScript の公開インターフェースと Rust の処理を **extension / op** で接続します。この境界を理解すると、新規 API の追加や権限チェックの追跡が容易になります。

## JS からネイティブ処理へ

```text
JavaScript API (ext/*/*.js, *.ts)
  ↓ 例: core.ops.op_xxx(...)
deno_core の op ディスパッチ
  ↓  #[op2] でバインディング生成
Rust extension (ext/*/*.rs)
  ↓  OpState / ResourceTable / PermissionsContainer
OS・ネットワーク・DB・外部ライブラリ
```

典型的な extension は [`deno_core::extension!`](https://github.com/taisan11/oden/blob/main/libs/core/extensions.rs) で依存関係・登録する ops・読み込む JS を宣言します。Rust op 自体は [`libs/ops`](https://github.com/taisan11/oden/tree/main/libs/ops) にある `#[op2]` で定義されます。

## ソースにある具体例

[`ext/sqlite/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) は `oden_sqlite` extension に `op_oden_sqlite_open`、`op_oden_sqlite_prepare`、`op_oden_sqlite_run` などを登録し、`sqlite.js` を `oden:sqlite` の ESM として公開します。`node_sqlite.js` は Node API のアダプターです。

[`ext/kv/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/kv/lib.rs) の `deno_kv` extension は `op_kv_database_open` と `op_kv_database_open_native` を分けており、共通の JS API に異なる保存 backend を提供します。

この例で分かるように、**API 名が異なっても Rust backend を共有でき、同じ extension でも複数の backend を選べます**。

## OpState と ResourceTable

`OpState` は op 側が必要とする依存・共有状態を保持します。長く生存するファイル・DB・ストリーム等は resource ID（`ResourceId`）から Rust オブジェクトを管理することがあります。

- `ext/kv/lib.rs` の `DatabaseResource` は close 時に backend とキャンセルハンドルを終了させます。
- `ext/sqlite/lib.rs` は Bun 風 API 用のリソースと、Node の GC 管理オブジェクトに対応した private handle table を分けています。
- すべての extension が同じリソース管理方式を使うわけではありません。

**実装時の確認点**は、リソースの解放、失敗時の取り消し、非同期 op のキャンセル、worker 終了時の後始末です。

## 権限は JS の表示制御ではない

[`runtime/permissions/`](https://github.com/taisan11/oden/tree/main/runtime/permissions) の `PermissionsContainer` は、ファイル、ネットワーク、環境変数、コマンド実行などのアクセス判断を扱います。起動時の構成は `CliFactory::root_permissions_container()` にも関係します。

```text
CLI flags / permission configuration
           ↓
root PermissionsContainer
           ↓
MainWorker / WebWorker が依存を受け取る
           ↓
機密操作を行う Rust op で check_*
           ↓
許可 → 処理  /  拒否 → エラー
```

例えば SQLite のファイル DB は Rust 側で [`PermissionsContainer` を参照](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs)します。`ATTACH` には通常の単純なファイルオープンとは別の権限制約があります。新規 op で JS 側に `if` を書くだけでは、信頼できる権限境界になりません。

## 新しい API を追加するチェックリスト

1. 既存の最も近い extension を見つけ、公開 JS API と Rust op を分ける
2. `#[op2]` の引数・戻り値・エラー変換、同期 / 非同期を決める
3. `extension!` に op と JS を登録し、必要な依存を `OpState` に置く
4. MainWorker と WebWorker、snapshot の extension 登録先を確認する
5. OS 資源・ファイル・ネットワーク等へのアクセス直前に Rust 側で権限確認する
6. resource の close、エラー時の rollback・キャンセルを考慮する
7. JS/TS の宣言、型検査・LSP・モジュール解決との接続を確認する
8. 成功系だけでなく、権限拒否・不正入力・Worker 内利用のテストを追加する


## `oden_sqlite` を例に op 登録を追う

[`ext/sqlite/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) の登録は、概念的に次の形です。

```rust
deno_core::extension!(
  oden_sqlite,
  ops = [
    op_oden_sqlite_open,
    op_oden_sqlite_close,
    op_oden_sqlite_prepare,
    op_oden_sqlite_run,
    // 他の op はソースを参照
  ],
  lazy_loaded_esm = ["oden:sqlite" = "sqlite.js"],
  lazy_loaded_js = ["node_sqlite.js"],
);
```

Rust の op 定義では `#[op2]` を使用し、文字列・バッファ・serde 対応値などを JS との境界で変換します。`oden:sqlite` と `node:sqlite` は JS adapter が異なっても同じ Turso backend の op を呼びます。

また、同ファイルの `PermissionIo` は Turso が行うファイル操作に許可判定を適用します。`:memory:` とファイル DB を分けて考え、WAL 等の補助ファイルにも権限チェックが働くか確認します。

### 新しい native resource を持つ API の注意点

- **所有権**: JS オブジェクトを破棄したあとも ResourceTable に handle が残らないか。
- **繰り返し利用**: すでに閉じた ID や別 Worker の ID を誤って受け付けないか。
- **失敗時**: open や prepare の途中で失敗した場合、作成済みの resource を回収できるか。
- **Async**: Promise の解決・拒否と Rust 側のキャンセルが対応しているか。
- **型と公開範囲**: extension を登録しただけでは、TS の宣言・Node adapter・LSP が追従するとは限らない。

本リポジトリの拡張手順を段階的に整理した [新機能の実装手順](/development/adding-features) も参照してください。

## 調査で混同しやすいこと

| 誤解 | 実装上の区別 |
| --- | --- |
| extension を宣言すれば常に全 Worker で使える | Worker 作成・snapshot 構成側の登録が必要 |
| JS API が拒否すれば安全 | Rust op 側の権限確認が必要 |
| リソースはすべて同じテーブル | extension 固有のハンドル管理もある |
| Async op と Tokio タスクは同じもの | JS 側の Promise / event loop と Rust の実行タスクを区別する |

関連: [実行ライフサイクル](/architecture/execution) / [Runtime](/architecture/runtime) / [Turso SQLite](/features/sqlite)
