// Verifies post-bump file contents.
console.log("--- a/oden.json ---");
console.log(Deno.readTextFileSync("a/oden.json").trim());
console.log("--- b/oden.json ---");
console.log(Deno.readTextFileSync("b/oden.json").trim());
console.log("--- import_map.json ---");
console.log(Deno.readTextFileSync("import_map.json").trim());
