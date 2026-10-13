# oden

oden is an experimental fork of [Deno](https://github.com/denoland/deno). It is
a JavaScript, TypeScript, and WebAssembly runtime built on
[V8](https://v8.dev/), [Rust](https://www.rust-lang.org/), and
[Tokio](https://tokio.rs/).

This project is experimental and is not an official Deno distribution. Features
and compatibility may change as the fork develops.

## Compatibility

The executable is **oden**. The `Deno` namespace, runtime APIs, environment
variables, and Rust crate names retain their existing names for compatibility.
Existing programs do not need to rename `Deno` to `oden`.

[`oden:sqlite` and `node:sqlite`](ext/sqlite/README.md) share Turso's Rust
engine. They provide Bun and Node API adapters with prepared statements,
transactions, BigInt/BLOB bindings, and database serialization. SQLite C API
features that are unavailable on this backend fail explicitly.

## Oden configuration

Oden discovers `oden.json` and `oden.jsonc` automatically. Generated project
files and npm dependency stores use `.oden` directories. The default global
cache uses `oden` under the platform cache directory, or `~/.oden` as a
fallback.

Run `oden from deno` in a project's directory to rename `deno.json` to
`oden.json`, or `deno.jsonc` to `oden.jsonc`. Configuration contents, comments,
and formatting are preserved. The command refuses to overwrite an existing Oden
configuration or to choose between two Deno configurations. Run it in each
workspace member directory as well. Run `oden install` after migration to
recreate dependency links, and `oden sync-types` to regenerate type
configuration under `.oden`.

## Build from source

Build this fork from this repository; upstream Deno installers and package
manager packages install Deno, not oden.

```sh
git clone --recurse-submodules https://github.com/taisan11/oden.git
cd oden
cargo build --locked -p deno --bin oden
```

See the
[contributing instructions](.github/CONTRIBUTING.md#building-from-source) for
the pinned Rust toolchain and native build prerequisites. The development binary
is available at `./target/debug/oden`.

## Your first program

Create a file called `server.ts`:

```ts
Deno.serve((_req: Request) => {
  return new Response("Hello, world!");
});
```

Run it with the development binary:

```sh
./target/debug/oden run --allow-net server.ts
```

The server listens on port 8000 by default.

## CI and releases

Default CI checks formatting, JavaScript/TypeScript lint and Cargo workspace
metadata. Full Rust builds are manual, and releases are drafted by a separate
manual workflow. Oden does not use Deno's publishing infrastructure.

See [workflow documentation](.github/WORKFLOWS.md) for commands and limitations.

## Documentation and contributing

- [Contributor guide](.github/CONTRIBUTING.md)
- [Internal architecture website (VitePress)](https://taisan11.github.io/oden/) ([source](dev_docs/README.md); available after GitHub Pages setup)
- [Architecture and development documentation](doc/README.md)
- [Upstream Deno documentation](https://docs.deno.com) for inherited APIs;
  experimental fork behavior may differ.
- [Deno standard library](https://jsr.io/@std)

## Attribution

oden derives from Deno. Original copyright notices and the
[MIT license](LICENSE.md) are retained.
