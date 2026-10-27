# Changelog

## Unreleased

- Opt-in `host-error-detail` records the host's explanation of the latest
  classified failure in `sdk::last_host_reason()`; `SdkError::detail()` appends
  it to `message()`. Errors keep the bare `SdkError::Host(status)` variant and
  message, so existing matches are unaffected. No WIT change is required.
  Hosts remain responsible for removing credentials and internal request data
  before sending explanations; this is not a generic secret redactor.

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.

