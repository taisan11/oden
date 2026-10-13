// ATTACH remains available with --allow-all. SQLite C limit controls are
// explicitly unsupported and are not needed to enable attachment.
import { DatabaseSync } from "node:sqlite";

try {
  new DatabaseSync("allowed/main.db", { limits: { attach: 1 } });
  throw new Error("Expected unsupported limits");
} catch (error) {
  if (error.code !== "ERR_SQLITE_UNSUPPORTED") throw error;
  console.log("limits-unsupported");
}
using db = new DatabaseSync("allowed/main.db");
try {
  db.limits.attach = 1;
  throw new Error("Expected unsupported limits");
} catch (error) {
  if (error.code !== "ERR_SQLITE_UNSUPPORTED") throw error;
  console.log("setter-unsupported");
}
db.exec("ATTACH DATABASE 'denied/secret.db' AS other");
console.log("attach:" + db.prepare("SELECT v FROM other.secrets").get().v);
