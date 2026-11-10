# Releasing Petals

The `petal` repository owns the package format and reusable release workflow.
Each individual Petal repository owns its source tag and GitHub Release. Bloom
is an installer and runtime: it does not compile Petal source during setup.

## Caller workflow

Add a thin workflow to the Petal repository; `petal new` writes one. The
workflow builds and packages with the `petal` CLI of the SDK commit that the
route workspace's `Cargo.lock` pins, so the SDK pin selects the tooling. Pin
the reusable workflow itself to a reviewed, full commit SHA:

```yaml
name: Release Petal

on:
  push:
    tags: ['v*.*.*']

permissions: {}

jobs:
  release:
    permissions:
      contents: write
    uses: bloom-directory/petal/.github/workflows/release-petal.yml@0123456789abcdef0123456789abcdef01234567
    with:
      petal-name: example
      build-command: scripts/build.sh
      package-root: .
      expected-route-count: 12
```

The workflow only accepts semantic version tags. It runs three jobs, so
that no code from the Petal runs where the CLI that packages it is built or
run:

- `resolve` checks that the tag names the checked-out commit and chooses
  the tooling commit from the lockfile. It runs Cargo and Python outside the
  checkout, with the Rust toolchain named and Python isolated, so a
  `rust-toolchain` file or a module in the Petal cannot run first.
- `build` runs `build-command` with a CLI built from that commit, as
  `PETAL_BIN`, and uploads `petal/<name>/`. The build command can change
  anything else in its job, so the job can only read the repository.
- `package` builds the CLI again before it checks out the Petal, puts the
  uploaded `petal/<name>/` in place of the checkout's, runs
  `petal package --no-build`, and publishes the release. It refuses to
  replace existing assets.

So the archive takes `petal/<name>/` from the build command and every other
file from the tagged commit: commit `petal.toml`, `README.md`, `AGENTS.md`,
and any `components/`, `modules/` or `wit/` rather than generating them.
`petal-name` must be the name in `petal.toml`.

`petal-tooling-ref` is an exact 40-character commit that replaces the
lockfile's SDK commit as the tooling source. The workflow warns when the two
differ, and the builder refuses an SDK of another version than its CLI.
Without it, a lockfile that does not take the SDK from one
`bloom-directory/petal` commit is refused.

Omit it only once the SDK pin is 0.2.0 or later. The workflow runs
`petal package --no-build`, which the CLI of an earlier SDK commit may lack,
so a Petal on an earlier SDK sets `petal-tooling-ref` to a commit with this
workflow's features and pins `uses:` to the same commit.

GitHub serves a commit from any fork of `bloom-directory/petal` at that
repository's URL, so before building the CLI the workflow fetches the
repository's `master` and release tags (`v` and a number) and refuses:

- a lockfile that takes crates from the repository at more than one commit,
  however it spells the URL, since Cargo builds them all from one;
- a lockfile that pins a release tag at a commit the tag does not name;
- a lockfile `rev`, or a `petal-tooling-ref`, that is not on `master` or a
  release tag.

The tooling commit must include `scripts/install-tools.sh` and the
`[workspace.metadata.tools]` configuration in `Cargo.toml`. The `build` job
runs that checkout's installer before building the CLI, so the CLI source
and `wasm-tools` version come from the same pinned tooling revision.

`expected-route-count` is optional. A positive value protects against a release
that silently omits routes; zero disables the assertion.

## Published assets

For Petal `example` tagged `v0.1.0`, the workflow publishes these assets in the
caller repository:

- `example-v0.1.0.petal.tar.gz`: platform-neutral Bloom package.
- `SHA256SUMS`: filename-bound archive checksum.
- `petal-release.json`: machine-readable release provenance.

The provenance schema is `bloom.petal.release.v1` and binds the Petal name,
caller repository, exact source commit, release tag, archive filename and
SHA-256, Bloom package hash, and exact Petal tooling commit. Bloom can pin and
verify all of these values without cloning or building the source repository.

The caller grants `contents: write`, which only the `package` job uses;
`resolve` and `build` read the repository, and all other permissions remain
disabled. A missing GitHub Release is created from the existing tag. An
existing release is accepted only when none of the three canonical asset
names already exists.
