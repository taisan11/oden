# Drafting an Oden release

The Deno upstream release scripts in `tools/release/` are legacy tooling and
are **not** invoked automatically by Oden's GitHub workflows.

1. Test the intended commit and note any unsupported platforms.
2. Create and push a tag (for example `v0.1.0`).
3. In [GitHub Actions](https://github.com/taisan11/oden/actions),
   manually run **Draft Oden Release** and enter the existing tag.
4. Verify the Linux x86_64 artifact and checksum attached to the draft.
5. Edit the draft's release notes and publish it when ready.

See [workflow documentation](../.github/WORKFLOWS.md) for supported platforms,
limitations, and CI guidance.
