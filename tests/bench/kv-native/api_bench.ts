// Copyright 2018-2026 the Deno authors. MIT license.
// Run with: oden run --unstable-kv -A tests/bench/kv-native/api_bench.ts
// deno-lint-ignore-file no-console
// --compare-sqlite adds a validation baseline; default output is Native only.
const iterations = 1000;
const repeats = 5;
const count = 1000;
const compare = Deno.args.includes("--compare-sqlite");
const dir = await Deno.makeTempDir({ prefix: "kv-api-bench-" });
console.log(JSON.stringify({
  kind: "metadata",
  api: true,
  iterations,
  repeats,
  entries: count,
  value_bytes: 256,
  runtime: Deno.version,
  build: "debug CLI",
  comparison: compare,
  backend_order: "alternated per repeat",
}));
try {
  for (const mode of ["memory", "disk"]) {
    for (const operation of ["get", "overwrite", "cas", "list_100"]) {
      for (let repeat = 0; repeat < repeats; repeat++) {
        const backends = !compare
          ? ["native"]
          : repeat % 2 === 0
          ? ["native", "sqlite"]
          : ["sqlite", "native"];
        for (const backend of backends) {
          const path = mode === "memory"
            ? ":memory:"
            : `${dir}/${mode}-${operation}-${repeat}-${backend}`;
          const db = backend === "native"
            ? await Oden.openKv(path)
            : await Deno.openKv(path);
          try {
            const value = new Uint8Array(256).fill(1);
            for (let start = 0; start < count; start += 100) {
              const atomic = db.atomic();
              for (let i = start; i < Math.min(start + 100, count); i++) {
                atomic.set(["bench", i], value);
              }
              const commit = await atomic.commit();
              if (!commit.ok) throw new Error("seed failed");
            }
            const run = async (i: number) => {
              const key = ["bench", (i * 17 + 13) % count];
              if (operation === "get") {
                const row = await db.get(key);
                if (row.value === null) throw new Error("missing entry");
              } else if (operation === "overwrite") {
                const result = await db.set(key, value);
                if (!result.ok) throw new Error("write failed");
              } else if (operation === "cas") {
                const row = await db.get(key);
                const result = await db.atomic().check(row).set(key, value)
                  .commit();
                if (!result.ok) throw new Error("CAS failed");
              } else {
                let rows = 0;
                for await (
                  const _ of db.list({ prefix: ["bench"] }, { limit: 100 })
                ) rows++;
                if (rows !== 100) throw new Error("wrong list length");
              }
            };
            for (let i = 0; i < 100; i++) await run(i);
            const samples = [];
            const total = performance.now();
            for (let i = 0; i < iterations; i++) {
              const start = performance.now();
              await run(i);
              samples.push(performance.now() - start);
            }
            const elapsed = (performance.now() - total) / 1000;
            console.log(JSON.stringify({
              kind: "workload",
              backend,
              mode,
              entries: count,
              operation,
              repeat,
              iterations,
              value_bytes: 256,
              latency_ms: samples,
              elapsed_s: elapsed,
              ops_per_s: iterations / elapsed,
            }));
          } finally {
            db.close();
          }
        }
      }
    }
  }
} finally {
  await Deno.remove(dir, { recursive: true });
}
