# CLI から JavaScript 実行まで

このページは `oden run app.ts` を例に、**起動・依存解決・Worker 構築・モジュール評価・終了処理**をソースコードの順に追います。`fmt` や `lint`、`compile` は異なる分岐を使うため、全コマンド共通の経路ではありません。

## 1. エントリポイントとコマンド振り分け

```text
cli/main.rs : main() → deno::main()
    ↓
cli/lib.rs : main() / run_subcommand(...)
    ↓
cli/args/*・libs/cli_parser/* : Flags / DenoSubcommand
    ↓
cli/tools/run/* : run 用の実行処理
    ↓
cli/factory.rs : CliFactory
    ↓
cli/worker.rs : CliMainWorkerFactory / CliMainWorker
    ↓
runtime/worker.rs : MainWorker
    ↓
libs/core : JsRuntime（V8 と event loop）
```

- [`cli/main.rs`](https://github.com/taisan11/oden/blob/main/cli/main.rs) は薄いラッパーです。
- [`cli/lib.rs`](https://github.com/taisan11/oden/blob/main/cli/lib.rs) の `run_subcommand` は `DenoSubcommand` に応じて `tools::*` を呼び分けます。`eval`・`test`・`lsp` は同じ処理ではありません。
- [`cli/args/mod.rs`](https://github.com/taisan11/oden/blob/main/cli/args/mod.rs) と [`libs/cli_parser`](https://github.com/taisan11/oden/tree/main/libs/cli_parser) が CLI 引数モデルに関与します。

## 2. CliFactory は初期化を遅延する

[`cli/factory.rs`](https://github.com/taisan11/oden/blob/main/cli/factory.rs) の `CliFactory` は、設定読み込み・キャッシュ・HTTP クライアント・パッケージ解決・モジュールグラフ・権限の構成を提供します。各部品は必要なタイミングで取得され、コマンドごとに無条件で全部作る設計ではありません。

| 入口 | 役割 |
| --- | --- |
| `resolver()` / `npm_resolver()` | import・npm パッケージの解決 |
| `module_graph_builder()` / `module_graph_creator()` | 実行前の依存グラフ準備 |
| `root_permissions_container()` | 起動時の root 権限コンテナー |
| `create_module_loader_factory()` | Worker に渡す ModuleLoader の構成 |
| `create_cli_main_worker_factory()` | CLI Worker の作成に必要な依存をまとめる |

例えば `--allow-read` の挙動を追う際は、引数解析だけでなく `root_permissions_container()` と extension 内のチェックまで確認します。

## 3. モジュール解決とロード

`import "./mod.ts"` などの specifier は、設定・import map・workspace・Node/npm/JSR の解決ルールを通ります。その後、`cli/graph_util.rs` が依存グラフ構築を補助し、`cli/module_loader.rs` が実際の読み込み・変換を担当します。

[`CliModuleLoader`](https://github.com/taisan11/oden/blob/main/cli/module_loader.rs) は `deno_core::ModuleLoader` の `resolve` / `load` を実装します。`load` の処理はグラフのソースや eszip、実行時のモジュールフックの有無などで変わります。**単純な「ネットワーク取得 → V8」という一本道ではありません。**

詳しくは [モジュール解決・キャッシュ](/architecture/modules) を参照してください。

## 4. Worker の生成と bootstrap

[`cli/worker.rs`](https://github.com/taisan11/oden/blob/main/cli/worker.rs) の `CliMainWorkerFactory` が `CliMainWorker` を作り、内部に runtime 側の `MainWorker` を持ちます。

[`runtime/worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker.rs) では以下が分離されています。

1. `MainWorker` と `JsRuntime`、extensions 等の組み立て
2. `bootstrap(BootstrapOptions)` による JS 側の初期化
3. `preload_main_module` による読み込み・インスタンス化
4. `evaluate_module` によるモジュール評価と event loop の並行処理

`BootstrapOptions` は [`runtime/worker_bootstrap.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker_bootstrap.rs) にあり、CLI 引数・モード・開発者ツール関連設定・serve 設定などを JS 側へ渡します。Rust と JS のフィールド順を変更する際は対応する bootstrap コードにも注意が必要です。

## 5. Event loop と終了処理

`MainWorker::evaluate_module` は `JsRuntime::mod_evaluate` と `run_event_loop` を協調させ、非同期 op・Promise・タイマーなどの進行を支えます。低レベルの実装は [`libs/core/runtime/jsruntime.rs`](https://github.com/taisan11/oden/blob/main/libs/core/runtime/jsruntime.rs) にあります。

CLI 固有の [`CliMainWorker::run`](https://github.com/taisan11/oden/blob/main/cli/worker.rs) は、必要に応じた coverage・CPU profiler・HMR・Inspector の処理も扱います。通常終了と `Deno.exit()` の即時終了、watch 下での終了は同じではありません。

## 6. Web Worker は別インスタンス

`new Worker(...)` の経路は [`runtime/web_worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/web_worker.rs) にあり、専用の worker ID・メッセージチャネル・ライフサイクルを持ちます。MainWorker に追加した extension や state が WebWorker にも自動反映されると思い込まないことが重要です。

## トレースの最短ルート

**起動コマンド** → `cli/lib.rs` → `cli/tools/run` → `cli/factory.rs` → `cli/worker.rs` → `runtime/worker.rs` → `libs/core/runtime/jsruntime.rs`

**モジュールの不具合**なら途中から `cli/module_loader.rs` / `libs/resolver` へ、**Web API** なら [Ops と権限](/architecture/ops-and-permissions) へ分岐して読むと効率的です。
