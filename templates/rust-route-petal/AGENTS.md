# {{petal-name}} operating contract

- `/petals/{{petal-name}}/status.json` is a read-only health route with no side
  effects.
- `/petals/{{petal-name}}/account/<wallet>/<index>/address.json` reads the EVM
  address of that Bloom account, from Bloom's own wallet projection. It has
  no side effects.

## Development skill

Petals are authored against the `bloom-petal-development` skill. `petal new`
wrote it to `.agents/skills/bloom-petal-development/SKILL.md`, byte for byte
from the contract crate of the CLI that generated the scaffold, so it is
available offline and matches that CLI. Its directory carries the skill's
name, as the Agent Skills format requires. Load it into your agent before
extending this Petal. After an SDK upgrade, refresh it with
`scripts/petal.sh skill --root .`. It is part of the Petal package, so editing
or refreshing it changes the package hash; Bloom does not serve it under
`/petals/{{petal-name}}/`.
