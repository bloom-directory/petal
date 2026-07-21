# Releasing Petals

The `petal` repository owns the package format and reusable release workflow.
Each individual Petal repository owns its source tag and GitHub Release. Bloom
is an installer and runtime: it does not compile Petal source during setup.

## Caller workflow

Add a thin workflow to the Petal repository. Pin both the reusable workflow and
the CLI tooling input to the same reviewed, full commit SHA:

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
      petal-tooling-ref: 0123456789abcdef0123456789abcdef01234567
      build-command: scripts/build.sh
      package-root: .
      expected-route-count: 12
```

The workflow only accepts semantic version tags and exact 40-character tooling
commits. It checks out the caller at the tagged commit, builds its routes,
packages them deterministically, and refuses to replace existing assets.

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

Release jobs need `contents: write`; all other permissions remain disabled. A
missing GitHub Release is created from the existing tag. An existing release is
accepted only when none of the three canonical asset names already exists.
