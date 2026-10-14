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

変更するレイヤーによって必要なテストが異なります。[新機能の実装](/development/adding-features) と [デバッグ](/development/debugging) に、修正種別ごとの確認ポイントを用意しています。

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
詳しくは [`doc/testing.md`](https://github.com/taisan11/oden/blob/main/doc/testing.md)。SQLite のような独自 API には [SQLite 固有の検証コマンド](/features/sqlite#変更時の検証) もあります。

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

## テストを選ぶときの目安

- CLI 引数・出力の変更: 対応する `tests/specs/` に最小再現を追加。
- Web API・Rust op の変更: 影響する crate の unit と `tests/unit/`。許可・拒否の両ケースを確認。
- Node API の変更: `tests/unit_node/` と必要な `tests/node_compat/`。
- モジュール解決: npm・JSR・workspace の spec。resolver と loader の段階を区別して再現。
- Desktop の変更: ABI・起動・画面側イベントをそれぞれ確認。軽量 CI だけでは保証されない。

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
