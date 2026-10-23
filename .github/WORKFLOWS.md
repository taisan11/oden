# Oden automation

Oden is a small experimental Deno fork. CI is intentionally smaller than the
upstream Deno pipeline. None of these workflows publishes to Deno's S3 buckets,
Slack, npm, or crates.io.

## CI: `ci.yml`

Runs for pull requests and pushes to `main`, and can be started manually:

- **Format:** dprint checks the repository using `.dprint.json`, including the
  Rust formatting integration. The pinned Rust toolchain is installed.
- **JavaScript and TypeScript lint:** runs `tools/lint.js --js` with upstream
  Deno as a _tooling bootstrap_, not as an Oden runtime test.
- **Cargo workspace metadata:** checks that the Cargo manifest and lockfile are
  consistent without compiling Rust crates.

These are fast structural checks, **not runtime compatibility or build
verification**. The `tools/lint.js` workflow-generator check was removed because
workflows are plain YAML.

## Build: `build.yml`

Use **Actions → Build Oden → Run workflow** and select `debug` or `release`.

The workflow builds Linux x86_64 `oden` and Windows x86_64 `oden.exe` from
source, smoke-tests both executables, and uploads them as separate expiring
workflow artifacts. Windows builds use the MSVC target and a Windows runner.
Both jobs initialize only `tests/util/std`, not the WPT or Node.js
compatibility suites.

The build can use significant disk space and memory. GitHub-hosted runners may
run out of resources; if so, use a suitable self-hosted runner or build locally.
macOS, ARM64, full WPT, and full Node compatibility testing are not part of
this minimal automation. Windows has basic executable smoke tests, not a full
platform compatibility suite. Run platform-specific tests locally before
publishing binaries.

## Release: `release.yml`

The release workflow is **manual-only** and builds **Linux and Windows x86_64**:

1. Create and push an appropriately versioned Git tag, e.g. `v0.1.0`.
2. Use **Actions → Draft Oden Release → Run workflow**.
3. Supply the existing tag and run the workflow.
4. The workflow checks out that tag on Linux and Windows, builds and
   smoke-tests both binaries, then packages a Linux `.tar.gz` and Windows
   `.zip` (containing `oden.exe`) with SHA-256 checksums.
5. Once both builds succeed, one job assembles all assets into a **draft**
   GitHub Release. Review the draft assets and notes before publishing it
   manually.

The workflow does not publish packages, create version tags, sign binaries,
update an upgrade CDN, or automatically distribute builds. The Rust build may
require more runner resources than a standard GitHub-hosted machine provides.

## Developer docs: `dev-docs.yml`

The `dev_docs/` directory contains a standalone Japanese VitePress site. Pull
requests changing the site or its workflow run a Node.js-only docs build. Pushes
to `main` that modify those paths, and manual workflow dispatches, build and
deploy the static files to GitHub Pages. Rust is not compiled.

The VitePress base is `/oden/` and the uploaded artifact is
`dev_docs/.vitepress/dist`. One-time setup: in the repository's **Settings →
Pages → Build and deployment**, select **GitHub Actions** as the source. Then
the site can be published at `https://taisan11.github.io/oden/`.

## Local checks

- `./x fmt`: format sources
- `./x lint-js`: JavaScript/TypeScript lint
- `cargo metadata --locked --no-deps --format-version 1`: workspace validation
- `cargo build --locked -p deno --bin oden`: build debug Oden
- `./target/debug/oden eval 'console.log(1+2)'`: Linux smoke test
- `.\\target\\debug\\oden.exe eval "console.log(1+2)"`: Windows PowerShell smoke test

## Repository settings after merging

If branch protection or repository rulesets require the former `ci status`
check, switch them to the new `Format`, `JavaScript and TypeScript lint`, and
`Cargo workspace metadata` jobs. Also review any existing self-hosted runners,
release environments, secrets, and stale scheduled workflow runs. GitHub
settings cannot be changed by editing this repository's files.
