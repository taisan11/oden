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

## 具体的な調査手順

- 起動順序、Worker の bootstrap、event loop：[実行ライフサイクル](/architecture/execution)
- JS/Rust の op や resource と権限：[Extensions・Ops・権限境界](/architecture/ops-and-permissions)
- KV のデータ形式・journal：[Native KV](/features/kv)
- Bun/Node SQLite の API 差：[Turso SQLite](/features/sqlite)
- Native window・WebView・binding：[Desktop](/features/desktop)
- 再現・検証の進め方：[不具合調査と検証](/development/debugging)
