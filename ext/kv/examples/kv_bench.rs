// Copyright 2018-2026 the Deno authors. MIT license.
// Backend-only benchmark: no JavaScript/V8 calls are included in timings.
#![allow(
  clippy::disallowed_methods,
  clippy::print_stdout,
  reason = "benchmark manages temporary files and emits JSONL on stdout"
)]

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use deno_core::OpState;
use deno_kv::DatabaseHandler;
use deno_kv::native::NativeDbHandler;
use denokv_proto::*;
use serde_json::json;

const ITERATIONS: usize = 1000;
const REPEATS: usize = 5;
const VALUE_BYTES: usize = 256;

fn key(index: usize) -> Vec<u8> {
  (index as u32).to_be_bytes().to_vec()
}
fn request(index: usize) -> ReadRange {
  let start = key(index);
  let mut end = start.clone();
  end.push(0);
  ReadRange {
    start,
    end,
    limit: NonZeroU32::new(1).unwrap(),
    reverse: false,
  }
}
fn options() -> SnapshotReadOptions {
  SnapshotReadOptions {
    consistency: Consistency::Strong,
  }
}
fn mutation(index: usize, byte: u8) -> Mutation {
  Mutation {
    key: key(index),
    kind: MutationKind::Set(KvValue::Bytes(vec![byte; VALUE_BYTES])),
    expire_at: None,
  }
}
fn file_bytes(dir: &Path) -> u64 {
  std::fs::read_dir(dir)
    .unwrap()
    .map(|e| e.unwrap().metadata().unwrap().len())
    .sum()
}
async fn seed<D: Database>(db: &D, count: usize) {
  for start in (0..count).step_by(1000) {
    db.atomic_write(AtomicWrite {
      checks: vec![],
      mutations: (start..(start + 1000).min(count))
        .map(|i| mutation(i, 1))
        .collect(),
      enqueues: vec![],
    })
    .await
    .unwrap()
    .unwrap();
  }
  for i in 0..5 {
    assert_eq!(
      db.snapshot_read(vec![request(i)], options()).await.unwrap()[0]
        .entries
        .len(),
      1
    );
  }
}
async fn operation<D: Database>(db: &D, name: &str, count: usize, i: usize) {
  let index = (i * 17 + 13) % count;
  match name {
    "get" => {
      assert_eq!(
        db.snapshot_read(vec![request(index)], options())
          .await
          .unwrap()[0]
          .entries
          .len(),
        1
      );
    }
    "overwrite" => {
      db.atomic_write(AtomicWrite {
        checks: vec![],
        mutations: vec![mutation(index, i as u8)],
        enqueues: vec![],
      })
      .await
      .unwrap()
      .unwrap();
    }
    "cas" => {
      let read = db
        .snapshot_read(vec![request(index)], options())
        .await
        .unwrap();
      let version = read[0].entries[0].versionstamp;
      db.atomic_write(AtomicWrite {
        checks: vec![Check {
          key: key(index),
          versionstamp: Some(version),
        }],
        mutations: vec![mutation(index, i as u8)],
        enqueues: vec![],
      })
      .await
      .unwrap()
      .unwrap();
    }
    "list_100" => {
      let range = ReadRange {
        start: key(0),
        end: key(count),
        limit: NonZeroU32::new(100).unwrap(),
        reverse: false,
      };
      assert_eq!(
        db.snapshot_read(vec![range], options()).await.unwrap()[0]
          .entries
          .len(),
        100
      );
    }
    _ => panic!("unknown operation"),
  }
}
async fn measure<D: Database>(
  db: D,
  backend: &str,
  mode: &str,
  dir: &Path,
  count: usize,
  name: &str,
  repeat: usize,
) {
  seed(&db, count).await;
  let initial_bytes = file_bytes(dir);
  let mut latencies = Vec::new();
  let total = Instant::now();
  for i in 0..ITERATIONS {
    let start = Instant::now();
    operation(&db, name, count, i).await;
    latencies.push(start.elapsed().as_secs_f64() * 1000.0);
  }
  let elapsed = total.elapsed().as_secs_f64();
  let final_bytes = file_bytes(dir);
  println!(
    "{}",
    json!({ "kind": "workload", "backend": backend, "mode": mode, "entries": count, "value_bytes": VALUE_BYTES, "operation": name, "repeat": repeat, "iterations": ITERATIONS, "elapsed_s": elapsed, "ops_per_s": ITERATIONS as f64 / elapsed, "latency_ms": latencies, "initial_file_bytes": initial_bytes, "final_file_bytes": final_bytes })
  );
  db.close();
}
async fn growth<D: Database>(db: D, backend: &str, dir: &Path) {
  let count = 1000;
  seed(&db, count).await;
  let mut updates = 0;
  for target in [0, 10, 50, 100] {
    while updates < target {
      operation(&db, "overwrite", count, updates).await;
      updates += 1;
    }
    let mut reads = Vec::new();
    for i in 0..10 {
      let start = Instant::now();
      operation(&db, "get", count, i).await;
      reads.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let start = Instant::now();
    // A separate open of the same store measures journal replay on startup.
    let reopened = NativeDbHandler::new(Some(dir.to_path_buf()), None)
      .open(Rc::new(RefCell::new(OpState::new(None))), None)
      .await
      .unwrap();
    reopened
      .snapshot_read(vec![request(0)], options())
      .await
      .unwrap();
    reopened.close();
    println!(
      "{}",
      json!({ "kind": "growth", "backend": backend, "entries": count, "updates": target, "file_bytes": file_bytes(dir), "read_latency_ms": reads, "reopen_ms": start.elapsed().as_secs_f64() * 1000.0 })
    );
  }
  db.close();
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
  assert!(
    std::env::var("DENO_KV_DB_MODE").is_err(),
    "unset DENO_KV_DB_MODE for benchmarking"
  );
  let root =
    std::env::temp_dir().join(format!("deno-kv-bench-{}", std::process::id()));
  std::fs::create_dir(&root).unwrap();
  println!(
    "{}",
    json!({ "kind": "metadata", "debug_assertions": cfg!(debug_assertions), "profile": if cfg!(debug_assertions) { "debug" } else { "release" }, "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "iterations": ITERATIONS, "repeats": REPEATS, "value_bytes": VALUE_BYTES, "seed_batch": 1000, "counts": [100, 1000, 10000], "comparison": std::env::args().any(|arg| arg == "--compare-sqlite"), "backend_order": "alternated per repeat", "io": "OS file locks and fsync, warm OS cache; temporary overlay filesystem" })
  );
  let compare = std::env::args().any(|arg| arg == "--compare-sqlite");
  #[cfg(not(feature = "sqlite"))]
  assert!(
    !compare,
    "--compare-sqlite requires the sqlite Cargo feature"
  );
  for count in [100, 1000, 10000] {
    for mode in ["memory", "disk"] {
      for name in ["get", "overwrite", "cas", "list_100"] {
        for repeat in 0..REPEATS {
          // Alternate order to reduce systematic first/second backend bias.
          let backends = if !compare {
            vec!["native"]
          } else if repeat % 2 == 0 {
            vec!["native", "sqlite"]
          } else {
            vec!["sqlite", "native"]
          };
          for backend in backends {
            let dir =
              root.join(format!("{count}-{mode}-{backend}-{name}-{repeat}"));
            std::fs::create_dir(&dir).unwrap();
            let storage = if mode == "disk" {
              Some(dir.clone())
            } else {
              None
            };
            let state = Rc::new(RefCell::new(OpState::new(None)));
            if backend == "native" {
              let db = NativeDbHandler::new(storage, None)
                .open(state, None)
                .await
                .unwrap();
              measure(db, backend, mode, &dir, count, name, repeat).await;
            } else {
              #[cfg(feature = "sqlite")]
              {
                let db =
                  deno_kv::sqlite::SqliteDbHandler::new(storage, Some(42))
                    .open(state, None)
                    .await
                    .unwrap();
                measure(db, backend, mode, &dir, count, name, repeat).await;
              }
            }
            std::fs::remove_dir_all(dir).unwrap();
          }
        }
      }
    }
  }
  {
    let backend = "native";
    let dir = root.join(format!("growth-{backend}"));
    std::fs::create_dir(&dir).unwrap();
    let state = Rc::new(RefCell::new(OpState::new(None)));
    growth(
      NativeDbHandler::new(Some(dir.clone()), None)
        .open(state, None)
        .await
        .unwrap(),
      backend,
      &dir,
    )
    .await;
    std::fs::remove_dir_all(dir).unwrap();
  }
  std::fs::remove_dir(root).unwrap();
}
