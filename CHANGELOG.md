# Changelog

## Unreleased

- `key_request_jcs` builds a key request with optional `approval_value_limits` (asset budgets the host seals into the key's reusable approval), and `sdk::request_key` accepts them. `PetalKeyRequest` is unchanged.
- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.

