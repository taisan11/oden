# 不具合調査と変更時の検証

Oden の不具合は **CLI パーサー / module resolution / JS runtime / native op / OS** のどこで起きているかを切り分けると、修正範囲を小さくできます。

## 再現からテストまで

1. 失敗する最小の `.ts` / `.js` と、実行コマンド（フラグ・cwd・OS）を記録する
2. `oden --version` と、変更対象 commit を確認する
3. CLI 分岐は `cli/lib.rs`、設定は `libs/config`、取得は `cli/file_fetcher.rs`、実行は `cli/worker.rs` を追う
4. JS API の失敗は公開 JS → `core.ops` → `#[op2]` → native resource / permission の順で確認する
5. 一番狭い既存の回帰テストを実行し、変更したレイヤーに新しいテストを追加する
6. 可能なら上位の spec テストで実際の CLI 挙動まで検証する

## 症状別チェック

| 症状 | 見る場所 | 確認すること |
| --- | --- | --- |
| `--flag` が認識されない | `cli/args` / `libs/cli_parser` | 解析・help・dispatch の接続 |
| `npm:` / `jsr:` が失敗 | `libs/resolver` / `cli/module_loader.rs` | import map・workspace・依存グラフ |
| 実行時だけ import が失敗 | `cli/module_loader.rs` | `resolve` / `load` / hook の違い |
| `PermissionDenied` | `runtime/permissions` と対象 `ext` | 権限構成と Rust op 内のチェック |
| `Deno.*` / `Oden.*` API が未定義 | `runtime/worker.rs` / `runtime/web_worker.rs` | extension・bootstrap・型宣言 |
| native KV の起動が遅い | `ext/kv/native.rs` | journal の再生量・メモリ index |
| SQLite の SQL が通らない | `ext/sqlite` | JS adapter 仕様と Turso SQL 対応 |
| Desktop が真っ白 | `cli/rt_desktop` / `runtime/ops/desktop.rs` | loopback HTTP と event channel の切り分け |

## 開発時の代表コマンド

```sh
# workspace 情報の確認（実行ファイルをビルドしない）
cargo metadata --locked --no-deps --format-version 1

# CLI 本体
cargo build --locked -p deno --bin oden
./target/debug/oden --help
./target/debug/oden eval 'console.log(1 + 2)'

# SQLite に変更を加えた場合の例
cargo check -p oden_sqlite --features deno_core/v8
./target/debug/oden test --config tests/config/oden.json --allow-all tests/unit/oden_sqlite_test.ts

# VitePress の変更だけなら Rust は不要
npm install --prefix dev_docs --no-audit --no-fund
npm run docs:build --prefix dev_docs
```

## テストの選び方

`tests/specs` は CLI 実行結果、`tests/unit` は JS API、`tests/unit_node` は Node API、各 Rust crate のテストは内部ロジックを検証します。MainWorker だけでなく WebWorker での挙動が関係する場合は worker のテストも選んでください。

CI の常時チェックは形式・lint・Cargo metadata を主に扱い、フル Rust ビルド・全互換テストは行いません。[ビルド・テスト・CI](/development/testing) にチェック範囲をまとめています。

## Docs を修正する際

- このサイトの記述は **現行 `main` のコードを一次情報**にし、上流 Deno の仕様をそのまま断定しない
- 関数名や CLI フラグは、実装・テストと対応するリンクを添える
- `oden` と `Deno` 名前空間、`Oden.openKv` と `Deno.openKv` など、意図的な別名・分岐を混同しない
- 内部説明を変えたらナビゲーション・既存の概要ページ・型定義にも古い記述が残っていないか確認する
