# Bloom Petal

This repository is the release authority for the Bloom Petal component
contract, Rust SDK, route builder, CLI, templates, and conformance fixtures.

The route contract is `bloom:route@0.1.0`. Its canonical WIT is under
`wit/route/`; custody-aware Petals use `bloom:key/derive@0.1.0` and
payload-bearing `bloom:sign/signing@0.2.0`, including atomic ordered batches.

## Workspace

- `bloom-petal-contract` embeds the canonical WIT and exposes contract IDs,
  capability mappings, and a deterministic WIT digest.
- `bloom-petal-sdk` is imported as the Rust crate `petal` by route components.
- `bloom-petal-builder` discovers route files and builds deterministic WebAssembly
  components.
- `bloom-petal-cli` provides `petal build`, `petal check`, `petal package`,
  `petal inspect`, and `petal new`.

## Development

With Rust, the `wasm32-unknown-unknown` target, and jq installed:

```sh
scripts/install-tools.sh
scripts/check.sh
cargo run -p bloom-petal-cli -- inspect
```

The installer reads the `wasm-tools` pin from `[workspace.metadata.tools]` in
`Cargo.toml` and matches `wit-bindgen-cli` to the workspace `wit-bindgen`
dependency. It installs with `--locked` and verifies the executables on `PATH`.
Use `scripts/install-tools.sh --wasm-tools-only` when bindings are not needed.
Update the `wasm-tools` pin manually when upgrading it; Dependabot does not
track workspace metadata.

Regenerate the committed Rust bindings after changing WIT or `wit-bindgen`:

```sh
scripts/install-tools.sh
scripts/generate-bindings.sh
```

The generated SDK bindings are derived output. `wit/route` is the only
authoritative WIT tree.

Read the contract and WIT digest from `petal inspect` using the pinned tooling.
Release provenance and checksums must describe the WIT shipped with that release;
a copied digest in authoring documentation is not a compatibility oracle.

## Host error detail

Host calls return the same `SdkError::Host(status)` with or without the
`host-error-detail` feature, and `message()` is unchanged, so existing matches
keep working when the feature is enabled anywhere in the build. Classification
is the same as in earlier releases.

With the feature, `petal::sdk::last_host_reason()` returns the host's
explanation of the latest SDK host call, when that call failed with a host
message of the form `denied: <reason>`, `permission denied: <reason>`,
`not found: <reason>` or `invalid: <reason>`. The status is the error's
classification, which reads the whole message and can differ from the label:
a `denied:` message whose text contains `not found` is `NotFound`. To react to
a specific refusal, match the code in the reason, such as
`APPROVAL_NOT_FOUND`, not the status. Machine reports that code with messages
such as `denied: APPROVAL_NOT_FOUND: approval does not exist`, which are
`Denied`. The reason belongs to that call, not to the error value: every host
call replaces it, and a success or an error the SDK builds itself clears it.
Read it right after the failing call. No WIT change is required.

Detail is limited to 512 Unicode characters; control, bidi-control and
zero-width characters become spaces and a truncated explanation ends with `…`.
Hosts must supply caller-safe messages; length limits cannot redact unknown
credentials. Machine's HTTP URL redaction is tracked in
[Bloom #364](https://github.com/bloom-directory/bloom/pull/364). Adopt that host
fix before enabling richer errors in consumers that perform HTTP requests.

For the transport regression, run
`sh scripts/check-component-errors.sh "$MACHINE_CHECKOUT"` against a committed
Machine checkout. It builds the opted-in conformance component and checks two
distinct synthetic refusals through the actual Machine VM. It grants no signing
authority. The existing CI workflow exposes this check through manual dispatch
with one full `machine_ref`; ordinary CI also builds the opted-in component.

Current Machine converts a route's -2 response to message-less PermissionDenied,
so an agent using IPC still sees only "permission denied". The address helper
and conformance fixture use -2 for denials; VM-level evidence does not prove
IPC propagation. Machine's owner must confirm which exact-signing explanations
are safe to expose before a consumer routes them through another error code.

## Pinning the SDK

A Petal pins the SDK in the route crate's Cargo dependencies, including renamed
or workspace-inherited dependencies. Generated components use that same source
and feature selection. `[sdk]` in `petal-build.toml` is optional transitional
validation: remove it, or keep it equal to the route dependency.

The route SDK must be an exact pin: a git `rev` that is a full 40-character
commit, a version requirement such as `=0.1.0`, or a local `path`. Then
`petal build`, `petal check` and `petal package` refuse a Petal when:

- any SDK dependency of the route crate, in any dependency table or inherited
  from its workspace, differs from the route SDK pin, or `[sdk]` is set and
  differs from it;
- for a git pin, another crate from the SDK's repository, such as
  `bloom-petal-contract`, is pinned at a different revision, in the route crate
  or in `[dependencies]` of `petal-build.toml`;
- the lockfile resolves any of them from another source or revision;
- the route crate or its workspace overrides the SDK, or another crate from
  its repository, with `[patch]` or `[replace]`, which generated route
  components ignore;
- the lockfile records a `[replace]` of any crate, for the same reason.

The component build starts from the route workspace's committed lockfile and
resolves wrapper packages without accepting dependency graph changes. The
route workspace is the one Cargo finds for the route crate, which in a monorepo
can sit above the Petal root. Missing locked sources may be fetched; provision
them first to use Cargo's offline mode. Declare any additional wrapper
dependencies in the route workspace so route tests and generated components
exercise the same graph.

To upgrade, change the route SDK pin (and any remaining transitional pins),
then refresh the lockfile:

```sh
cargo update --manifest-path route/Cargo.toml -p bloom-petal-sdk
```

A local checkout can still be tested through a `[patch]` in
`.cargo/config.toml`, as this repository's consumer CI does. Cargo reads that
file from the directory `petal` runs in, not from `--root`, so run `petal`
from the Petal root for the patch, or any rustflags there, to apply. The pin
check accepts a lockfile that resolves the SDK to a local path only while
Cargo, run from that same directory, resolves the SDK to a local path, as such
a patch does wherever its config sets it, including through `include`, a
parent directory or Cargo home. `PETAL_REQUIRE_LOCKED_SOURCES=1` refuses that
exception. The build then also refuses any package that Cargo config moves
away from its locked source, which the lockfile cannot show: a `paths`
override, or a source replacement such as a vendored directory. Every package
the lockfile takes from git or a registry must be read from Cargo's own copy
in Cargo home. The release workflow sets it, so a committed Cargo config
cannot ship other code under the pins. Pins outside Cargo are not checked:
scripts that install the `petal` CLI, and the release workflow's `uses:`
reference and `petal-tooling-ref`. Keep them at the same commit.

## Packaging a Petal

Build current route sources and create a platform-neutral runtime archive:

```sh
petal package \
  --root . \
  --out dist/example-v0.1.0.petal.tar.gz
```

`petal package` rebuilds current sources and checks every generated component
before writing the archive. It cannot silently package an older build. A
rebuild sees only the current environment, so a Petal whose build command
supplies compile-time values, such as a key read with `option_env!`, runs that
command and then `petal package --no-build`, which checks the components it
produced and packages them as they are. The release workflow does this. It
refuses a component with no route controller, but cannot tell that sources
changed after the build, so run it straight after the build command. Machine
composition and installation still apply the host's authoritative checks. The
archive uses sorted strict ustar entries, normalized ownership, permissions and
timestamps, and a gzip header with a zero timestamp. Repeating the command over
the same clean package tree produces identical bytes. It refuses to overwrite
an existing output. It includes `petal.toml`, `README.md`, `AGENTS.md` and
the runtime tree `petal/<name>/`. When present, it also includes the pinned
development skill that `petal new` writes to
`.agents/skills/bloom-petal-development/SKILL.md`, and the optional
`components/`, `modules/` and `wit/` directories of Bloom's package format;
route sidecars compose from `components/` and `modules/`. Each is taken only
under exactly that name, and links and other non-regular files are refused.
Bloom refuses any other entry under `petal/`, such as a `.DS_Store`, so the
archive leaves it out. An archive is never packaged, and the output may not be
written inside a packaged tree. The configured output must be `petal/<name>`.
Unrelated development files do not change runtime identity; code, capabilities
and runtime documents do. This deliberately changes older source-tree package
hashes: publish a new immutable package and preserve its predecessor lineage.
Machine assigns a separate final identity when composing the installed package.

Component builds remap the checkout, Cargo home and Rust toolchain source
paths, omit symbol names and use one codegen unit at compile time. Multiple
units produced path-dependent empty-function ordering on Rust 1.99.1 even after
remapping. Two different checkout directories with identical compiler and
locked inputs produce the same tested components. The builder passes its
release profile on the Cargo command line and sets `CARGO_INCREMENTAL=0`,
which override `[profile.release]` and `build.incremental` in Cargo config
and `CARGO_PROFILE_RELEASE_*` variables; a per-package override, such as
`[profile.release.package."*"]`, still applies to the packages it names.

Each build moves the output it replaces to
`target/petal-routes/staging/<name>-<process>-<time>.previous` instead of
deleting it, and keeps it there; delete those directories when you no longer
need them.

The builder adds its flags to `CARGO_BUILD_RUSTFLAGS`, after any value set
there, and Cargo appends that to the configured `build.rustflags`. When a
remapped path contains whitespace, the flags go in a `--config` array
instead, which Cargo cannot merge into a string `build.rustflags`. With
`RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` set, Cargo ignores configured
rustflags and the builder adds its flags to `CARGO_ENCODED_RUSTFLAGS`.
Otherwise explicit target-specific Cargo rustflags keep their normal
precedence, which drops these flags; include equivalent path remapping if you
set them. Other paths, such as a path dependency on a local SDK checkout, are
not remapped.

Petal repositories should publish with the reusable workflow documented in
[`docs/releasing-petals.md`](docs/releasing-petals.md). The Petal repository
owns its release assets; Bloom only downloads and verifies them during setup.

## Distribution

Rust packages are published to crates.io at coordinated exact versions. GitHub
Releases carry the WIT archive, contract provenance, the Petal development
skill, checksums and the source archive. Git tags identify source provenance
but are not the terminal Cargo dependency channel.

`scripts/release-check.sh` runs the release gates and assembles the canonical
WIT archive, contract provenance, the Petal development skill, and SHA-256
checksums under `dist/`. Assembling them needs git 2.40 or later, for
`git archive --mtime`. A pushed version tag publishes those artifacts through
GitHub Releases. crates.io publication is performed in dependency order:
contract, SDK, builder, CLI.
