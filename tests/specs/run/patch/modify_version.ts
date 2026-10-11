const data = JSON.parse(Deno.readTextFileSync("./add/oden.json"));
data.version = "2.0.0";
Deno.writeTextFileSync("./add/oden.json", JSON.stringify(data, null, 2));
