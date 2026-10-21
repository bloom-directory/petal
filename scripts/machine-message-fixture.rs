//! Test-only adapter: actual Machine package validation and WASM forwarding.
//! The host captures requests; it cannot authorize or produce a signature.
use async_trait::async_trait;
use bloom_petals::{
    EvmOutboxOutcome, EvmTransactionRequest, HostError, HostVfsEntry, NameRegistry,
    PayloadSignRequest, PetalHost, PetalRouter, PetalRunner, PetalStore, PetalVm, SignOutcome,
};
use bloom_vfs::{Handler, Vfs, path::VfsPath};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Capture(
    Mutex<Vec<PayloadSignRequest>>,
    Mutex<Vec<EvmTransactionRequest>>,
);

#[async_trait]
impl PetalHost for Capture {
    async fn vfs_lookup(&self, path: &str) -> Result<HostVfsEntry, HostError> {
        Err(HostError::NotFound(path.into()))
    }
    async fn vfs_read(&self, path: &str) -> Result<Vec<u8>, HostError> {
        if path == "wallets/alice/1/address.evm" {
            return Ok(std::env::var("PETAL_FIXTURE_SIGNER").unwrap().into_bytes());
        }
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
    async fn evm_tx_stage(
        &self,
        request: EvmTransactionRequest,
    ) -> Result<EvmOutboxOutcome, HostError> {
        self.1.lock().unwrap().push(request);
        Ok(EvmOutboxOutcome {
            outbox_id: "fixture-stage".into(),
            plan_md: "Captured only; no chain submission.".into(),
            approval_required: None,
        })
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
            "contract":bloom_petals::package::ROUTE_PACKAGE,
            "wit_digest":bloom_petals::package::contract_wit_digest(),
            "authority":"proposal only; trusted installer enrollment not exercised"
        }))
        .unwrap(),
    )
    .unwrap();

    // The normal validator, rather than SDK source heuristics, must reject
    // the compiled sign import when the package capability ceiling omits it.
    let manifest = root.join("petal.toml");
    let original = fs::read_to_string(&manifest).unwrap();
    let invalid = original.replace("\"bloom:sign\", ", "");
    assert_ne!(
        invalid, original,
        "capability mutation did not change the manifest"
    );
    fs::write(&manifest, invalid).unwrap();
    let refusal = bloom_petals::package::build_petal_package_dir(&root)
        .expect_err("compiled capability was not enforced")
        .to_string();
    fs::write(&manifest, original).unwrap();
    assert!(refusal.contains("component route import"), "{refusal}");
    assert!(
        refusal.contains("requires missing petal.toml cap bloom:sign"),
        "{refusal}"
    );
    fs::write(evidence.join("capability-refusal.txt"), refusal).unwrap();

    let state = evidence.join("machine-state");
    let store = PetalStore::open(state.join("store")).unwrap();
    store.install_prepared_petal_package(first).unwrap();
    let registry = Arc::new(NameRegistry::open(state.join("registry")).unwrap());
    let runner = PetalRunner::new(store, registry, PetalVm::new().unwrap());
    let host = Arc::new(Capture::default());
    let vfs = Vfs::builder()
        .mount("petals", Arc::new(PetalRouter::new(runner, host.clone())))
        .build();
    for format in ["personal", "typed"] {
        let path = VfsPath::parse(&format!("petals/conformance/{format}/alice/1/sign")).unwrap();
        let before = host.0.lock().unwrap().len();
        assert!(vfs.write(&path, b"arbitrary input").await.is_err());
        assert_eq!(host.0.lock().unwrap().len(), before);
        vfs.write(&path, b"sign").await.unwrap();
        vfs.write(&path, b"sign").await.unwrap();
    }
    let stage = VfsPath::parse("petals/conformance/stage/alice/1/self-call").unwrap();
    assert!(vfs.write(&stage, b"").await.is_err());
    assert!(
        host.1.lock().unwrap().is_empty(),
        "missing chain reached the host"
    );
    vfs.write(&stage, b"polygon").await.unwrap();
    {
        let transactions = host.1.lock().unwrap();
        assert_eq!(transactions.len(), 1);
        let tx = &transactions[0];
        assert_eq!(tx.chain, "polygon");
        assert_eq!(tx.to, std::env::var("PETAL_FIXTURE_SIGNER").unwrap());
        assert_eq!(tx.value_wei, "0");
        assert_eq!(tx.data_hex, "0x");
        assert_eq!(
            tx.context
                .as_ref()
                .unwrap()
                .trusted_account()
                .unwrap()
                .number,
            1
        );
    }
    let requests = host.0.lock().unwrap();
    assert_eq!(requests.len(), 4);
    let mut fixtures = Vec::new();
    for (pair, class) in requests
        .chunks_exact(2)
        .zip(["conformance.personal", "conformance.typed"])
    {
        let request = &pair[0];
        assert_eq!(request, &pair[1], "retry changed signing identity");
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
        assert_eq!(request.operation_class, class);
        let claim: serde_json::Value =
            serde_json::from_slice(&request.petal_use_claim_jcs).unwrap();
        assert_eq!(
            claim["package_hash"],
            request.context.as_ref().unwrap().package_hash
        );
        assert_eq!(claim["route"], request.context.as_ref().unwrap().route_id);
        let witness: serde_json::Value =
            serde_json::from_slice(request.action.as_ref().unwrap()).unwrap();
        fixtures.push(serde_json::json!({
            "preimage_hex":hex::encode(&request.preimage),
            "hash_hex":hex::encode(request.claimed_hash), "witness":witness
        }));
    }
    fs::write(
        evidence.join("producer.json"),
        serde_json::to_vec_pretty(&fixtures).unwrap(),
    )
    .unwrap();
}
