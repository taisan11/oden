# Native KV と Deno KV の比較

Oden には **互換性を保つ Deno KV** と **Rust Native backend の Oden KV** が共存します。両者はメソッドや返り値の型を共有しますが、ファイル形式と永続化方式は別物です。

## API と保存先

| API | ローカル backend | 既定の保存ファイル | HTTP(S) |
| --- | --- | --- | --- |
| `Deno.openKv(path?)` | Deno 互換の SQLite backend | `kv.sqlite3` | KV Connect 対応 |
| `Oden.openKv(path?)` | 独自 Native journal | `kv.native` | 非対応 |

どちらも **`--unstable-kv` が必要**です。ファイルに対する権限確認も適用されます。

```ts
// kv.ts: メモリ DB の例
const denoKv = await Deno.openKv(":memory:");
const odenKv = await Oden.openKv(":memory:");
try {
  await odenKv.set(["counter"], new Deno.KvU64(1n));
  const entry = await odenKv.get<Deno.KvU64>(["counter"]);
  console.log(entry.value?.value);
} finally {
  denoKv.close();
  odenKv.close();
}
```

```sh
oden run --unstable-kv kv.ts
```

型として `Oden.Kv` / `Oden.KvU64` / `Oden.AtomicOperation` 等は対応する `Deno` 側と同じ公開クラスを指します。ただし Native journal を `Deno.openKv` で開いたり、SQLite DB を `Oden.openKv` で開いたりする変換機能はありません。

## Native backend の内部

[`ext/kv/native.rs`](https://github.com/taisan11/oden/blob/main/ext/kv/native.rs) の Native 保存方式は次の構成です。

```text
JS API (ext/kv/01_db.ts)
  ↓ op_kv_database_open_native
ext/kv/lib.rs → NativeDbHandler
  ↓
ordered in-memory index + TTL index
  ↕ (変更分だけ反映)
checksummed append-only delta journal
  ↕ file lock / sync_all
local filesystem
```

- 起動時に journal を再生してメモリ上の index を復元します。
- 書き込みは触れた key / queue message の差分を追記し、`sync_all` で永続化を行います。
- 他プロセスの変更は、ファイルのロック下で増分フレームを再生して取り込みます。
- TTL 用に別の順序付き index を使い、読み取りのたびに全キーを走査しません。
- メモリのみの操作では、mutex が即時取得できる場合に blocking worker への往復を避けます。

## atomic・queue・watch

Native backend は atomic check/mutation、TTL、数値演算、queue、watch に対応します。Queue の配送は再試行される場合があり、**重複配送を考慮した処理**が必要です。

Native 実装の watch と queue 消費は 20 ms 間隔の polling を利用し、queue 配送には更新可能な 5 秒の lease が存在します。これらは遅延・配送保証の評価に重要な値であり、強いリアルタイム性を約束するものではありません。

## 運用上の制限

- **compact 処理は未実装**で、更新履歴が増えるほど journal が大きくなります。
- 生存中の key の index はメモリに保持され、起動時には journal の再生が必要です。
- 1 transaction あたりのシリアライズ済みデータ上限は 256 MiB です。
- 過去の `DENOKV01` snapshot は読み込めますが、新しい差分 frame を書き込んだ DB を古い Oden が扱えるとは限りません。
- Unix では `O_NOFOLLOW`、Windows では親ディレクトリからの `NtCreateFile` ベースでパスを検証します。その他の OS では Native ファイル永続化を拒否します。
- SQLite → Native の自動移行はありません。ファイルロックと `fsync` の保証があるローカルファイルシステムを使ってください。

## 変更箇所とテスト

| 対象 | コード |
| --- | --- |
| JavaScript API | [`ext/kv/01_db.ts`](https://github.com/taisan11/oden/blob/main/ext/kv/01_db.ts) |
| op / handler 登録 | [`ext/kv/lib.rs`](https://github.com/taisan11/oden/blob/main/ext/kv/lib.rs) |
| Native journal | [`ext/kv/native.rs`](https://github.com/taisan11/oden/blob/main/ext/kv/native.rs) |
| Deno 互換 SQLite backend | [`ext/kv/sqlite.rs`](https://github.com/taisan11/oden/blob/main/ext/kv/sqlite.rs) |
| 実測性能の条件 | [`tests/bench/kv-native/README.md`](https://github.com/taisan11/oden/blob/main/tests/bench/kv-native/README.md) |

Native と SQLite は保存形式も性能特性も異なるため、性能差を評価するときは同じ操作・データ量・永続化条件で比較してください。

一次資料: [`ext/kv/README.md`](https://github.com/taisan11/oden/blob/main/ext/kv/README.md)
