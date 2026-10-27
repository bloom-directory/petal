# Changelog

## Unreleased

- Opt-in `host-error-detail`: `sdk::last_host_reason()` returns the host's
  explanation when the latest SDK host call failed. Every host call replaces
  it; a success clears it. Errors keep the bare `SdkError::Host(status)` variant,
  message and classification, so Petals that upgrade behave as before. No WIT
  change is required.
  Hosts remain responsible for removing credentials and internal request data
  before sending explanations; this is not a generic secret redactor.

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.

