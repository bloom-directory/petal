// `[wallet]/[index]` is the account Bloom resolved before this route runs;
// `wallet_account` reads that trusted pair, never the raw path.
petal::route_file!(
    spec: petal::static_read_spec().caps(&["bloom:vfs.read"]),
    read: |ctx: &petal::Ctx| {
        let account = match petal::wallet_account(ctx) {
            Ok(account) => account,
            Err(response) => return response,
        };
        match account.address_evm() {
            Ok(address) => petal::read_json_value(&serde_json::json!({
                "wallet": account.wallet(),
                "account": account.number(),
                "address": address,
            })),
            Err(response) => response,
        }
    }
);
