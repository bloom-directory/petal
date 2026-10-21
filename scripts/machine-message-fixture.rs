//! Test-only adapter: actual Machine package validation and WASM forwarding.
//! The host captures requests; it cannot authorize or produce a signature.
use async_trait::async_trait;
use bloom_petals::{
    HostError, HostVfsEntry, NameRegistry, PayloadSignRequest, PetalHost, PetalRouter, PetalRunner,
    PetalStore, PetalVm, SignOutcome,
};
use bloom_vfs::{Handler, Vfs, path::VfsPath};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Capture(Mutex<Vec<PayloadSignRequest>>);

#[async_trait]
impl PetalHost for Capture {
    async fn vfs_lookup(&self, path: &str) -> Result<HostVfsEntry, HostError> {
        Err(HostError::NotFound(path.into()))
    }
    async fn vfs_read(&self, path: &str) -> Result<Vec<u8>, HostError> {
        if path == "wallets/alice/1/account.json" {
            return Ok(serde_json::to_vec(&serde_json::json!({
                "schema":"bloom.account.v1", "wallet":"alice", "number":1,
                "freshness":"fresh", "evm":{"state":"active",
                "public_key_fingerprint":"01".repeat(32)}, "solana":{"state":"missing"}
            }))
            .unwrap());
        }
        Err(HostError::NotFound(path.into()))
    }
    async fn vfs_list(&self, path: &str) -> Result<Vec<HostVfsEntry>, HostError> {
        Err(HostError::NotFound(path.into()))
    }
    async fn vfs_write(&self, path: &str, _: &[u8]) -> Result<(), HostError> {
        Err(HostError::Denied(path.into()))
    }
    async fn sign_payload_outcome(
        &self,
        request: PayloadSignRequest,
    ) -> Result<SignOutcome, HostError> {
        self.0.lock().unwrap().push(request);
        Ok(SignOutcome::ApprovalPending(
            bloom_petals::abi::ApprovalPending {
                action_id: "55".repeat(32),
                expires_ms: u64::MAX,
            },
        ))
    }
}

#[tokio::test]
async fn compiled_constructor_reaches_actual_machine_host() {
    let evidence = PathBuf::from(std::env::var_os("PETAL_COMPONENT_EVIDENCE").unwrap());
    let root = evidence.join("package-a");
    let first = bloom_petals::package::build_petal_package_dir(&root).unwrap();
    let second =
        bloom_petals::package::build_petal_package_dir(evidence.join("package-b")).unwrap();
    assert_eq!(
        first.hash, second.hash,
        "Machine composition differs between directories"
    );
    let mut archive = Vec::new();
    first.write_petal_tar(&mut archive).unwrap();
    let mut other = Vec::new();
    second.write_petal_tar(&mut other).unwrap();
    assert_eq!(
        archive, other,
        "Machine archives differ between directories"
    );
    fs::write(evidence.join("composed.petal.tar"), archive).unwrap();
    let routes: Vec<_> = first
        .route_index
        .routes
        .iter()
        .map(|route| {
            let mut classes = route.key_derive_operation_classes.clone();
            if let Some(class) = &route.install_metadata.sign_intent {
                classes.push(class.clone());
            }
            classes.sort();
            classes.dedup();
            serde_json::json!({"route_id":route.route_id, "pattern":route.pattern,
            "operation_classes":classes})
        })
        .collect();
    fs::write(
        evidence.join("composition.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "package_hash":first.hash, "routes":routes,
            "authority":"proposal only; trusted installer enrollment not exercised"
        }))
        .unwrap(),
    )
    .unwrap();

    // The normal validator, rather than SDK source heuristics, must reject
    // the compiled sign import when the package capability ceiling omits it.
    let manifest = root.join("petal.toml");
    let original = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        original.replace("allowed = [\"bloom:sign\"]", "allowed = []"),
    )
    .unwrap();
    let refused = bloom_petals::package::build_petal_package_dir(&root).is_err();
    fs::write(&manifest, original).unwrap();
    assert!(refused, "compiled capability was not enforced");

    let state = evidence.join("machine-state");
    let store = PetalStore::open(state.join("store")).unwrap();
    store.install_prepared_petal_package(first).unwrap();
    let registry = Arc::new(NameRegistry::open(state.join("registry")).unwrap());
    let runner = PetalRunner::new(store, registry, PetalVm::new().unwrap());
    let host = Arc::new(Capture::default());
    let vfs = Vfs::builder()
        .mount("petals", Arc::new(PetalRouter::new(runner, host.clone())))
        .build();
    let path = VfsPath::parse("petals/conformance/personal/alice/1/sign").unwrap();
    vfs.write(&path, "Bloom component é".as_bytes())
        .await
        .unwrap();
    vfs.write(&path, "Bloom component é".as_bytes())
        .await
        .unwrap();
    let requests = host.0.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let request = &requests[0];
    assert_eq!(request, &requests[1], "retry changed signing identity");
    assert_eq!(
        request
            .context
            .as_ref()
            .unwrap()
            .trusted_account()
            .unwrap()
            .number,
        1
    );
    assert_eq!(request.operation_class, "conformance.personal");
    let claim: serde_json::Value = serde_json::from_slice(&request.petal_use_claim_jcs).unwrap();
    assert_eq!(
        claim["package_hash"],
        request.context.as_ref().unwrap().package_hash
    );
    assert_eq!(claim["route"], request.context.as_ref().unwrap().route_id);
    let witness: serde_json::Value =
        serde_json::from_slice(request.action.as_ref().unwrap()).unwrap();
    fs::write(
        evidence.join("producer.json"),
        serde_json::to_vec_pretty(&serde_json::json!([{
            "preimage_hex":hex::encode(&request.preimage),
            "hash_hex":hex::encode(request.claimed_hash), "witness":witness
        }]))
        .unwrap(),
    )
    .unwrap();
}
