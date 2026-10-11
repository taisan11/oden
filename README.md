# oden

oden is an experimental fork of [Deno](https://github.com/denoland/deno). It is
a JavaScript, TypeScript, and WebAssembly runtime built on
[V8](https://v8.dev/), [Rust](https://www.rust-lang.org/), and
[Tokio](https://tokio.rs/).

This project is experimental and is not an official Deno distribution. Features
and compatibility may change as the fork develops.

## Compatibility

The project brand is **oden**. The `Deno` namespace, runtime APIs, executable
name (`deno`), command-line interface, configuration filenames, environment
variables, and Rust crate names retain their existing names for compatibility.
Existing programs do not need to rename `Deno` to `oden`.

## Build from source

Build this fork from this repository; upstream Deno installers and package
manager packages install Deno, not oden.

```sh
git clone --recurse-submodules https://github.com/taisan11/oden.git
cd oden
cargo build --locked -p deno --bin deno
```

See the
[contributing instructions](.github/CONTRIBUTING.md#building-from-source) for
the pinned Rust toolchain and native build prerequisites. The development binary
is available at `./target/debug/deno`.

## Your first program

Create a file called `server.ts`:

```ts
Deno.serve((_req: Request) => {
  return new Response("Hello, world!");
});
```

Run it with the development binary:

```sh
./target/debug/deno run --allow-net server.ts
```

The server listens on port 8000 by default.

## Documentation and contributing

- [Contributor guide](.github/CONTRIBUTING.md)
- [Architecture and development documentation](doc/README.md)
- [Upstream Deno documentation](https://docs.deno.com) for inherited APIs;
  experimental fork behavior may differ.
- [Deno standard library](https://jsr.io/@std)

## Attribution

oden derives from Deno. Original copyright notices and the
[MIT license](LICENSE.md) are retained.
