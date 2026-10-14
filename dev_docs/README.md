# Oden internal developer docs (VitePress)

This directory is the source for the Japanese Oden internals website.

- Site entry: `index.md`
- VitePress config: `.vitepress/config.mts`
- Architecture: `architecture/`
- Oden-specific features: `features/`
- Development: `development/`
- GitHub Pages workflow: `../.github/workflows/dev-docs.yml`

## Run locally

Requires Node.js 20 or later and npm.

```sh
cd dev_docs
npm install
npm run docs:dev
npm run docs:build
npm run docs:preview
```

The VitePress base is `/oden/` for project Pages at
`https://taisan11.github.io/oden/`. Build output is
`dev_docs/.vitepress/dist`; generated assets and `node_modules` are ignored.

## Deploy

The GitHub Actions workflow builds this site for relevant pull requests.
A push to `main` (or a manual dispatch) builds and deploys it.
Set **Settings → Pages → Source → GitHub Actions** once before deploying.
The repository must allow the `github-pages` environment/deployment.

This site documents current Oden internals. Keep code references accurate
and update existing guidance when implementation changes. In particular,
the inherited `doc/ci.md` describes older upstream CI and is not the
source of truth for this fork's workflow configuration.

Further deep-dive pages cover the CLI execution lifecycle, native ops and
permissions, Native KV, Turso SQLite, Desktop, and debugging. When changing
source code, update both the overview and the relevant deep-dive page.
