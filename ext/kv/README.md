# deno_kv

This crate provides a key/value store for Deno. For an overview of Deno KV,
please read the [manual](https://docs.deno.com/deploy/kv/manual).

## APIs and Storage Backends

The runtime exposes two distinct entry points with the same KV operations and
return types. Both require `--unstable-kv` and enforce file permissions.

| API                  | Local backend                | Default file | HTTP(S)       |
| -------------------- | ---------------------------- | ------------ | ------------- |
| `Deno.openKv(path?)` | SQLite, compatible with Deno | `kv.sqlite3` | KV Connect    |
| `Oden.openKv(path?)` | Rust Native, without SQLite  | `kv.native`  | Not supported |

```ts
const compatible = await Deno.openKv("compatible.sqlite3");
const native = await Oden.openKv("native.kv");
await compatible.set(["key"], "value");
await native.set(["key"], "value");
compatible.close();
native.close();
```

`Oden.Kv`, `Oden.KvU64`, `Oden.AtomicOperation`, and `Oden.KvListIterator` refer
to the same public classes as their Deno counterparts. Files and default storage
are separate: opening a SQLite file with `Oden.openKv`, or a native journal with
`Deno.openKv`, fails without converting the data. Existing `kv.sqlite3`
databases remain accessible through `Deno.openKv()` and do not prevent creating
a separate `kv.native` database with `Oden.openKv()`.

The SQLite backend is implemented by the
[denokv_sqlite crate](https://github.com/denoland/denokv/blob/main/sqlite). The
runtime enables the `sqlite` Cargo feature for Deno compatibility. Embedders can
build `deno_kv` without that feature and supply their own compatibility and
native handlers through the extension options.

The Native backend uses an in-memory ordered index and a checksummed,
append-only delta journal. Commits use OS file locks and `sync_all`; independent
opens observe each other's commits. Atomic writes, TTL, numeric mutations,
queues, and watches are supported. Watches and queue consumers poll every 20 ms.
Queue deliveries have a renewable five-second lease and may be redelivered.
`:memory:` and `DENO_KV_DB_MODE=memory` use isolated in-memory stores for either
API.

Each operation checks the file under an OS lock and replays only newly appended
frames. Unchanged files need no replay. A commit writes only touched keys and
queue messages, and failed mutations undo only those changes. An ordered TTL
index avoids scanning all keys on reads. In-memory operations avoid copying the
database or dispatching to a blocking worker when the mutex is available.
Persistent file I/O and locking run on Tokio blocking workers.

Existing `DENOKV01` snapshot journals remain readable and can receive new delta
frames; older binaries cannot read those new frames. Each transaction has a 256
MiB serialized limit. There is no compaction yet: startup replays the journal
once, its size grows with updates, and the full live index resides in memory.
Use a local filesystem with reliable locking and fsync semantics. Automatic
migration from SQLite is not implemented.
[Native benchmark results and comparison ratios](../../tests/bench/kv-native/README.md)
describe the measured conditions and limits.

Additional backends can implement the `Database` trait.

## KV Connect

The KV Connect protocol allows the Deno CLI to communicate with a remote KV
database. The
[specification for the protocol](https://github.com/denoland/denokv/blob/main/proto/kv-connect.md),
and the
[protobuf definitions](https://github.com/denoland/denokv/blob/main/proto/schema/datapath.proto)
can be found in the `denokv` repository, under the `proto` directory.
