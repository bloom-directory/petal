petal::route_file!(
    spec: petal::signing_write_spec("conformance.sign").caps(&["bloom:sign"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(b"write to exercise structured signing\n".to_vec()),
    write: |_ctx: &petal::Ctx, body: &[u8]| {
        let mut hash32 = [0_u8; 32];
        let copied = body.len().min(hash32.len());
        hash32[..copied].copy_from_slice(&body[..copied]);
        match petal::sdk::sign_hash(&petal::SignRequest {
            wallet: "fixture".into(),
            hash32,
            purpose: "conformance.sign".into(),
        }) {
            Ok(_) => petal::DispatchResponse::Write,
            Err(error) => petal::DispatchResponse::Error {
                code: 500,
                message: error.message(),
            },
        }
    }
);
