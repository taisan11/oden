// Confirms --dry-run did not modify files.
console.log("--- a/oden.json ---");
console.log(Deno.readTextFileSync("a/oden.json").trim());
console.log("--- b/oden.json ---");
console.log(Deno.readTextFileSync("b/oden.json").trim());
