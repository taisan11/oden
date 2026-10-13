# Oden 独自機能と upstream との差分

Oden は Deno の実験的なフォークです。
すべての公開 API が新名称になったわけではありません。
**実行ファイルは `oden`、JS のグローバル名前空間は `Deno`** です。

## ブランド・設定の変更

| 項目 | Oden 側の扱い |
| --- | --- |
| 実行ファイル | `oden` |
| 自動検出する設定 | `oden.json`、`oden.jsonc` |
| ローカル生成ディレクトリ | `.oden` |
| グローバルキャッシュ | プラットフォームキャッシュ配下の `oden` など |
| JS 名前空間 | `Deno` のまま |
| 多くの Rust crate 名 | `deno` / `deno_runtime` / `deno_core` のまま |

ソース：[`README.md`](https://github.com/taisan11/oden/blob/main/README.md)。

### `oden from deno`

[`cli/tools/from_deno.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/from_deno.rs)
はカレントディレクトリの `deno.json` または `deno.jsonc` を
対応する `oden.*` にリネームします。JSONC のコメントや
フォーマットを保つため、内容を再パースして書き換えません。

- 元ファイルがない・両方ある・宛先が存在する場合は拒否
- 各 workspace member は必要に応じて個別に実行する
- 移行後は `oden install`、`oden sync-types` を適宜実行する

```sh
oden from deno
oden install
oden sync-types
```

## Turso ベースの SQLite

[`ext/sqlite`](https://github.com/taisan11/oden/tree/main/ext/sqlite)
は `turso_core` を使用する共通 Rust バックエンドです。
`oden:sqlite` は Bun に近い `Database` API、
`node:sqlite` は Node の `DatabaseSync` / `StatementSync`
API を提供します。

```ts
import { Database } from "oden:sqlite";

using db = new Database(":memory:");
db.exec("CREATE TABLE notes (message TEXT)");
db.query("INSERT INTO notes VALUES (?)").run("hello");
console.log(db.query("SELECT * FROM notes").all());
```

Rust 側の op、権限チェック、prepared statement は共通です。
**SQLite C API の完全互換ではありません。**
拡張ロードや一部の open flags などの制限は
[`ext/sqlite/README.md`](https://github.com/taisan11/oden/blob/main/ext/sqlite/README.md)
で確認してください。

## `oden desktop`

Desktop 機能はネイティブウィンドウと Oden runtime を組み合わせます。

- [`cli/tools/desktop.rs`](https://github.com/taisan11/oden/blob/main/cli/tools/desktop.rs)
  がパッケージングと backend 解決を管理
- [`cli/rt_desktop`](https://github.com/taisan11/oden/tree/main/cli/rt_desktop)
  は `libdenort` 共有ライブラリ（`cdylib`）を提供
- `laufey` のネイティブ backend が共有ライブラリをロード
- WebView コンテンツはループバック HTTP、ネイティブ binding は
  event channel など別経路で通信
- [`runtime/ops/desktop.rs`](https://github.com/taisan11/oden/blob/main/runtime/ops/desktop.rs)
  と [`cli/rt/desktop.rs`](https://github.com/taisan11/oden/blob/main/cli/rt/desktop.rs)
  が連携処理に関与

設計の詳細は
[`doc/desktop-architecture.md`](https://github.com/taisan11/oden/blob/main/doc/desktop-architecture.md)
を参照してください。この機能は現時点で実験的です。

## V8 / QuickJS バックエンド

[`cli/Cargo.toml`](https://github.com/taisan11/oden/blob/main/cli/Cargo.toml)
の標準 feature は `v8` です。
[`libs/deno_v8/Cargo.toml`](https://github.com/taisan11/oden/blob/main/libs/deno_v8/Cargo.toml)
で `v8` / `quickjs` feature を切り替える facade が定義されています。
両方同時、または両方無効はコンパイルエラーになる設計です。

QuickJS は実験的な経路です。標準の V8 build と
同じ動作・互換性・速度を提供するという意味ではありません。

## 他の注目箇所

- [`cli/schemas/`](https://github.com/taisan11/oden/tree/main/cli/schemas):
  設定・権限等の JSON schema
- [`tests/specs/from/`](https://github.com/taisan11/oden/tree/main/tests/specs/from):
  設定移行の spec テスト
- [`tests/specs/`](https://github.com/taisan11/oden/tree/main/tests/specs):
  新機能の CLI テストを追加する場所

新機能の改修時には upstream との差分だけでなく、
既存 Deno 互換テストへの影響も確認してください。
