# Runtime・Ops・権限

## runtime の組み立て

[`runtime/worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker.rs)
は `JsRuntime` と拡張機能を組み合わせる中心的な実装です。
[`runtime/web_worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/web_worker.rs)
には Web Worker 向けの別経路があります。
[`runtime/lib.rs`](https://github.com/taisan11/oden/blob/main/runtime/lib.rs)
は runtime crate の公開モジュール群を示します。

Worker の構築は CLI の [§cli/worker.rs§](https://github.com/taisan11/oden/blob/main/cli/worker.rs) と [§cli/lib/worker.rs§](https://github.com/taisan11/oden/blob/main/cli/lib/worker.rs) を経由します。実際の実行順を読む場合は [実行ライフサイクル](/architecture/execution-flow) から始めてください。

主要な構成要素：

- `libs/core/` の `JsRuntime`：JS 実行・モジュール管理・イベントループ
- `ext/*`：Web API、ファイル、ネットワーク、Node 互換の実装
- `runtime/js/`：worker の bootstrap に利用する JS
- `runtime/ops/`：runtime 固有の native ops
- `runtime/permissions/`：権限コンテナーとチェック

## 拡張機能（extensions）

典型的な拡張は Rust の `deno_core::extension!` マクロで
ネイティブ ops と JS モジュールを登録します。

```text
ユーザーの JS / TS
    │  Deno.readFile / fetch / node:sqlite / ...
    ▼
ext/* の JavaScript API
    │  core.ops など
    ▼
ext/* の Rust op (#[op2])
    │  必要なら権限確認
    ▼
OS / ネットワーク / データベース / native library
```

| 分野 | 主な実装 |
| --- | --- |
| Web APIs | [`ext/web`](https://github.com/taisan11/oden/tree/main/ext/web)、`ext/fetch`、`ext/websocket`、`ext/crypto` |
| システム操作 | `ext/fs`、`ext/net`、`ext/io`、`ext/process` |
| Node 互換 | [`ext/node`](https://github.com/taisan11/oden/tree/main/ext/node)、`ext/node_crypto` |
| Deno API | `ext/kv`、`ext/cron`、`ext/ffi`、`ext/napi` |
| Oden SQLite | [`ext/sqlite`](https://github.com/taisan11/oden/tree/main/ext/sqlite) |

Extension 登録・op 引数・ResourceTable・権限を詳しく追う場合は [Extensions・Ops・権限チェック](/architecture/ops-permissions) を参照してください。

extension を新設する際は Rust/JS 実装、Cargo 依存関係、
`runtime/worker.rs` 側の登録、snapshot・テストの影響を確認します。

## 権限チェックの位置

権限の管理は
[`runtime/permissions/`](https://github.com/taisan11/oden/tree/main/runtime/permissions)
の権限 crate と runtime の構成にまたがります。
読み取り・書き込み・ネットワーク・環境変数・プロセス実行等の
許可判定は、機密操作に到達する前に Rust 側で行う設計です。
JS の UI 上の可否判定だけをセキュリティ境界とみなさないでください。

独自 op を追加するなら、既存の同種 extension の
`PermissionsContainer` の使用方法を参考にしてください。

## JS エンジンとイベントループ

Oden は JavaScript エンジンとして **V8 のみ**をサポートします。
[`libs/deno_v8`](https://github.com/taisan11/oden/tree/main/libs/deno_v8)
は `rusty_v8` を再エクスポートする facade です。
`quickjs` feature とエンジン選択用の CLI オプションはありません。

組込みモジュール `oden:sqlite` の extension とバックエンドについては [Turso SQLite の内部](/features/sqlite) を参照してください。

Tokio は I/O タスクの実行を支え、
`deno_core` が JS の pending tasks や非同期 ops を
イベントループで進めます。

より低レベルを追うには、
[`libs/core/README.md`](https://github.com/taisan11/oden/blob/main/libs/core/README.md)
および [`libs/ops/`](https://github.com/taisan11/oden/tree/main/libs/ops)
を参照してください。
