# ADR 0001: Petal contract, SDK, builder, and release boundary

## Status

Accepted for the initial v0.1.0 release candidate.

## Decisions

- `bloom-directory/petal` is the sole authority for Petal WIT, SDK bindings,
  builder behavior, templates, and conformance fixtures.
- The initial WIT contract is `bloom:route@0.1.0` and the sole signing import is
  `bloom:sign/signing@0.1.0`. Multi-version support is deferred until another
  WIT version is proposed.
- The SDK contains contract-level helpers only. Domain-specific route policy
  remains in each Petal.
- The builder compiles route source. Bloom separately validates, hashes, and
  archives completed packages.
- crates.io is the terminal Rust distribution channel. Git dependencies are
  permitted only during release-candidate bootstrap.
- GitHub Releases distribute WIT archives, checksums, and optional binaries.
- Released versions are immutable. A broken crates.io version may be yanked,
  but is never overwritten or reused; a replacement version and explanation
  must be published.

## Release authority

The `bloom-directory` organization owns GitHub releases. Before v0.1.0, the
organization must assign crates.io ownership for every package and configure a
protected release environment using crates.io trusted publishing or a scoped
release token. The release workflow must link crate versions, Git tag, source
commit, WIT digest, and artifact checksums.

