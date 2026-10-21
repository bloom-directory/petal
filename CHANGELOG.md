# Changelog

## Unreleased

- Opt-in `host-error-detail` retains classified host failures in `SdkError::HostReason`.
  `message()` includes it and `host_status()` classifies both bare and detailed
  failures. Migrate `SdkError::Host(..)` matches to that accessor before enabling
  the feature. Default builds retain the original variants and status behavior.
  No global last-error slot or WIT change is required.
  Hosts remain responsible for removing credentials and internal request data
  before sending explanations; this is not a generic secret redactor.

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.

