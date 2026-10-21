fn request(
    ctx: &petal::Ctx,
    body: &[u8],
) -> Result<petal::DispatchResponse, petal::DispatchResponse> {
    if body != b"sign" {
        return Err(petal::error(
            -3,
            "write exactly sign for the informational example",
        ));
    }
    let account = petal::wallet_account(ctx)?;
    let origin = petal::ClaimOrigin::from_ctx(ctx)?;
    let request = petal::evm_messages::EvmMessage::personal("Bloom component é")?.prepare(
        &origin,
        account.wallet(),
        "conformance.personal",
        &petal::ClaimEffects::moves_nothing(),
        petal::ClaimNonce::Derived,
    )?;
    // This informational example acknowledges the host result. The owner
    // projection exposes approval/completion; protocol adapters consume the
    // returned signature for their own operation rather than discarding it.
    match petal::sdk::sign_payload(request.request()) {
        Ok(_) => Ok(petal::DispatchResponse::Write),
        Err(error) => Err(petal::error(-4, error.message())),
    }
}

petal::route_file!(
    spec: petal::signing_write_spec("conformance.personal").caps(&["bloom:sign"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(
        "Informational message: Bloom component é\nWrite exactly sign. The owner reads petal-signing-requests/ for approval and completion, then retries sign after approval.\n".as_bytes().to_vec()
    ),
    write: |ctx: &petal::Ctx, body: &[u8]| request(ctx, body).unwrap_or_else(|error| error)
);
