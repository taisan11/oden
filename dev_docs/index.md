---
layout: home

hero:
  name: Oden Internals
  text: Oden の内部構造を読み解く
  tagline: Deno を基にした実験的 JS/TS ランタイムのレイヤー・起動経路・独自機能をコードと結びつけて解説
  actions:
    - theme: brand
      text: アーキテクチャを読む
      link: /architecture/overview
    - theme: alt
      text: 開発を始める
      link: /guide/getting-started

features:
  - icon: 🧱
    title: レイヤー別の構造
    details: cli / runtime / ext / libs の責務と依存方向を解説
    link: /architecture/overview
  - icon: 🔧
    title: 変更箇所を素早く特定
    details: CLI・モジュール解決・権限・Ops・テストの編集ポイント
    link: /development/navigation
  - icon: ⚡
    title: Oden 独自機能
    details: Native KV・Turso SQLite・Desktop・V8・設定移行
    link: /features/oden
  - icon: 🔍
    title: 実行の流れを追う
    details: 起動から ModuleLoader・Worker・Event loop・Ops への呼び出しを追跡
    link: /architecture/execution
  - icon: 🧪
    title: 検証と自動化
    details: ローカルビルド、各種テスト、軽量 GitHub Actions の役割
    link: /development/testing
---

## このサイトの位置づけ

このサイトは [`taisan11/oden`](https://github.com/taisan11/oden) の
**実装を読む人向け**のガイドです。ユーザー向け API リファレンスではなく、
実際の Rust・JS・TS ソースと、リポジトリ内の既存説明を接続します。

Oden は Deno 由来の実験的フォークで、独自の `oden` 実行ファイルを持ちます。
`Deno` 名前空間や多くの Rust crate 名は互換性のため維持されています。
独自機能には変更中・未完成のものがあるため、利用前にコードとテストを確認してください。

> [!IMPORTANT]
> 記述はサイト作成時の `main` ブランチを基準にしています。
> 詳細が必要なときは各ページにあるソースリンクと
> [既存の開発ノート](https://github.com/taisan11/oden/tree/main/doc)を参照してください。

## 実装をさらに深掘りする

| テーマ | コードの調査入口 |
| --- | --- |
| `run` の実行と Worker 作成 | [実行ライフサイクル](/architecture/execution) |
| `import` の解決・Graph・キャッシュ・変換 | [モジュール処理の詳細](/architecture/module-pipeline) |
| JavaScript と Rust の境界 | [Extensions・Ops・権限](/architecture/ops-and-permissions) |
| 独自バックエンド | [Turso SQLite](/features/sqlite) / [Desktop](/features/desktop) / [Native KV](/features/kv) |
| 新しいフラグ・Web API を開発する | [新機能の実装手順](/development/adding-features) |
| 不具合を再現して追跡する | [デバッグと検証](/development/debugging) |

すべてのページでソースコードと現行の実装制約を優先し、上流 Deno の振る舞いを無条件に保証するものではありません。
