#!/usr/bin/env -S oden run --config=tools/oden.json -A --lock=tools/deno.lock.json
// Copyright 2018-2026 the Deno authors. MIT license.
import { $ } from "./deps.ts";
import { DenoWorkspace } from "./deno_workspace.ts";

const workspace = await DenoWorkspace.load();

// create a release notes file for the GH release draft
$.path(DenoWorkspace.rootDirPath)
  .join("./target/release/release-notes.md")
  .writeTextSync(workspace.getReleasesMdFile().getLatestReleaseText().fullText);
