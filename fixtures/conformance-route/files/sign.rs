fn sign(ctx: &petal::Ctx, body: &[u8]) -> Result<petal::DispatchResponse, petal::DispatchResponse> {
    let claim = petal::ClaimOrigin::from_ctx(ctx)?.claim(
        "conformance.sign",
        petal::CryptoSuite::Secp256k1Sha256Recoverable,
        &[petal::PayloadSignItem {
            preimage: body.to_vec(),
            claimed_hash: crate::sha256(body),
        }],
        &petal::ClaimEffects::moves_nothing(),
        petal::ClaimNonce::Derived,
    )?;
    let request = claim.sign_request("fixture", petal::SignSelector::Exact)?;
    petal::sdk::sign_payload(&request).map_err(|error| {
        let message = match petal::sdk::last_host_reason() {
            Some((_, reason)) => format!("{}: {reason}", error.message()),
            None => error.message(),
        };
        let code = match error {
            petal::SdkError::Host(petal::HostStatus::NotFound) => -1,
            petal::SdkError::Host(petal::HostStatus::Denied) => -2,
            petal::SdkError::Host(petal::HostStatus::Invalid) => -3,
            _ => -4,
        };
        petal::error(code, message)
    })?;
    Ok(petal::DispatchResponse::Write)
}

petal::route_file!(
    spec: petal::signing_write_spec("conformance.sign").caps(&["bloom:sign"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(b"write to exercise structured signing\n".to_vec()),
    write: |ctx: &petal::Ctx, body: &[u8]| sign(ctx, body).unwrap_or_else(|error| error)
);
