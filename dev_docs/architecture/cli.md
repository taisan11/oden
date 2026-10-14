# CLI・コマンド実行

## バイナリのエントリポイント

[`cli/Cargo.toml`](https://github.com/taisan11/oden/blob/main/cli/Cargo.toml)
は package 名を `deno`、`[[bin]]` 名を `oden` と指定します。
[`cli/main.rs`](https://github.com/taisan11/oden/blob/main/cli/main.rs)
はほぼ `deno::main()` の呼び出しだけに留められ、
実処理は [`cli/lib.rs`](https://github.com/taisan11/oden/blob/main/cli/lib.rs)
にあります。

```text
process 起動
  → cli/main.rs
  → deno::main() / cli/lib.rs
  → 引数解析 (cli/args, libs/cli_parser)
  → DenoSubcommand ごとの分岐
      ├─ fmt / lint / test / bundle / compile 等 → cli/tools/*
      ├─ LSP                              → cli/lsp/*
      └─ run / eval 等                    → CliFactory → Worker
```

コマンドにより runtime を使う深さは異なります。

## 引数パーサーは移行途中の二系統

- [`cli/args/flags.rs`](https://github.com/taisan11/oden/blob/main/cli/args/flags.rs):
  従来の `clap` ベースのパーサー
- [`libs/cli_parser/src/`](https://github.com/taisan11/oden/tree/main/libs/cli_parser/src):
  新しいパーサーの定義・変換・テスト
- [`cli/args/mod.rs`](https://github.com/taisan11/oden/blob/main/cli/args/mod.rs):
  パースされた引数・設定の統合

フラグを増やすときは片方だけ更新しないこと。
既存 [パッケージ管理設計ノート](https://github.com/taisan11/oden/blob/main/doc/package-management.md)
にも同期が必要な箇所が詳しく記されています。

## CliFactory の役割

[`cli/factory.rs`](https://github.com/taisan11/oden/blob/main/cli/factory.rs)
は、設定・依存グラフ・ネットワーククライアント・キャッシュ・
npm resolver・ローダー・Worker ファクトリーなどを必要に応じて
組み立てる入口です。個々のサブコマンドに初期化処理を重複させず、
共有部品を提供します。

関連するファイル：

| ファイル | 担当 |
| --- | --- |
| [`cli/module_loader.rs`](https://github.com/taisan11/oden/blob/main/cli/module_loader.rs) | module のロードと変換 |
| [`cli/graph_util.rs`](https://github.com/taisan11/oden/blob/main/cli/graph_util.rs) | module graph 生成の補助 |
| [`cli/file_fetcher.rs`](https://github.com/taisan11/oden/blob/main/cli/file_fetcher.rs) | ソース取得 |
| [`cli/cache/`](https://github.com/taisan11/oden/tree/main/cli/cache) | キャッシュ層 |
| [`cli/lsp/`](https://github.com/taisan11/oden/tree/main/cli/lsp) | Language Server |
| [`cli/tools/`](https://github.com/taisan11/oden/tree/main/cli/tools) | 各コマンドの実装 |

## サブコマンドを追加する際の順序

1. 既存の `DenoSubcommand` と引数構造を確認する
2. `cli/args/` および `libs/cli_parser/` の両方を確認する
3. `cli/tools/<command>.rs` かディレクトリに実行処理を置く
4. `cli/lib.rs` の dispatch へ接続する
5. `tests/specs/<command>/` に CLI の入出力テストを追加する

新しいフラグやコマンドの動作は `./target/debug/oden --help` と
spec テストでも確認してください。

## `run` 系の詳細なコールフロー

`CliFactory` は `module_graph_builder()`、`resolver()`、`create_module_loader_factory()`、`create_cli_main_worker_factory()` などを通して依存を遅延生成します。作成後は `CliMainWorker` と runtime 側の `MainWorker` が bootstrap・ES module 評価・event loop を担当します。

分岐ごとの参照順とデバッグ時の入口は [実行ライフサイクル](/architecture/execution) にまとめています。
