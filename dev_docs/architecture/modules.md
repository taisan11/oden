# モジュール解決・キャッシュ・パッケージ管理

## 主要な担当範囲

モジュールの解決・取得・実行は単一の関数ではなく、複数の crate と
CLI のコンポーネントに分かれます。

```text
import specifier (file:, http:, npm:, jsr:, node:)
      │
      ▼
config / import map / workspace
      │
      ▼
resolver + npm / JSR / Node resolver
      │
      ▼
module graph + file fetcher + caches
      │
      ▼
module loader → runtime worker → JS 実行
```

| 領域 | 主なソース |
| --- | --- |
| workspace/config 検出 | [`libs/config`](https://github.com/taisan11/oden/tree/main/libs/config) |
| 依存解決 | [`libs/resolver`](https://github.com/taisan11/oden/tree/main/libs/resolver) |
| Node.js モジュール解決 | [`libs/node_resolver`](https://github.com/taisan11/oden/tree/main/libs/node_resolver) |
| npm バージョン・解決 | [`libs/npm`](https://github.com/taisan11/oden/tree/main/libs/npm)、`libs/npm_installer` |
| ロック管理 | [`libs/lockfile`](https://github.com/taisan11/oden/tree/main/libs/lockfile) |
| グラフ構築 | [`cli/graph_util.rs`](https://github.com/taisan11/oden/blob/main/cli/graph_util.rs) |
| ダウンロードとキャッシュ | [`cli/file_fetcher.rs`](https://github.com/taisan11/oden/blob/main/cli/file_fetcher.rs)、`cli/cache/` |
| 実際のロード | [`cli/module_loader.rs`](https://github.com/taisan11/oden/blob/main/cli/module_loader.rs) |

各 crate は独立してテストしやすいよう分割され、
`cli/factory.rs` が利用に必要な実装を接続します。

## `oden add` / `oden install` の流れ

1. `cli/args/` または `libs/cli_parser/` がフラグを解析
2. [`cli/tools/pm/mod.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/pm/mod.rs)
   が要求された依存のバージョンと書き込み先を決定
3. `ConfigUpdater` が `oden.json` の `imports` または
   `package.json` の依存関係セクションを書き換える
4. [`cli/tools/pm/cache_deps.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/pm/cache_deps.rs)
   にある共有処理が依存グラフ・キャッシュ・インストールを進める

`--dev`、`--save-optional`、`--no-save` は
設定の保存方法に影響します。
特に `optionalDependencies` は通常の解決ルートだけでは
インストールされないため、追加の graph roots を使う実装が
[既存設計ノート](https://github.com/taisan11/oden/blob/main/doc/package-management.md)
に解説されています。改修前に最新コードで挙動を再確認してください。

## Oden 固有の名前と配置

Oden は `oden.json` / `oden.jsonc` を自動検出し、
プロジェクト内の生成ファイル・npm ストアに `.oden` を使用します。
グローバルキャッシュにも Oden 用のディレクトリを用います。
一方で互換性のため `Deno` 名前空間や内部 crate の `deno_*`
名称はそのまま残されています。

設定の移行手順は [Oden 独自機能](/features/oden) にあります。
