# はじめに

## 何を理解するためのサイトか

Oden は [Deno](https://github.com/denoland/deno) を基にした、Rust / V8 /
Tokio を中心とする JavaScript・TypeScript・WebAssembly ランタイムです。
CLI・モジュール解決・実行エンジン・Web API・OS アクセスが別の crate に
分割されています。

入口となるのは次のページです。

- [全体像](/architecture/overview)：各レイヤーの依存関係
- [CLI](/architecture/cli)：引数解析からコマンド実行まで
- [Runtime](/architecture/runtime)：JS の実行とネイティブ API の橋渡し
- [モジュール解決](/architecture/modules)：`npm:`・`jsr:`・ロック・キャッシュ
- [Oden 独自機能](/features/oden)：Deno と異なるコード

## ソースを読むおすすめ順序

最初は [全体アーキテクチャ](/architecture/overview) → [実行ライフサイクル](/architecture/execution) → [モジュール処理](/architecture/module-pipeline) の順番に読むと、CLI・Worker・JS 実行の境界をつかみやすくなります。API を追加したいときは [Ops・権限](/architecture/ops-and-permissions) と [新機能の実装手順](/development/adding-features) を併読してください。

## ソースの準備

ルートの [README](https://github.com/taisan11/oden/blob/main/README.md) と
[CONTRIBUTING](https://github.com/taisan11/oden/blob/main/.github/CONTRIBUTING.md)
を参照してください。サブモジュールを含めて clone します。

```sh
git clone --recurse-submodules https://github.com/taisan11/oden.git
cd oden
cargo build --locked -p deno --bin oden
./target/debug/oden eval 'console.log(1 + 2)'
```

Rust の指定バージョンは [`rust-toolchain.toml`](https://github.com/taisan11/oden/blob/main/rust-toolchain.toml)
で管理され、ネイティブライブラリ用のビルド環境も必要です。
大規模な Rust ワークスペースなので、初回のビルドには相応の
ディスク容量とメモリが必要です。

## 最初に追うべき実行経路

1. [`cli/main.rs`](https://github.com/taisan11/oden/blob/main/cli/main.rs)
   が `deno::main()` を呼び出す
2. [`cli/lib.rs`](https://github.com/taisan11/oden/blob/main/cli/lib.rs)
   が初期化・サブコマンド振り分けを担当する
3. [`cli/factory.rs`](https://github.com/taisan11/oden/blob/main/cli/factory.rs)
   が設定・ローダー・resolver などを組み立てる
4. [`runtime/worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker.rs)
   が extensions と `JsRuntime` の準備を進める
5. [`libs/core`](https://github.com/taisan11/oden/tree/main/libs/core)
   が JS エンジン・イベントループと native op の実行を支える

これは代表的な `run` 系の読み方です。
`fmt` などランタイムを起動しないツールもあり、全コマンドが
同じ経路を通るわけではありません。

## このドキュメントサイトをローカルで起動

Rust 製の Oden 本体のビルドとは独立しています。Node.js と npm を使用します。

```sh
cd dev_docs
npm install
npm run docs:dev
```

`npm run docs:build` で静的サイトを生成し、
`npm run docs:preview` でビルド結果を確認できます。
`base: "/oden/"` は GitHub Pages のプロジェクトサイトの公開先に合わせた設定です。

## 読む際の注意

元の [`doc/`](https://github.com/taisan11/oden/tree/main/doc) には
Deno upstream の大規模 CI を前提とした記述も残っています。
**現在有効な CI は `.github/workflows/*.yml` が一次情報**です。
また、Oden 独自機能は upstream Deno の公開 API と
互換性・安定性が同じとは限りません。

## 読む目的から選ぶ

| 目的 | 推奨する順序 |
| --- | --- |
| `oden run` の実行を追う | [CLI](/architecture/cli) → [ライフサイクル](/architecture/execution) → [Runtime](/architecture/runtime) |
| 独自 Web API を追加する | [Extensions・Ops・権限](/architecture/ops-and-permissions) → [変更箇所](/development/navigation) |
| 保存系を改修する | [Native KV](/features/kv) または [Turso SQLite](/features/sqlite) |
| Desktop の表示・IPC を調査する | [Desktop](/features/desktop) → [不具合調査](/development/debugging) |

最初からすべて読む必要はありません。問題が起きているレイヤーと、その境界だけを先に把握するのが近道です。
