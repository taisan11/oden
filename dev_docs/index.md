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
    link: /architecture/execution-flow
  - icon: 🔧
    title: 変更箇所を素早く特定
    details: CLI・モジュール解決・権限・Ops・テストの編集ポイント
    link: /development/adding-features
  - icon: ⚡
    title: Oden 独自機能
    details: Turso SQLite・Desktop・V8・設定移行の実装と制約
    link: /features/sqlite
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

## ソースコードの読み方を選ぶ

| 読みたい内容 | 推奨する入口 |
| --- | --- |
| CLI 引数から JS が動くまでの実際の順序 | [実行ライフサイクル](/architecture/execution-flow) |
| import の解決・ダウンロード・キャッシュ | [モジュール処理](/architecture/module-pipeline) |
| JS と Rust の境界、resource と権限 | [Extensions・Ops](/architecture/ops-permissions) |
| Turso を使う二つの SQLite API | [SQLite の内部](/features/sqlite) |
| native window と runtime の通信 | [Desktop の内部](/features/desktop) |
| 新機能の実装・変更前の確認 | [新機能の実装](/development/adding-features) |
| 再現から原因を絞り込む | [デバッグ](/development/debugging) |

ページ中の GitHub リンクはコード上の調査起点です。変更が続いている機能については、実際に使用する revision とそのテストを優先してください。
