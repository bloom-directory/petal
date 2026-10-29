# Changelog

## Unreleased

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.
- Require `[sdk]` to be an exact pin, and refuse to build, check or package a
  Petal whose route crate, workspace, `[dependencies]` or lockfile pins the SDK
  or another crate from its repository differently, or overrides it.
- Add `petal package --no-build`, which packages the components a Petal's own
  build command produced, with any compile-time values it supplied. The
  release workflow uses it.

