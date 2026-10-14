# 全体アーキテクチャ

Oden は多数の Rust crate から成る Cargo workspace で、
上位の CLI が下位の実行基盤やモジュール解決ライブラリを組み合わせます。

## 主要レイヤー

```text
                    oden executable
              cli/main.rs → cli/lib.rs
                           │
       ┌───────────────────┴──────────────────┐
       │ cli/   flags / subcommands / factory │
       │        loader / LSP / package tools  │
       └───────────────────┬──────────────────┘
                           │
       ┌───────────────────▼──────────────────┐
       │ runtime/   MainWorker / WebWorker    │
       │            bootstrap / permissions   │
       └───────────────────┬──────────────────┘
                           │
       ┌───────────────────▼──────────────────┐
       │ ext/   Web APIs / filesystem / net   │
       │        Node compat / SQLite / ops   │
       └───────────────────┬──────────────────┘
                           │
       ┌───────────────────▼──────────────────┐
       │ libs/core / libs/ops / libs/serde_v8 │
       │        JsRuntime / op bridge         │
       └───────────────────┬──────────────────┘
                           │
                     V8 + Tokio
              (JavaScript エンジンは V8 のみ)
```

この図は役割の概略です。実際には `cli/` と `runtime/` の両方が
複数の `libs/*` を直接参照し、すべてが一直線の呼び出し順に
なるわけではありません。

| ディレクトリ | 主な責務 | 代表ファイル |
| --- | --- | --- |
| [`cli/`](https://github.com/taisan11/oden/tree/main/cli) | ユーザー向けコマンド・設定・LSP・ツール | `lib.rs`、`args/`、`factory.rs` |
| [`runtime/`](https://github.com/taisan11/oden/tree/main/runtime) | worker と拡張機能の組立・JS bootstrap | `worker.rs`、`web_worker.rs` |
| [`ext/`](https://github.com/taisan11/oden/tree/main/ext) | ネイティブ ops と JS API 実装 | `fs/`、`net/`、`web/`、`sqlite/` |
| [`libs/`](https://github.com/taisan11/oden/tree/main/libs) | JS エンジンの中核・解決・キャッシュ | `core/`、`ops/`、`resolver/` |
| [`tests/`](https://github.com/taisan11/oden/tree/main/tests) | CLI・Web API・Node 互換性テスト | `specs/`、`unit/`、`node_compat/` |
| [`tools/`](https://github.com/taisan11/oden/tree/main/tools) | フォーマット・lint 等の開発スクリプト | `format.js`、`lint.js` |

## Cargo workspace

[ルート `Cargo.toml`](https://github.com/taisan11/oden/blob/main/Cargo.toml)
の `[workspace].members` が各 crate の所属を定義します。

- `cli/` の crate 名は `deno`、実行ファイル名は `oden`
- `runtime/` の crate 名は `deno_runtime`
- `libs/core/` の crate 名は `deno_core`
- `ext/sqlite/` の crate 名は `oden_sqlite`
- 解決器や npm サポートは `libs/resolver/`、`libs/npm*/`、
  `libs/config/` などで構成

Oden のブランド名と内部 crate の命名が混在しているのは、
上流 Deno のコードとの互換性を維持するためです。

## JS と Rust の境界

JS からファイル操作・ネットワークなどのネイティブ機能を呼び出す際には、
`ext/*` 内の JavaScript API から登録済みの **op** を経由して Rust 実装へ
到達します。`libs/ops` の `#[op2]` マクロが橋渡しを生成し、
`libs/core` がイベントループや `JsRuntime` を管理します。

詳細は [Runtime・Ops・権限](/architecture/runtime) および
[Extensions・Ops・権限境界](/architecture/ops-and-permissions) を参照。

## ビルド時と実行時の区別

`cli/build.rs` や `cli/snapshot/`、`runtime/snapshot.rs` は
組込み JavaScript・スナップショットなどの生成に関わります。
実行時のロード・権限チェック・イベントループは別経路です。
コードを読む際は「ビルド時に実行されるか、利用者のプロセスで
実行されるか」を最初に区別してください。

## 既存の詳細資料

- [`doc/architecture.md`](https://github.com/taisan11/oden/blob/main/doc/architecture.md)
- [`doc/codebase-map.md`](https://github.com/taisan11/oden/blob/main/doc/codebase-map.md)
- [`CLAUDE.md`](https://github.com/taisan11/oden/blob/main/CLAUDE.md)

既存資料には upstream 時代のパスや CI の説明が一部残るため、
このサイトでは現行ツリーと実際の workflow を優先しています。

## より深く調べる

- [CLI → Worker → V8 の実行ライフサイクル](/architecture/execution)：`run` の起動から event loop の終了まで
- [Extensions・Ops・権限境界](/architecture/ops-and-permissions)：JS と Rust の接続および権限判定
- [Native KV](/features/kv) / [Turso SQLite](/features/sqlite) / [Desktop](/features/desktop)：独自機能の処理フロー
