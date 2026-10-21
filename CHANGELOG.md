# Changelog

## Unreleased

- Classified host failures retain bounded detail in `SdkError::HostReason`.
  `message()` includes it and `host_status()` classifies both bare and detailed
  failures. Callers matching only `SdkError::Host(..)` should use that accessor.
  No global last-error slot or WIT change is required.

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.

