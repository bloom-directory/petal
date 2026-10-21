use sha2::{Digest as _, Sha256};

fn sign(ctx: &petal::Ctx, body: &[u8]) -> Result<petal::DispatchResponse, petal::DispatchResponse> {
    let claim = petal::ClaimOrigin::from_ctx(ctx)?.claim(
        "conformance.sign",
        petal::CryptoSuite::Secp256k1Sha256Recoverable,
        &[petal::PayloadSignItem {
            preimage: body.to_vec(),
            claimed_hash: Sha256::digest(body).into(),
        }],
        &petal::ClaimEffects::moves_nothing(),
        petal::ClaimNonce::Derived,
    )?;
    let request = claim.sign_request("fixture", petal::SignSelector::Exact)?;
    petal::sdk::sign_payload(&request).map_err(|error| {
        petal::error(
            match error.host_status() {
                Some(petal::HostStatus::NotFound) => -1,
                Some(petal::HostStatus::Denied) => -2,
                Some(petal::HostStatus::Invalid) => -3,
                _ => -4,
            },
            error.message(),
        )
    })?;
    Ok(petal::DispatchResponse::Write)
}

petal::route_file!(
    spec: petal::signing_write_spec("conformance.sign").caps(&["bloom:sign"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(b"write to exercise structured signing\n".to_vec()),
    write: |ctx: &petal::Ctx, body: &[u8]| sign(ctx, body).unwrap_or_else(|error| error)
);
