# 変更箇所を探す

目的から実装ファイルに移動するためのショートカットです。

## 逆引きマップ

| やりたいこと | 最初に見る場所 | 関連するテスト |
| --- | --- | --- |
| CLI フラグ・サブコマンドを追加 | [`cli/args/flags.rs`](https://github.com/taisan11/oden/blob/main/cli/args/flags.rs)、[`libs/cli_parser`](https://github.com/taisan11/oden/tree/main/libs/cli_parser) | `tests/specs/<command>/` |
| コマンドの処理を変更 | [`cli/tools/`](https://github.com/taisan11/oden/tree/main/cli/tools)、`cli/lib.rs` | `tests/specs/` |
| module resolution | [`libs/resolver`](https://github.com/taisan11/oden/tree/main/libs/resolver)、`cli/module_loader.rs` | `tests/specs/npm/`、`tests/specs/jsr/` |
| npm / package.json | `libs/npm`、`libs/npm_installer`、`cli/tools/pm/` | `tests/specs/add/`、`tests/specs/install/` |
| config / workspace | [`libs/config`](https://github.com/taisan11/oden/tree/main/libs/config) | `tests/specs/workspaces/` |
| Web API を追加 | [`ext/`](https://github.com/taisan11/oden/tree/main/ext) の対応 crate、`runtime/worker.rs` | `tests/unit/` |
| ファイル・ネットワーク権限 | [`runtime/permissions`](https://github.com/taisan11/oden/tree/main/runtime/permissions)、`ext/fs`、`ext/net` | `tests/specs/permission/` |
| JS エンジン・op macro | `libs/core`、`libs/ops`、`libs/deno_v8` | crate unit tests |
| Node API 互換 | `ext/node`、`ext/node_crypto` | `tests/unit_node/`、`tests/node_compat/` |
| Turso SQLite | [`ext/sqlite`](https://github.com/taisan11/oden/tree/main/ext/sqlite) | 対象 API の spec / unit |
| Desktop backend | `cli/tools/desktop.rs`、`cli/rt_desktop/`、`runtime/ops/desktop.rs` | Desktop 関連のテスト |
| LSP | [`cli/lsp/`](https://github.com/taisan11/oden/tree/main/cli/lsp) | LSP 関連のテスト |
| 設定移行 | [`cli/tools/from_deno.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/from_deno.rs) | `tests/specs/from/deno/` |
| Worker 起動・イベントループ | [実行ライフサイクル](/architecture/execution-flow)、`cli/worker.rs`、`runtime/worker.rs` | `tests/specs/`、runtime unit |
| extension・op・resource | [Ops・権限](/architecture/ops-permissions)、`libs/ops` | `tests/unit/`、対応 crate |
| ネイティブ連携 | [Desktop の内部](/features/desktop)、`cli/rt_desktop/` | Desktop 関連のテスト |

変更作業のチェックリストは [新機能の実装手順](/development/adding-features)、実行時の症状別の切り分けは [デバッグ](/development/debugging) を参照してください。

## デバッグ用の基本ルート

1. 最小の再現コード・CLI 引数を用意する
2. `cli/args` で渡ったオプションを確認する
3. `CliFactory` が採用した設定・resolver・cache を追う
4. JS 処理なら extension の JS → op → Rust の順に追う
5. 修正するレイヤーのテストに加え、利用者から見える
   振る舞いには `tests/specs/` を追加する

## 役立つ既存ドキュメント

- [`CLAUDE.md`](https://github.com/taisan11/oden/blob/main/CLAUDE.md)
- [`doc/codebase-map.md`](https://github.com/taisan11/oden/blob/main/doc/codebase-map.md)
- [`doc/package-management.md`](https://github.com/taisan11/oden/blob/main/doc/package-management.md)
- [`doc/desktop-architecture.md`](https://github.com/taisan11/oden/blob/main/doc/desktop-architecture.md)

既存ドキュメントのパス・実装説明が現行コードと食い違う場合は、
該当ブランチのソースを優先してください。
