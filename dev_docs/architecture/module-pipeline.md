# モジュール解決とロードの実装経路

JavaScript の `import` は、解決・取得・キャッシュ・変換・評価という別々の段階を通ります。エラーの発生箇所を間違えないため、それぞれを分けて追います。

## 全体のパイプライン

```text
import "specifier"
       │
       ▼
CliOptions / workspace config (oden.json, package.json, import map)
       │
       ▼
CliResolver / WorkspaceResolver / NodeResolver / npm・JSR resolver
       │
       ▼
ModuleGraphCreator + ModuleGraphBuilder
       │     依存関係・事前準備・必要に応じて型解析
       ▼
CliFileFetcher + DenoDir / HTTP cache / npm cache
       │
       ▼
CliModuleLoaderFactory → CliModuleLoader
       │     media type 判定・JS/TS 変換・source map・code cache
       ▼
deno_core::ModuleLoader → V8 module evaluation
```

この図は論理的な分担です。実際は、resolver・fetcher・loader がそれぞれ内部から他のコンポーネントを呼ぶため、全 import に対して上から一度ずつ実行されるわけではありません。

## 入口と役割

| 段階 | コード | 確認する内容 |
| --- | --- | --- |
| 構成の発見 | [`libs/config/`](https://github.com/taisan11/oden/tree/main/libs/config)、[`cli/args/mod.rs`](https://github.com/taisan11/oden/blob/main/cli/args/mod.rs) | `oden.json(c)`、workspace・task・import map |
| 解決器の作成 | [`cli/factory.rs`](https://github.com/taisan11/oden/blob/main/cli/factory.rs) | `workspace_resolver`、`node_resolver`、`cli_resolver` |
| 解決ロジック | [`cli/resolver.rs`](https://github.com/taisan11/oden/blob/main/cli/resolver.rs)、[`libs/resolver/`](https://github.com/taisan11/oden/tree/main/libs/resolver) | 相対パス・workspace・npm/JSR の解釈 |
| Node 解決 | [`libs/node_resolver/`](https://github.com/taisan11/oden/tree/main/libs/node_resolver)、[`cli/node.rs`](https://github.com/taisan11/oden/blob/main/cli/node.rs) | `node:`、package exports、CJS/ESM |
| Graph | [`cli/graph_util.rs`](https://github.com/taisan11/oden/blob/main/cli/graph_util.rs)、[`cli/graph_container.rs`](https://github.com/taisan11/oden/blob/main/cli/graph_container.rs) | 依存グラフの準備・共有 |
| 取得 | [`cli/file_fetcher.rs`](https://github.com/taisan11/oden/blob/main/cli/file_fetcher.rs) | HTTP やローカルファイルの取得 |
| ロード・変換 | [`cli/module_loader.rs`](https://github.com/taisan11/oden/blob/main/cli/module_loader.rs) | `ModuleLoader`・transpile・cache |
| キャッシュ | [`cli/cache/`](https://github.com/taisan11/oden/tree/main/cli/cache)、[`libs/cache_dir/`](https://github.com/taisan11/oden/tree/main/libs/cache_dir) | リモートソース・解決結果・V8 code cache |

## スキームごとの扱い

| 指定例 | 重要な確認点 |
| --- | --- |
| `./lib.ts`、`file:///...` | パス解決、メディアタイプ、ファイル権限 |
| `https://example.com/mod.ts` | fetcher、HTTP cache、ネットワークアクセス |
| `npm:pkg@version` | npm レジストリの解決、インストーラー、Node resolver |
| `jsr:@scope/pkg` | JSR のメタデータ・依存関係、version 解決 |
| `node:fs` | Node builtin の実装と runtime 側の Node extension |
| `oden:sqlite` | Oden 組込みの lazy-loaded ESM。通常の遠隔取得ではない |

`oden:sqlite` は [`ext/sqlite/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) の `lazy_loaded_esm` に登録されています。任意の文字列を新しい import scheme として採用するには、単に resolver を修正するだけでなく、runtime・型解決・bundler を含めて検証する必要があります。

## Loader と Graph は別物

- **Graph** は依存関係を集め、preload や解析の準備を進めます。`ModuleLoadPreparer`・`ModuleGraphCreator` を検索してください。
- **Resolver** は specifier の意味と参照先を決めます。同じ裸の package 名でも workspace・package.json・CJS/ESM の条件で経路が異なります。
- **Loader** は確定した module を読み込み、V8 に渡せる形へ変換します。[`cli/module_loader.rs`](https://github.com/taisan11/oden/blob/main/cli/module_loader.rs) の `resolve`、`load`、`prepare_load` を起点にすると見通しがよくなります。
- **Type checker** は実行用のロードと同一ではありません。[`cli/tsc/`](https://github.com/taisan11/oden/tree/main/cli/tsc) と [`cli/type_checker.rs`](https://github.com/taisan11/oden/blob/main/cli/type_checker.rs) を別途確認します。

## キャッシュの見取り図

`DenoDir` は CLI が利用するキャッシュの場所を管理します。ただし「一つのキャッシュ」ではありません。HTTP 応答の保存、npm アーカイブ・展開結果、解析済み情報、トランスパイル・code cache などは異なるレイヤーです。初期化元は [`cli/factory.rs`](https://github.com/taisan11/oden/blob/main/cli/factory.rs) の `deno_dir`、`caches`、`file_fetcher` から確認できます。

Oden は設定として `oden.json`・`oden.jsonc`、生成先に `.oden` を使いますが、互換性維持のため環境変数や内部型には `DENO_*`・`DenoDir` 等の名称も残っています。すべての名前が `ODEN_*` になっていると仮定しないでください。

## `oden add` と `oden run` は同じ処理ではない

[`cli/tools/pm/mod.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/pm/mod.rs) は依存追加時に保存先を判断し、`oden.json` の `imports` または `package.json` の `dependencies`・`devDependencies`・`optionalDependencies` を更新します。`cache_deps.rs` は依存キャッシュ処理を担当します。

一方 `oden run` は既存設定・import を解決し、必要な依存を準備して実行します。「追加コマンドの設定ファイル更新」と「実行時の依存解決」を分けて読むと、余分な設定変更を避けられます。

## 不具合の切り分け

1. 対象が workspace 内の相対パス、`npm:`、`jsr:`、`node:` のどれかを特定する。
2. 誤った URL や package が選ばれたら **resolver** を調べる。
3. URL は正しいが取得できないなら **fetcher / permission / cache** を調べる。
4. ファイル取得後の構文・変換エラーなら **loader / deno_ast / type checker** を調べる。
5. 評価時の例外・イベントループの問題なら **runtime/worker.rs / libs/core** を調べる。
6. 修正後は該当 import スキームの [`tests/specs/`](https://github.com/taisan11/oden/tree/main/tests/specs) を実行し、Node や JSR の回帰も確認する。

関連: [既存のモジュール概説](/architecture/modules) · [実行ライフサイクル](/architecture/execution-flow)
