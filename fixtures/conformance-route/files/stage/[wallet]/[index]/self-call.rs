fn stage(
    ctx: &petal::Ctx,
    body: &[u8],
) -> Result<petal::DispatchResponse, petal::DispatchResponse> {
    let account = petal::wallet_account(ctx)?;
    let chain = std::str::from_utf8(body)
        .map_err(|error| petal::error(-3, format!("chain must be UTF-8: {error}")))?
        .trim();
    if chain.is_empty() {
        return Err(petal::error(
            -3,
            "write the explicit chain name to stage a self-call",
        ));
    }
    petal::sdk::tx_stage(&petal::EvmTransaction {
        wallet: account.wallet().to_owned(),
        chain: chain.to_owned(),
        to: account.address_evm()?,
        value_wei: "0".into(),
        data_hex: "0x".into(),
        nonce: None,
        max_fee_per_gas: None,
        max_priority_fee_per_gas: None,
    })
    .map_err(|error| petal::error(-4, error.message()))?;
    // Inspect the normal account/chain outbox. Staging never confirms or
    // broadcasts the request; a real transaction would still spend gas.
    Ok(petal::DispatchResponse::Write)
}

petal::route_file!(
    spec: petal::write_spec().caps(&["bloom:tx.outbox", "bloom:vfs.read"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(
        b"Write an explicit chain name to stage a zero-value self-call; inspect the normal account outbox.\n".to_vec()
    ),
    write: |ctx: &petal::Ctx, body: &[u8]| stage(ctx, body).unwrap_or_else(|error| error)
);
