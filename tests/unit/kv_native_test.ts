// Copyright 2018-2026 the Deno authors. MIT license.
import { assert, assertEquals, assertRejects } from "./test_util.ts";

Deno.test("native KV persists values and checks across independent opens", async () => {
  const path = await Deno.makeTempFile();
  try {
    const a = await Oden.openKv(path);
    const b = await Oden.openKv(path);
    try {
      await a.set(["entry"], { bytes: new Uint8Array([1, 2]), n: 42n });
      const before = await b.get(["entry"]);
      assertEquals(before.value, { bytes: new Uint8Array([1, 2]), n: 42n });
      await a.set(["entry"], "updated");
      const conflict = await b.atomic().check(before).set(["other"], 1)
        .commit();
      assertEquals(conflict.ok, false);
      assertEquals((await a.get(["other"])).value, null);
    } finally {
      a.close();
      b.close();
    }
    const reopened = await Oden.openKv(path);
    try {
      assertEquals((await reopened.get(["entry"])).value, "updated");
    } finally {
      reopened.close();
    }
    const bytes = await Deno.readFile(path);
    assertEquals(new TextDecoder().decode(bytes.subarray(0, 8)), "DENOKV01");
  } finally {
    await Deno.remove(path);
  }
});

Deno.test("native KV recovers an interrupted trailing write", async () => {
  const path = await Deno.makeTempFile();
  try {
    let db = await Oden.openKv(path);
    await db.set(["entry"], 1);
    db.close();
    await Deno.writeFile(path, new Uint8Array([1, 2, 3]), { append: true });
    db = await Oden.openKv(path);
    try {
      assertEquals((await db.get(["entry"])).value, 1);
      await db.set(["entry"], 2);
    } finally {
      db.close();
    }
    db = await Oden.openKv(path);
    try {
      assertEquals((await db.get(["entry"])).value, 2);
    } finally {
      db.close();
    }
  } finally {
    await Deno.remove(path);
  }
});

Deno.test("native KV rejects SQLite files without modifying them", async () => {
  const path = await Deno.makeTempFile();
  try {
    const original = new TextEncoder().encode("SQLite format 3\0");
    await Deno.writeFile(path, original);
    await assertRejects(
      () => Oden.openKv(path),
      TypeError,
      "require migration",
    );
    assertEquals(await Deno.readFile(path), original);
  } finally {
    await Deno.remove(path);
  }
});

Deno.test("native KV rejects checksum corruption", async () => {
  const path = await Deno.makeTempFile();
  try {
    const db = await Oden.openKv(path);
    await db.set(["entry"], 1);
    db.close();
    const bytes = await Deno.readFile(path);
    assert(bytes.length > 48);
    bytes[16] ^= 0xff;
    await Deno.writeFile(path, bytes);
    await assertRejects(
      () => Oden.openKv(path),
      TypeError,
      "checksum mismatch",
    );
  } finally {
    await Deno.remove(path);
  }
});

Deno.test("Deno and Oden KV use separate backends and shared public types", async () => {
  const dir = await Deno.makeTempDir();
  const sqlitePath = `${dir}/compatible.sqlite3`;
  const nativePath = `${dir}/native.kv`;
  try {
    const compatible = await Deno.openKv(sqlitePath);
    const native = await Oden.openKv(nativePath);
    try {
      assert(compatible instanceof Deno.Kv);
      assert(native instanceof Deno.Kv);
      assert(native instanceof Oden.Kv);
      assertEquals(Oden.KvU64, Deno.KvU64);
      assertEquals(Oden.AtomicOperation, Deno.AtomicOperation);
      assertEquals(Oden.KvListIterator, Deno.KvListIterator);
      await compatible.set(["same"], "sqlite");
      await native.set(["same"], "native");
      assertEquals((await compatible.get(["same"])).value, "sqlite");
      assertEquals((await native.get(["same"])).value, "native");
    } finally {
      compatible.close();
      native.close();
    }
    assertEquals(
      new TextDecoder().decode(
        (await Deno.readFile(sqlitePath)).subarray(0, 16),
      ),
      "SQLite format 3\0",
    );
    assertEquals(
      new TextDecoder().decode(
        (await Deno.readFile(nativePath)).subarray(0, 8),
      ),
      "DENOKV01",
    );
    await assertRejects(() => Oden.openKv(sqlitePath));
    await assertRejects(() => Deno.openKv(nativePath));
    const reopenedCompatible = await Deno.openKv(sqlitePath);
    const reopenedNative = await Oden.openKv(nativePath);
    try {
      assertEquals((await reopenedCompatible.get(["same"])).value, "sqlite");
      assertEquals((await reopenedNative.get(["same"])).value, "native");
    } finally {
      reopenedCompatible.close();
      reopenedNative.close();
    }
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});

Deno.test("Oden.openKv is available in workers", async () => {
  const source = `
    const db = await Oden.openKv(":memory:");
    await db.set(["worker"], "native");
    const entry = await db.get(["worker"]);
    db.close();
    postMessage(entry.value);
    close();
  `;
  const url = URL.createObjectURL(
    new Blob([source], { type: "text/javascript" }),
  );
  const worker = new Worker(url, { type: "module" });
  try {
    const value = await new Promise((resolve, reject) => {
      worker.onmessage = (event) => resolve(event.data);
      worker.onerror = (event) => reject(new Error(event.message));
    });
    assertEquals(value, "native");
  } finally {
    worker.terminate();
    URL.revokeObjectURL(url);
  }
});
