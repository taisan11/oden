#!/usr/bin/env -S oden run --allow-read

for (const item of Deno.readDirSync(".")) {
  console.log(item.name);
}
