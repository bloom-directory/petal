fn request(
    ctx: &petal::Ctx,
    body: &[u8],
) -> Result<petal::DispatchResponse, petal::DispatchResponse> {
    let account = petal::wallet_account(ctx)?;
    let origin = petal::ClaimOrigin::from_ctx(ctx)?;
    let message = std::str::from_utf8(body)
        .map_err(|error| petal::error(-3, format!("message must be UTF-8: {error}")))?;
    let request = petal::evm_messages::EvmMessage::personal(message)?.prepare(
        &origin,
        account.wallet(),
        "conformance.personal",
        &petal::ClaimEffects::moves_nothing(),
        petal::ClaimNonce::Derived,
    )?;
    // The caller reads normal host approval state before retrying. This
    // fixture never approves, confirms or submits an operation itself.
    match petal::sdk::sign_payload(request.request()) {
        Ok(_) => Ok(petal::DispatchResponse::Write),
        Err(error) => Err(petal::error(-4, error.message())),
    }
}

petal::route_file!(
    spec: petal::signing_write_spec("conformance.personal").caps(&["bloom:sign"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(
        b"Write informational UTF-8 text to request an exact personal-message approval.\n".to_vec()
    ),
    write: |ctx: &petal::Ctx, body: &[u8]| request(ctx, body).unwrap_or_else(|error| error)
);
