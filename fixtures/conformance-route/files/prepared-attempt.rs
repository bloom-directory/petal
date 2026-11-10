petal::route_file!(
    spec: petal::signing_write_spec("conformance.message").caps(&["bloom:store", "bloom:sign", "bloom:sign.status"]),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(b"write JSON to sign or resume\n".to_vec()),
    write: |ctx: &petal::Ctx, body: &[u8]| {
        use conformance_route::{alloy_primitives, serde_json};
        use petal::signing::{PrepareMode, CtxHost, SignHalt, SigningError, ErrorCode, eip712_action, erc20_spend};
        let result = (|| -> Result<(), SignHalt> {
            let input: serde_json::Value = serde_json::from_slice(body).map_err(|e| SigningError { code: ErrorCode::Invalid, message: e.to_string() })?;
            let build = || {
                let message = input.get("message").and_then(|v| v.as_str()).unwrap_or("hello");
                let typed = serde_json::json!({"types":{"EIP712Domain":[{"name":"name","type":"string"}],"Message":[{"name":"message","type":"string"}]},"domain":{"name":"SDK fixture"},"primaryType":"Message","message":{"message":message}});
                let action = eip712_action("conformance.message", &typed)?.with_spend(erc20_spend(1, alloy_primitives::Address::repeat_byte(0x11), alloy_primitives::U256::from(1))?);
                Ok((action, message.to_owned()))
            };
            let mut host = CtxHost(ctx);
            let mode = if input.get("renew").and_then(|v| v.as_bool()).unwrap_or(false) {
                PrepareMode::RenewTerminal
            } else {
                PrepareMode::Resume
            };
            petal::prepared_sign!(host: &mut host, wallet: "fixture",
                scope: "state/fixture/attempt", mode: mode, build: build,
                validate: |_| Ok(()))?;
            Ok(())
        })();
        match result { Ok(()) => petal::DispatchResponse::Write, Err(e) => e.into() }
    }
);
