# `oden run` の実行ライフサイクル

このページは `oden run main.ts` を起点に、**CLI の分岐、依存の準備、Worker の構築、JS の実行、終了処理**を追います。構成は [`cli/lib.rs`](https://github.com/taisan11/oden/blob/main/cli/lib.rs)、[`cli/tools/run/mod.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/run/mod.rs)、[`cli/worker.rs`](https://github.com/taisan11/oden/blob/main/cli/worker.rs) を基準にしています。

## 1. 処理の流れ

```text
oden run main.ts
  │
  ├─ cli/main.rs                        バイナリの入口
  ├─ cli/lib.rs::main                   プロセス・V8・Tokio 初期化
  ├─ cli/args + libs/cli_parser         引数 → Flags / DenoSubcommand
  ├─ cli/lib.rs::run_subcommand         Run → tools::run::run_script
  │
  ├─ cli/tools/run/mod.rs::run_script
  │    ├─ check_permission_before_script
  │    ├─ CliFactory::from_flags
  │    ├─ cli_options / workspace_resolver / node_resolver
  │    ├─ main_module, preload_modules, require_modules を解決
  │    ├─ maybe_npm_install と npm bin の分岐
  │    └─ CliMainWorkerFactory::create_main_worker...
  │
  ├─ cli/worker.rs::CliMainWorkerFactory
  │    ├─ npm package の準備
  │    └─ deno_lib::worker::LibMainWorkerFactory
  │           └─ runtime/worker.rs::MainWorker
  │                ├─ JsRuntime + extensions
  │                ├─ PermissionsContainer / OpState
  │                └─ bootstrap / user module / event loop
  │
  └─ CliMainWorker::run → 終了コード・エラー処理
```

**注意:** これは代表的な `run` 経路です。`eval` は独立した入口を持ち、`fmt`、`lint` などは別のツール実装へ分岐します。`--watch` を付けた場合も `run_with_watch` を経由するため、起動経路が変わります。

## 2. エントリポイントとコマンドの振り分け

- [`cli/main.rs`](https://github.com/taisan11/oden/blob/main/cli/main.rs) は `deno::main()` だけを呼び出す薄い入口です。crate 名が `deno` なのは upstream との互換性によります。
- [`cli/lib.rs`](https://github.com/taisan11/oden/blob/main/cli/lib.rs) の `main` がプロセスを初期化し、`run_subcommand` が `DenoSubcommand` を `cli/tools/*` に振り分けます。
- 引数の変換と解析は [`cli/args/`](https://github.com/taisan11/oden/tree/main/cli/args) と [`libs/cli_parser/src/`](https://github.com/taisan11/oden/tree/main/libs/cli_parser/src) を両方確認します。コマンド追加時に片方のみに修正を入れないでください。

`run_subcommand` では非同期コマンドを `spawn_subcommand` でラップし、終了ステータスとエラーを共通の型へ揃えています。サブコマンドごとに戻り値の形が異なっていても、外部プロセスの終了コードとして扱うためです。

## 3. `run_script` で何を準備するか

[`run_script`](https://github.com/taisan11/oden/blob/main/cli/tools/run/mod.rs) の主要な処理は以下です。

1. 実行前の権限条件と watch モードを判定する。
2. `CliFactory::from_flags` から `CliOptions`、キャッシュの場所、HTTP クライアントを取得する。
3. workspace resolver と Node resolver を準備し、main module と preload/require module を求める。
4. 必要であれば npm 依存を準備し、npm バイナリの直接実行に該当するかを調べる。
5. `create_cli_main_worker_factory_with_roots` で Worker factory を組み立てる。
6. `create_main_worker_with_unconfigured_runtime` で Worker を作成し、`worker.run().await` で実行する。

初期化を個々のコマンドに埋め込むのではなく、[`cli/factory.rs`](https://github.com/taisan11/oden/blob/main/cli/factory.rs) に集約したサービスを遅延生成・共有する構造です。

## 4. CLI Worker と Runtime Worker の境界

ここは混同しやすいポイントです。

| 階層 | クラス・型 | 主な責務 |
| --- | --- | --- |
| `cli/factory.rs` | `CliFactory` | 設定・解決器・HTTP・cache などを用意 |
| `cli/worker.rs` | `CliMainWorkerFactory` | CLI 固有のモジュール指定・npm・preload・権限の引き渡し |
| `cli/lib/worker.rs` | `LibMainWorkerFactory` | runtime に渡すサービスやオプションを組み立てる |
| `runtime/worker.rs` | `MainWorker` | `JsRuntime`、拡張、bootstrap、メインモジュール実行 |
| `libs/core/runtime/jsruntime.rs` | `JsRuntime` | V8 isolate、モジュール、イベントループ・非同期 op の実行 |

[`cli/worker.rs::create_custom_worker`](https://github.com/taisan11/oden/blob/main/cli/worker.rs) は、`npm:` 指定から実際のバイナリエントリを求め、必要な npm package を取得した上で `LibMainWorkerFactory` に制御を渡します。テスト実行時には test/bench 関連 JS モジュールも追加でロードします。

## 5. Bootstrap と実行後のイベントループ

[`runtime/worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker.rs) は、`deno_web`・`deno_fetch`・`deno_fs`・`deno_node` などの extension を組み立て、`OpState` に `PermissionsContainer` や各種サービスを格納します。Bootstrap で JS 側のグローバル API を初期化した後、ユーザーモジュールの評価とイベントループへ進みます。

JS の `fetch()` や `Deno.readFile()` が OS の機能に触れる場所は、主に [Ops と権限](/architecture/ops-permissions) で扱います。`await` があるから Rust 側も常に async になるわけではありません。同期 op・非同期 op を区別して追ってください。

## 6. 具体的な追跡手順

```sh
cargo build --locked -p deno --bin oden
./target/debug/oden --version
./target/debug/oden run --allow-read ./main.ts
```

まず `cli/tools/run/mod.rs` で `run_script` を検索し、次に `cli/worker.rs` の `create_main_worker_with_unconfigured_runtime`、`cli/lib/worker.rs` の `create_custom_worker`、`runtime/worker.rs` の `bootstrap` を追います。起動が遅い場合は、`run_script` 内の `boot_phase` と `cli/lib.rs` の初期化経路を確認してください。`DENO_STARTUP_PHASES` による診断の実装も [`runtime/worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker.rs) にあります。

関連: [CLI](/architecture/cli) · [モジュール処理](/architecture/module-pipeline) · [デバッグ](/development/debugging)
