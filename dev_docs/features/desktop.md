# Desktop の起動構造と WebView 通信

`oden desktop` は、Deno 由来の runtime を **ネイティブ UI backend に埋め込む**実験的機能です。通常の `oden run` と異なり、ウィンドウを所有する実行ファイルは Laufey backend です。

## 逆転したホスト構造

```text
oden desktop / cli/tools/desktop.rs
        ↓ パッケージ生成
Laufey executable (CEF または OS WebView)
        ↓ --runtime <shared library>
libdenort (cli/rt_desktop, cdylib)
        ↓ Standalone payload / VFS を抽出
cli/rt/run.rs → Deno runtime
        ↓
loopback HTTP を起動 → WebView が localhost を表示
```

`cli/rt_desktop` が C ABI を公開する `libdenort` をビルドし、Laufey が共有ライブラリをロードします。アプリのエントリポイント・アセット等は `d3n0l4nd` section として同梱され、通常の standalone メタデータと仮想ファイルシステムへ展開されます。

関連ソース: [`cli/tools/desktop.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/desktop.rs)、[`cli/rt_desktop/lib.rs`](https://github.com/taisan11/oden/blob/main/cli/rt_desktop/lib.rs)。

## Backend の解決と整合性

CLI の `LaufeyBackendResolver` は開発用ディレクトリ（`LAUFEY_DEV_DIR`）、キャッシュ、ダウンロード済み配布物を扱います。ダウンロードの整合性は [`cli/laufey_sums.lock`](https://github.com/taisan11/oden/blob/main/cli/laufey_sums.lock) に固定した SHA-256 で検証します。

Laufey C ABI のバージョンはビルド時に検証され、backend と runtime の取り違えが起こりにくいようになっています。`oden.json` の desktop 設定は CLI flag で上書き可能で、未指定の backend は `webview` が既定値です。

## 起動順序が重要な理由

1. runtime 起動前にランダムな loopback port を選び、`DENO_SERVE_ADDRESS` を設定します。
2. 埋め込んだ仮想ファイルシステムを準備し、必要な作業ディレクトリを決定します。
3. Tokio 上で Oden runtime と Laufey event loop を並行して動かします。
4. Web サーバーが応答可能になるまで **HTTP GET** で検査し、成功後に初期ウィンドウを遷移させます。

環境変数やカレントディレクトリの変更はプロセス全体に影響するため、スレッド生成前に行う必要があります。単なる TCP 接続ではアプリが HTTP を返せる状態になったか判断できません。

## Web コンテンツと Native API は別経路

| 用途 | 経路 |
| --- | --- |
| HTML・CSS・JS、`fetch` | `http://127.0.0.1:<port>/` の loopback HTTP |
| `Deno.desktop` や native window events | `runtime/ops/desktop.rs` の event channel |
| `window.bindings.*` の呼び出し | 名前付き binding + request ID + oneshot 応答 |

`window.bind(name, fn)` で登録された処理を WebView から呼び出す場合、native 側が `call_id` を作り、`DesktopEvent::BindCall` を JS 側に渡します。JS handler の処理結果が `op_desktop_resolve_bind_call` または拒否 op に戻り、対応する oneshot に配送されます。

このため複数の binding が同時に実行されても、`call_id` で応答が区別されます。イベントの配送には容量制限があり、高頻度イベントは backpressure 時に間引かれます。**全イベントの配送を保証するキューではありません。**

## HMR・Inspector と終了処理

[`cli/rt/hmr.rs`](https://github.com/taisan11/oden/blob/main/cli/rt/hmr.rs) がソース監視・再読み込みに関与します。フレームワーク開発サーバーではブラウザー側の HMR を使う場合があり、通常の JS アプリとは処理が異なります。

`cli/rt_desktop/lib.rs` は runtime と native event loop の終了・ウィンドウ破棄・Inspector 接続なども扱います。Desktop の不具合調査は、**WebView の表示 / loopback HTTP / binding / runtime** のどこで停止したかを最初に分けるのが有効です。

より詳細な ABI・起動順序・通信の解説: [`doc/desktop-architecture.md`](https://github.com/taisan11/oden/blob/main/doc/desktop-architecture.md)。
