# Desktop の内部アーキテクチャ

`oden desktop` は通常の JS 実行とは異なり、**ネイティブウィンドウのバックエンドが Oden runtime を共有ライブラリとしてロードする**構成です。実装の入口は [`cli/tools/desktop.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/desktop.rs)、詳しい設計メモは [`doc/desktop-architecture.md`](https://github.com/taisan11/oden/blob/main/doc/desktop-architecture.md) にあります。

## プロセスと責任の分離

```text
oden desktop (準備・パッケージ生成)
     │
     ├── launcher / native backend (laufey または laufey_webview)
     │       │
     │       ├── native windows / event loop
     │       ├── WebView  → http://127.0.0.1:<port>/
     │       └── C ABI で runtime をロード
     │
     └── libdenort (cli/rt_desktop, cdylib)
             │
             ├── bundled application + virtual filesystem
             ├── denort runtime + Tokio / V8
             ├── Deno.serve / fetch handler
             └── Deno.desktop と window APIs
```

通常の CLI では `oden` 自身がプロセスの入口です。Desktop の起動後は backend がホストとなり、`libdenort` はその中で動く runtime になります。

## コンポーネントの対応表

| 場所 | 役割 |
| --- | --- |
| [`cli/tools/desktop.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/desktop.rs) | CLI フラグ、backend 解決、パッケージ、起動・HMR supervisor |
| [`cli/rt_desktop/Cargo.toml`](https://github.com/taisan11/oden/blob/main/cli/rt_desktop/Cargo.toml) | `denort_desktop` crate と `libdenort` の `cdylib` 定義 |
| [`cli/rt_desktop/lib.rs`](https://github.com/taisan11/oden/blob/main/cli/rt_desktop/lib.rs) | Laufey ABI、イベント転送、ウィンドウと runtime の協調 |
| [`runtime/ops/desktop.rs`](https://github.com/taisan11/oden/blob/main/runtime/ops/desktop.rs) | ネイティブ連携 op、イベント、bind 呼び出しの pending response |
| [`cli/rt/desktop.rs`](https://github.com/taisan11/oden/blob/main/cli/rt/desktop.rs) | `DESKTOP_JS`、`Deno.BrowserWindow` / `Deno.desktop` |
| [`cli/rt/hmr.rs`](https://github.com/taisan11/oden/blob/main/cli/rt/hmr.rs) | ソース監視、V8 Inspector を使ったスクリプト差し替え |
| [`cli/tools/desktop_devtools.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/desktop_devtools.rs) | runtime と renderer の DevTools 接続 |

## Backend と ABI ハンドシェイク

`libdenort` は [`cli/rt_desktop/Cargo.toml`](https://github.com/taisan11/oden/blob/main/cli/rt_desktop/Cargo.toml) の `crate-type = ["cdylib"]` により共有ライブラリとして生成されます。backend は `--runtime` で指定された共有ライブラリをロードし、Laufey の C ABI を介して初期化・開始・終了を管理します。

組み込み API バージョンと backend バイナリの版に整合性が必要です。backend は pin されたバージョンと [`cli/laufey_sums.lock`](https://github.com/taisan11/oden/blob/main/cli/laufey_sums.lock) のチェックサムを使います。開発中の backend を調べる際は、[`doc/desktop-architecture.md`](https://github.com/taisan11/oden/blob/main/doc/desktop-architecture.md) にある `LAUFEY_DEV_DIR`、cache、download の優先順位も確認してください。

## ページ配信と Native Binding は別経路

**画面の HTML/JS:** WebView は loopback HTTP にアクセスし、Deno 側の `Deno.serve` 等がコンテンツを提供します。起動時はサーバーの準備を確かめてからウィンドウを遷移させます。

**Native API:** ウィンドウイベントや JS の binding 呼び出しは HTTP に混ぜず、channel と op を通します。

```text
WebView: window.bindings.someAction(args)
    │
    ▼
Laufey async binding → call_id を発行
    │
    ▼
DesktopEvent::BindCall ── mpsc ──► Deno 側イベントループ
                                      │
                                   JS handler
                                      │
    ◄──────── oneshot response ◄──────┘
```

複数の呼び出しが重なるため、`call_id` で pending response を対応付けます。頻繁に発生する pointer move・wheel などは、backpressure 時にメモリーを無制限に増やさず drop する処理があります。このため「すべての mouse move イベントが必ず届く」と想定した設計は避けます。

主な確認先は [`runtime/ops/desktop.rs`](https://github.com/taisan11/oden/blob/main/runtime/ops/desktop.rs) の `DesktopEvent`・`register_bind_call`、[`cli/rt/desktop.rs`](https://github.com/taisan11/oden/blob/main/cli/rt/desktop.rs) の `DESKTOP_JS` です。

## HMR の 3 つの結果

Desktop の HMR は [`cli/rt/hmr.rs`](https://github.com/taisan11/oden/blob/main/cli/rt/hmr.rs) と V8 Inspector の `Debugger.setScriptSource` を使って変更を分類します。

| 結果 | 実際の処理 | 典型例 |
| --- | --- | --- |
| `Replaced` | 動作中のスクリプトを差し替える | V8 が受け付ける関数本体等の変更 |
| `SoftReload` | ウィンドウのページを再読み込み | アセット変更・モジュール削除など |
| `Restart` | runtime ごと再起動 | import やトップレベルの構造変更でパッチ不可 |

再起動が必要な場合は、実行中の runtime が特別な終了コードを返し、Desktop supervisor が子プロセスを起動し直します。開発サーバーを使うフレームワークでは独自 HMR に委ねる分岐もあるため、すべての変更が Oden 側の HMR に通るわけではありません。

## 起動・終了の調査ポイント

1. **ウィンドウが表示されない:** backend 実行ファイル、共有ライブラリの配置、ABI 整合性を確認。
2. **白い画面:** loopback の HTTP レスポンス、初回 navigation、アプリ側のサーバー起動を確認。
3. **Binding が返らない:** `DesktopEvent::BindCall`、`call_id`、oneshot 完了・reject を追う。
4. **HMR が効かない:** framework dev の分岐か、V8 差し替え不可で restart に分類されているかを確認。
5. **終了後にプロセスが残る:** backend のイベントループ、runtime の終了、監視 supervisor を分けて調査。

この機能は実験的です。公開 API の安定性やプラットフォームの対応状況は、実装・関連テストを基準にしてください。

関連: [独自機能](/features/oden) · [実行ライフサイクル](/architecture/execution-flow) · [デバッグ](/development/debugging)
