// `[wallet]/[index]` is the account Bloom resolved before this route runs;
// `wallet_account` reads that trusted pair, never the raw path.
petal::route_file!(
    spec: petal::static_read_spec().caps(&["bloom:vfs.read"]),
    read: |ctx: &petal::Ctx| {
        let account = match petal::wallet_account(ctx) {
            Ok(account) => account,
            Err(response) => return response,
        };
        match crate::account_address(&mut petal::host::BloomHost, &account) {
            Ok(summary) => petal::read_json_value(&summary),
            Err(response) => response,
        }
    }
);
