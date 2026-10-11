const newReq = Deno.args[0]?.trim();
if (!newReq) {
  throw new Error("Missing required argument");
}
const config = JSON.parse(Deno.readTextFileSync("oden.json"));
config.imports["@denotest/esm-basic"] = `npm:@denotest/esm-basic@${newReq}`;
Deno.writeTextFileSync("oden.json", JSON.stringify(config));
