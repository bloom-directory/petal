# Changelog

## Unreleased

- `petal::MessageReview` builds the `bloom.message.review.v1` envelope that
  Bloom's v0.5.0 release set requires in `action` before it signs an EIP-712
  or EIP-191 message, and checks its shape against Broker's limits.
  `sdk::sign_payload` now refuses such a message without that envelope before
  calling Bloom, with an error naming `MessageReview`, where Machine would
  refuse it as a bare `invalid`. Safe confirmations, other suites and
  payloads that do not start with `0x19` are unaffected. Released Bloom
  v0.4.2 accepts the envelope and ignores it.
- `sdk::describe_error(&err)` gives the error's message followed by the reason
  `host-error-detail` recorded for it, and is `message()` without the feature.
- Document installing the CLI from git, pinning the SDK by commit, signing
  messages, reading Bloom's refusal reasons, and enrolling a signing Petal's
  provenance on a new developer triad.

- Opt-in `host-error-detail`: `sdk::last_host_reason()` returns the host's
  explanation when the latest SDK host call failed. Every host call replaces
  it; a success clears it. Errors keep the bare `SdkError::Host(status)` variant,
  message and classification, so Petals that upgrade behave as before. No WIT
  change is required.
  Hosts remain responsible for removing credentials and internal request data
  before sending explanations; this is not a generic secret redactor.
  A reason is recorded after any recognized status label, paired with the
  classified status, so a `denied:` message that classifies as `NotFound`
  keeps its reason. Match refusal codes such as `APPROVAL_NOT_FOUND` in the
  reason, not the status.
  Local argument validation clears stale reasons. Store and address helpers
  append the current read's reason; Machine still drops -2 denial messages at IPC.

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.
- Take generated components' SDK from the route crate's own dependency,
  including renames, workspace inheritance and features. `[sdk]` is optional;
  when set it must match. The route SDK must be an exact pin, and `petal build`,
  `check` and `package` refuse a Petal whose route crate, workspace,
  `[dependencies]` or lockfile pins the SDK or another crate from its
  repository differently, a route crate that makes the SDK optional, a route
  crate or workspace that overrides any crate with `[patch]` or `[replace]`,
  and a lockfile that records a `[replace]` of any crate.
  `PETAL_REQUIRE_LOCKED_SOURCES=1`, which the release workflow sets, also
  refuses a Cargo config patch of the SDK, any Cargo config in the
  checkout and a `rust-toolchain` file that names a toolchain by path, and
  builds only from the tested lockfile: a package moved by a `paths`
  override or a source replacement in Cargo home's config is refused. The
  release workflow builds its tooling outside the Petal's checkout.
- Build components from the route workspace's committed `Cargo.lock`,
  refusing any change to its versions, sources, checksums or dependency
  edges. A new scaffold needs that lockfile before `petal build`.
- Make components reproducible across checkout directories: remap the
  checkout, Cargo home and toolchain paths, reach the route crate through a
  link inside the generated workspace so Cargo's symbol hashes do not depend
  on the checkout's location, strip symbols and use one codegen unit, with the
  release profile set on the Cargo command line and `CARGO_INCREMENTAL=0`.
- Replace the runtime output with a fresh build, moving the replaced output
  under `target/petal-routes/staging/` and removing it again only when it
  holds exactly the components an earlier build recorded writing to that
  output, ignoring Finder's `.DS_Store`. A build that stops with an error
  removes what it staged. `petal check` and `package --no-build` refuse a
  component with no route controller.
- `petal package` rebuilds current sources first, and its archive holds only
  what Bloom installs: `petal.toml`, `README.md`, `AGENTS.md`,
  `petal/<name>/`, `components/`, `modules/`, `wit/` and the development
  skill. It never packages an archive. Package hashes change accordingly.
- Add `petal package --no-build`, which packages the components a Petal's own
  build command produced, with any compile-time values it supplied. The
  release workflow uses it.

