petal::route_file!(
    spec: petal::signing_write_spec("conformance.sign").caps(&["bloom:sign"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(b"write to exercise structured signing\n".to_vec()),
    write: |_ctx: &petal::Ctx, body: &[u8]| {
        match petal::sdk::sign_payload(&petal::PayloadSignRequest {
            wallet: "fixture".into(),
            preimage: body.to_vec(),
            claimed_hash: [0_u8; 32],
            signature_algorithm: "secp256k1-keccak256-recoverable".into(),
            operation_class: "conformance.sign".into(),
            petal_use_claim_jcs: b"{}".to_vec(),
            claim_assurance_evidence: None,
            approval_hint: None,
            action: None,
            advisory: None,
            selector: petal::SignSelector::Exact,
            key_ref_jcs: None,
        }) {
            Ok(_) => petal::DispatchResponse::Write,
            Err(error) => petal::DispatchResponse::Error {
                code: 500,
                message: error.message(),
            },
        }
    }
);
