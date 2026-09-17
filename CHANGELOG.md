# Changelog

## Unreleased

- `PetalKeyRequest` carries optional `approval_value_limits` (asset budgets the host seals into the key's reusable approval). Requests without budgets serialize unchanged; code that builds the struct literally adds `approval_value_limits: Vec::new()`.
- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.

