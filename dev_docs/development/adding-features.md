# 新機能・コマンドの実装手順

Oden の実装変更では「表面上の API を追加する」だけでなく、**引数・解決・runtime・型定義・テスト**のどこまで変更する必要があるかを最初に切り分けます。以下はリポジトリの責務分割に合わせた作業手順です。

## どのレイヤーを変えるか

| 追加・修正したいもの | 主な変更先 | 併せて確認 |
| --- | --- | --- |
| CLI サブコマンド | `cli/args/`、`libs/cli_parser/`、`cli/lib.rs`、`cli/tools/` | help、spec |
| CLI フラグ | `cli/args/flags.rs`、`libs/cli_parser/src/` | default、設定との優先順位 |
| import 解決 | `cli/resolver.rs`、`libs/resolver/`、`cli/module_loader.rs` | npm/JSR、type checker、bundler |
| Web API | `ext/*`、`runtime/worker.rs` | JS API、op、permission、types |
| `Deno.*` API | `runtime/js/`、`runtime/ops/`、関連 `ext/` | bootstrap、宣言ファイル |
| Node 互換 API | `ext/node/`、`ext/node_crypto/` | CJS/ESM、Node テスト |
| SQLite API | `ext/sqlite/` | Bun 風 / Node 両アダプター、Turso 制約 |
| Desktop | `cli/tools/desktop.rs`、`cli/rt_desktop/`、`runtime/ops/desktop.rs` | ABI、イベント、HMR |
| LSP・型チェック | `cli/lsp/`、`cli/tsc/` | runtime の module scheme との一致 |

## 実例 A: CLI フラグを追加する

1. [`cli/args/flags.rs`](https://github.com/taisan11/oden/blob/main/cli/args/flags.rs) の既存フラグと [`libs/cli_parser/src/`](https://github.com/taisan11/oden/tree/main/libs/cli_parser/src) を確認する。
2. 使用する構造体・help・default の仕様を決める。`--help` に表示される説明も変更する。
3. 必要なら [`cli/args/mod.rs`](https://github.com/taisan11/oden/blob/main/cli/args/mod.rs) の `CliOptions` などに設定への反映処理を加える。
4. 対応する [`cli/tools/`](https://github.com/taisan11/oden/tree/main/cli/tools) の処理へ値を渡す。
5. CLI 引数と `oden.json` の両方から指定できる機能は優先順位をテストする。
6. 対象の [`tests/specs/`](https://github.com/taisan11/oden/tree/main/tests/specs) に正常・異常入力のケースを追加する。

## 実例 B: 新しい組込み JavaScript API を追加する

```text
API 仕様
  │
  ├─ JavaScript adapter + d.ts
  ├─ Rust op (#[op2]) + Extension
  ├─ OpState / ResourceTable / permission
  ├─ Worker + Web Worker + snapshot registration
  ├─ import 解決 / bundler / LSP (組込み module の場合)
  └─ unit test + spec / compatibility test
```

1. [`ext/`](https://github.com/taisan11/oden/tree/main/ext) に類似する extension がないかを探す。
2. JavaScript API の引数・戻り値・エラー・同期/非同期を明文化する。
3. Rust op は **ネイティブ操作の直前に権限確認**を行う。不要な権限を追加で要求しないこともテストする。
4. `ResourceTable` に登録する resource は解放後・エラー後の扱いまで設計する。
5. [`runtime/worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/worker.rs)、[`runtime/web_worker.rs`](https://github.com/taisan11/oden/blob/main/runtime/web_worker.rs) の登録と、ビルド時の snapshot を確認する。
6. 型宣言・tooling が必要なら [`cli/tsc/`](https://github.com/taisan11/oden/tree/main/cli/tsc) と [`cli/lsp/`](https://github.com/taisan11/oden/tree/main/cli/lsp) も更新する。

具体例は [`oden:sqlite` の Rust extension](https://github.com/taisan11/oden/blob/main/ext/sqlite/lib.rs) と [Ops・権限](/architecture/ops-and-permissions) を参照してください。

## 実例 C: 既存処理のバグ修正

「期待する動作」と「実際の動作」を最小ケースで再現してから、原因の層を特定します。

- URL / package 名の選択がおかしい → resolver / workspace
- URL は正しいが fetch できない → fetcher / permissions
- 読めた TS が実行できない → module loader / transpile
- 実行時の API が未定義 → extension / bootstrap / snapshot
- Node API と返却値が違う → Node adapter / compatibility test
- ウィンドウ操作だけ動かない → Desktop backend / event channel

修正を始める前に対応するテストを見つけ、できれば再現するテストを先に追加してください。

## PR 前のセルフチェック

- [ ] 実際のソースで使われている名前・責務に沿って修正している
- [ ] Cargo workspace・feature の構成を壊していない
- [ ] `oden --help` と使用例が矛盾していない
- [ ] 設定・型定義・LSP・bundler への影響を確認した
- [ ] 権限を不必要に緩めていない
- [ ] 正常系・権限拒否・解放時のテストを確認した
- [ ] 対象の unit/spec test を実行した（未実行は PR に記載）
- [ ] `dev_docs` と実装にある説明を同期した

CI はフォーマット等の軽量チェックが中心です。**CI が成功しても、すべての native runtime ビルドや互換性テストが通ったことにはなりません。** [テストと CI](/development/testing) で対象範囲を確認してください。

関連: [変更箇所を探す](/development/navigation) · [デバッグ](/development/debugging)
