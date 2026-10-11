#!/usr/bin/env -S oden run --allow-write --allow-read --allow-net --allow-env --config=tests/config/oden.json
// Copyright 2018-2026 the Deno authors. MIT license.
import { getPrebuilt } from "./util.js";

const args = Deno.args.slice();
for (const arg of args) {
  await getPrebuilt(arg);
}
