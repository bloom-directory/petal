petal::route_file!(
    spec: petal::signing_write_spec("conformance.message").caps(&["bloom:sign"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(b"write a message to sign\n".to_vec()),
    write: |ctx: &petal::Ctx, body: &[u8]| {
        use petal::signing::{PreparedAction, ReviewArtifact, SigningPayload, sign};
        let result = (|| {
            let payload = SigningPayload::ed25519_message(body.to_vec())?;
            let review = ReviewArtifact::new("conformance.message", 1, body.to_vec())?;
            let action = PreparedAction::new("conformance.message", payload, review)?;
            sign(ctx, "fixture", &action, "message-1")
        })();
        match result {
            Ok(_) => petal::DispatchResponse::Write,
            Err(error) => petal::DispatchResponse::Error { code: 500, message: error.to_string() },
        }
    }
);
