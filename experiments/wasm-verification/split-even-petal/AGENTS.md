# split-even operating contract

`/petals/split-even/split/<total>.json` is a read-only, effect-free route. It
needs no capabilities. Invalid totals are `-3` (invalid input).

This Petal is a verification fixture. `route/src/split.rs` is the verified
target; its emitted function is identified by the core export
`bloom_verify_split_even`. Changing it, the SDK, the toolchain or the build
flags invalidates the recorded evidence.

## Development skill

Petals are authored against the `bloom-petal-development` skill. `petal new`
wrote it to `.agents/skills/bloom-petal-development/SKILL.md`, byte for byte
from the contract crate of the CLI that generated the scaffold. It is part of
the Petal package, so editing or refreshing it changes the package hash.
