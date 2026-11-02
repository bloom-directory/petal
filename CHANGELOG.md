# Changelog

## Unreleased

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.
- Take generated components' SDK from the route crate's own dependency,
  including renames, workspace inheritance and features. `[sdk]` is optional;
  when set it must match. The route SDK must be an exact pin, and `petal build`,
  `check` and `package` refuse a Petal whose route crate, workspace,
  `[dependencies]` or lockfile pins the SDK or another crate from its
  repository differently, or overrides it with `[patch]` or `[replace]`, and
  a lockfile that records a `[replace]` of any crate.
  `PETAL_REQUIRE_LOCKED_SOURCES=1`, which the release workflow sets, also
  refuses a Cargo config patch of the SDK, and any locked package that Cargo
  config moves away from its locked source, such as through a `paths`
  override or a vendored source replacement.
- Build components from the route workspace's committed `Cargo.lock`,
  refusing any change to its versions, sources, checksums or dependency
  edges. A new scaffold needs that lockfile before `petal build`.
- Make components reproducible across checkout directories: remap the
  checkout, Cargo home and toolchain paths, strip symbols and use one codegen
  unit, with the release profile set on the Cargo command line and
  `CARGO_INCREMENTAL=0`.
- Replace the runtime output with a fresh build, keeping the replaced output
  under `target/petal-routes/staging/`. `petal check` and
  `package --no-build` refuse a component with no route controller.
- `petal package` rebuilds current sources first, and its archive holds only
  what Bloom installs: `petal.toml`, `README.md`, `AGENTS.md`,
  `petal/<name>/`, `components/`, `modules/`, `wit/` and the development
  skill. It never packages an archive. Package hashes change accordingly.
- Add `petal package --no-build`, which packages the components a Petal's own
  build command produced, with any compile-time values it supplied. The
  release workflow uses it.

