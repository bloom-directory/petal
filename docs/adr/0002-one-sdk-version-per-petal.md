# ADR 0002: One SDK version per Petal

## Status

Accepted for v0.2.0.

## Context

Before v0.2.0 no release was tagged, so a Petal pinned the SDK by commit, and
pinned it again in up to five places: the route crate, `[sdk]` in
`petal-build.toml`, its build script, its CI and its release workflow. Those
pins drifted apart, and three first-party Petals built with whatever `petal`
CLI was installed. `petal new` wrote an SDK requirement, `=0.1.0`, that no
registry provides.

## Decisions

- The SDK, builder and CLI share the workspace version and are released
  together as a `v<version>` tag. A tag is immutable once published.
- A Petal pins the SDK to a release tag. The builder accepts the tag as an
  exact pin because the lockfile records the commit it named, and refuses a
  lockfile that does not.
- GitHub serves a commit from any fork of the repository at its URL, so a
  lockfile can name a tag beside a commit the tag never named. The builder
  works offline and cannot tell; it only refuses a lockfile that records two
  commits for the repository. The scaffold's `scripts/petal.sh` and the
  reusable release workflow, which choose the CLI, refuse a tag pin whose
  commit is not the tag's, and the release workflow refuses any other commit
  it would build from, CLI or crate, that is not on `master` or a release
  tag.
- The CLI follows the Petal's SDK, not a separate pin. The builder refuses a
  lockfile that resolves the SDK at another version than its own. The
  scaffold's `scripts/petal.sh` and the reusable release workflow install the
  CLI from the lockfile's SDK commit.
- `petal new` writes the tag of the CLI that runs it, and a scaffold that
  builds as it is. CI scaffolds one and builds it.

## Consequences

- Upgrading a Petal is one edit, the tag, and `cargo update -p
  bloom-petal-sdk`.
- A release must bump the workspace version before tagging, and the release
  workflow checks that the tag matches.
- crates.io remains the terminal channel (ADR 0001). Publishing there turns
  the tag pin into `version = "=X.Y.Z"`, with the same version check.
