# ビルド・テスト・CI

## ローカル開発

Oden 本体のビルドは Cargo workspace から行います。

```sh
# Rust toolchain は rust-toolchain.toml の指定を使用
cargo metadata --locked --no-deps --format-version 1
cargo build --locked -p deno --bin oden
./target/debug/oden --version
./target/debug/oden eval 'console.log(1 + 2)'
```

開発者向け `./x` コマンドも用意されています。

```sh
./x --help
./x fmt
./x lint-js
./x verify
./x spec
```

`./x` のオプションやサブコマンドは実装により変更されるため、
まずヘルプを確認してください。

## テストの種類

| 種類 | 場所 | 目的 |
| --- | --- | --- |
| Spec | [`tests/specs/`](https://github.com/taisan11/oden/tree/main/tests/specs) | CLI を実行し、終了状態・出力・生成物を検証 |
| Runtime unit | `tests/unit/` | JS/TS の Deno・Web API |
| Node unit | `tests/unit_node/` | Node 組込み API |
| Node compatibility | `tests/node_compat/` | Node のテスト群による互換性チェック |
| Web Platform Tests | `tests/wpt/` | Web 標準 API |
| Rust unit | 各 crate | Rust の個別機能 |

Spec は `__test__.jsonc` に手順を記述する方式です。
テストハーネスが必要な JS unit テストは、
`oden test` だけで代替できるとは限りません。
詳しくは [`doc/testing.md`](https://github.com/taisan11/oden/blob/main/doc/testing.md)。

## 現在の GitHub Actions

**現行の [`.github/workflows/ci.yml`](https://github.com/taisan11/oden/blob/main/.github/workflows/ci.yml) は軽量 CI** です。

| Workflow | 起動方法 | 検証内容 |
| --- | --- | --- |
| `ci.yml` | main push / PR / 手動 | dprint 形式、JS/TS lint、Cargo metadata |
| `build.yml` | 手動 | Linux x86_64 の Rust build、smoke test、artifact |
| `release.yml` | 手動 | タグ指定の Linux build、draft GitHub Release |
| `dev-docs.yml` | dev_docs の変更 / 手動 | VitePress build、main から Pages deploy |

CI が緑でも **Rust 全体のコンパイル・Runtime 互換性は確認されません**。
その検証は必要に応じて手動 build・対応するテストで実施してください。
upstream 由来の [`doc/ci.md`](https://github.com/taisan11/oden/blob/main/doc/ci.md)
には現在存在しない生成 workflow の説明があるため、実際の YAML を優先します。

## ドキュメントサイトの検証・公開

```sh
cd dev_docs
npm install
npm run docs:build
npm run docs:preview
```

`.github/workflows/dev-docs.yml` は PR 時にビルドを検証し、
`main` への push と手動実行で Pages artifact を公開します。
出力先は `dev_docs/.vitepress/dist`、公開 base は `/oden/` です。

GitHub で最初に一度、
**Settings → Pages → Build and deployment → Source → GitHub Actions**
を選択する必要があります。サイトの URL は通常
[`https://taisan11.github.io/oden/`](https://taisan11.github.io/oden/) です
（初回デプロイが成功するまでは利用できません）。

詳細は [`.github/WORKFLOWS.md`](https://github.com/taisan11/oden/blob/main/.github/WORKFLOWS.md)。

## 変更種類に応じた最小検証

- **ドキュメントのみ**：`npm run docs:build --prefix dev_docs` と相対リンク・ナビゲーションの確認
- **CLI 解析・フラグ**：parser の Rust unit / `--help` / 対象 `tests/specs`
- **Runtime / Ops**：対象 crate の Rust テストと `tests/unit`、必要なら worker テスト
- **SQLite**：`cargo check -p oden_sqlite --features deno_core/v8`、[SQLite のテスト例](/features/sqlite#型と-worker-テスト)
- **KV**：Native と Deno 互換 backend の両方を検証し、永続化と再起動後の読み込みを確認

具体的な切り分けは [不具合調査と検証](/development/debugging) を参照してください。
