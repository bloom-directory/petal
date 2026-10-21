//! Public synthetic errors through an actual compiled SDK component and Machine VM.
use async_trait::async_trait;
use bloom_petals::{
    Capability, DispatchOp, DispatchRequest, DispatchResponse, HostError, HostVfsEntry,
    PayloadSignRequest, PetalHost, PetalVm, RunOptions, SignOutcome,
};
use std::sync::Arc;

struct RefusingHost;

#[async_trait]
impl PetalHost for RefusingHost {
    async fn vfs_lookup(&self, _: &str) -> Result<HostVfsEntry, HostError> {
        panic!("signing fixture must not look up VFS entries")
    }
    async fn vfs_read(&self, _: &str) -> Result<Vec<u8>, HostError> {
        panic!("signing fixture must not read VFS entries")
    }
    async fn vfs_list(&self, _: &str) -> Result<Vec<HostVfsEntry>, HostError> {
        panic!("signing fixture must not list VFS entries")
    }
    async fn vfs_write(&self, _: &str, _: &[u8]) -> Result<(), HostError> {
        panic!("signing fixture must not write VFS entries")
    }
    async fn sign_payload_outcome(
        &self,
        request: PayloadSignRequest,
    ) -> Result<SignOutcome, HostError> {
        assert_eq!(request.operation_class, "conformance.sign");
        match request.preimage.as_slice() {
            b"cancelled" => Err(HostError::Denied("owner cancelled this request".into())),
            b"malformed" => Err(HostError::Invalid("message evidence is malformed".into())),
            _ => panic!("unexpected public fixture"),
        }
    }
}

#[tokio::test]
async fn compiled_sdk_preserves_each_host_error_reason() {
    let wasm = std::fs::read(std::env::var_os("PETAL_ERROR_COMPONENT").unwrap()).unwrap();
    let vm = PetalVm::new().unwrap();
    let mut messages = vec![];
    for (body, expected_code, expected) in [
        ("cancelled", -2, "denied: owner cancelled this request"),
        ("malformed", -3, "invalid: message evidence is malformed"),
    ] {
        let output = vm
            .dispatch_component_route(
                &wasm,
                DispatchRequest {
                    op: DispatchOp::Write,
                    path: "sign".into(),
                    body: body.as_bytes().to_vec(),
                    ctx: vec![("bloom.route_id".into(), "r000001".into())],
                },
                [Capability::Sign].into(),
                Arc::new(RefusingHost),
                &"11".repeat(32),
                "conformance",
                vec![],
                RunOptions::default(),
            )
            .await
            .unwrap();
        match output.response {
            DispatchResponse::Error { code, message } => {
                assert_eq!(code, expected_code);
                assert_eq!(message, expected);
                messages.push(message);
            }
            _ => panic!("compiled route must return its host refusal"),
        }
    }
    // The later operation cannot overwrite the earlier returned explanation.
    assert_eq!(messages[0], "denied: owner cancelled this request");
}
