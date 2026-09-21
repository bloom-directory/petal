# Changelog

## Unreleased

- `SdkError::Host` now carries the host's reason text alongside the coarse
  status. Routes that matched `SdkError::Host(HostStatus::NotFound)` become
  `err.is_not_found()`, or `SdkError::Host { status: HostStatus::NotFound, .. }`
  when the status is needed; `SdkError::host(status)` builds a reason-free one.

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.

