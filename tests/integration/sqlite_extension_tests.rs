// Copyright 2026 the Oden authors. MIT license.

use test_util::deno_cmd;

#[test_util::test]
fn sqlite_extension_unsupported() {
  let output = deno_cmd()
    .arg("eval")
    .arg(r#"import { DatabaseSync } from "node:sqlite";
using db = new DatabaseSync(":memory:");
for (const action of [() => db.loadExtension("not-a-library"), () => db.enableLoadExtension(true), () => new DatabaseSync(":memory:", { allowExtension: true })]) {
  try { action(); throw new Error("Expected unsupported extension error"); }
  catch (error) { if (error.code !== "ERR_SQLITE_UNSUPPORTED") throw error; }
}
console.log("extension-unsupported");"#)
    .output().unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  assert_eq!(
    String::from_utf8_lossy(&output.stdout).trim(),
    "extension-unsupported"
  );
}
