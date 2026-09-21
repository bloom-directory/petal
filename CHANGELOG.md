# Changelog

## Unreleased

- Add `sdk::take_host_reason()`, which returns the reason the host gave for the
  most recent failed host call. `SdkError` is unchanged, so no route needs
  updating; a route that wants the reason reads it right after the failing
  call.

- Establish the canonical `bloom:route@0.1.0` contract.
- Extract the shared Rust SDK and route builder.
- Add the `petal` CLI and Rust Petal scaffold.

