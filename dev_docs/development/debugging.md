# デバッグと原因の切り分け

Oden は CLI・resolver・runtime・V8・Rust extension の多層構成です。エラーが出た場所ではなく、**どの段階の入出力が最初に期待と違ったか**を基準に追跡すると効率的です。

## まず再現を固定する

問題を再現する最小の `main.ts` と実行引数を用意し、バイナリのバージョン・設定・権限を明示します。

```sh
./target/debug/oden --version
./target/debug/oden run --allow-read ./main.ts
./target/debug/oden --help
```

`main` から再現するのか、`--watch`、`eval`、`compile`、`desktop` などの個別経路でのみ発生するのかを分けます。設定の自動検出が疑わしければ、使用中の `oden.json(c)` と workspace 構成も保存してください。

## エラーの種類別マップ

| 症状 | 最初に追うファイル | 次に確認すること |
| --- | --- | --- |
| フラグが認識されない | `cli/args/flags.rs`、`libs/cli_parser/src/` | parser 結果、`--help` |
| 起動前に止まる | `cli/lib.rs`、`cli/tools/run/mod.rs` | runtime 初期化、factory |
| module not found | `cli/resolver.rs`、`cli/module_loader.rs` | specifier、workspace、npm/JSR |
| リモート取得できない | `cli/file_fetcher.rs`、`cli/cache/` | HTTP cache、許可 |
| TS 変換・型エラー | `cli/module_loader.rs`、`cli/tsc/` | 実行時変換と型チェックの区別 |
| JS API が存在しない | `runtime/worker.rs`、対応 `ext/` | extension、bootstrap、snapshot |
| PermissionDenied 系 | `runtime/permissions/`、関連 `ext/` | 実際にチェックされた操作 |
| 処理が終了しない | `runtime/worker.rs`、`libs/core/` | pending tasks、resource、イベントループ |
| SQLite 特有の失敗 | `ext/sqlite/lib.rs`、`ext/sqlite/*.js` | 権限、statement、Turso 対応範囲 |
| Desktop だけの問題 | `cli/rt_desktop/`、`runtime/ops/desktop.rs` | backend、loopback HTTP、channel |

## 起動経路を細分化する

[`cli/tools/run/mod.rs::run_script`](https://github.com/taisan11/oden/blob/main/cli/tools/run/mod.rs) には `boot_phase` の呼び出しがあり、初期化過程を区別できます。[`runtime/worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker.rs) には `DENO_STARTUP_PHASES` を調べるコードがあります。利用する build で何が出力されるかは実際に確かめてください。

```sh
# Unix 系シェルでの例
DENO_STARTUP_PHASES=1 ./target/debug/oden run ./main.ts
```

※ 診断用の環境変数は upstream 由来の `DENO_*` 名が残っている場合があります。Oden の設定名と混同しないでください。

## 問題が最初に発生した境界を探す

### import は解決されるのに評価に失敗する

まず `cli/module_loader.rs` の `resolve`・`load`・`prepare_load` を調べます。解決された URL と取得済みソースの形式が正しければ、deno_ast の変換・source map・runtime の module evaluation に進みます。[モジュール処理](/architecture/module-pipeline) を参照してください。

### 権限が許可されているのに処理が失敗する

CLI に渡ったフラグと、実際の Rust op が検査する permission を分けて確認します。DB やソケットは内部で追加ファイル・接続先を使う場合もあります。[Ops・権限](/architecture/ops-permissions) を参照してください。

### 新しい API が main worker でしか使えない

`runtime/worker.rs` と `runtime/web_worker.rs`、snapshot や組込み ESM の登録を比較します。runtime の実行時 extension 一覧と、型定義・bundler 側の import 扱いの不一致も確認してください。

## テストによる局所化

1. Rust の特定 crate の修正なら、その crate に対象を絞った `cargo test -p <crate>` を検討する。
2. CLI 入出力の変更なら [`tests/specs/`](https://github.com/taisan11/oden/tree/main/tests/specs) に再現ケースを加える。
3. JS API の変更なら [`tests/unit/`](https://github.com/taisan11/oden/tree/main/tests/unit) や [`tests/unit_node/`](https://github.com/taisan11/oden/tree/main/tests/unit_node) を確認する。
4. 互換性に広く影響するなら Node compatibility / WPT 等へ範囲を広げる。

全テストの一括実行より、まず最小ケースを再現してから関連領域を増やす方が結果を解釈しやすくなります。実行コマンドの詳細は [テスト・CI](/development/testing) を参照してください。

## 修正後に残す情報

PR には再現手順、期待動作と実動作、原因となった境界、関連する変更ファイル、実行したテストと未実行の検証を残します。Oden 固有機能の場合は upstream Deno と振る舞いが異なる理由も記載すると、将来の差分追跡に役立ちます。

関連: [実行ライフサイクル](/architecture/execution-flow) · [新機能の追加](/development/adding-features)
