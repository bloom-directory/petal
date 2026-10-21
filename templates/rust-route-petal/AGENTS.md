# {{petal-name}} operating contract

`/petals/{{petal-name}}/status.json` is a read-only health route with no side
effects.

## Development skill

Petals are authored against the `bloom-petal-development` skill. `petal new`
wrote it as `SKILL.md` in this Petal's source root, byte for byte from the
contract crate of the CLI that generated the scaffold, so it is available
offline and matches that CLI. Load it into your agent before extending this
Petal. It is part of the Petal package, so editing or refreshing it changes the
package hash; Bloom does not serve it under `/petals/{{petal-name}}/`.

