# byte-sum operating contract

`/petals/byte-sum/sum/<text>.json` is a read-only, effect-free route that needs
no capabilities. `route/src/sum.rs` is the verified target, identified by the
core export `bloom_verify_byte_sum`. Changing it, the SDK, the toolchain or the
build flags invalidates the proof.

## Development skill

Petals are authored against the `bloom-petal-development` skill in
`.agents/skills/bloom-petal-development/SKILL.md`.
