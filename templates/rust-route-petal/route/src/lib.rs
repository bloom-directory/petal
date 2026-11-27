//! Domain code shared by this Petal's route components.
//!
//! Route files under `route/files/` stay thin: they read their parameters,
//! call into modules here for reusable domain logic, and assemble the
//! response. Code here that calls Bloom takes a `&mut impl petal::host::Host`
//! rather than calling `petal::sdk` itself: a route file passes
//! `petal::host::BloomHost`, and a test passes `petal::testing::FakeHost`.
//! Take a `petal::WalletAccount` rather than a bare wallet string.

use petal::host::Host;
use petal::{DispatchResponse, WalletAccount};

/// What `account/<wallet>/<index>/address.json` shows: the account Bloom
/// resolved for the route, and its EVM address from Bloom's projection.
pub fn account_address(
    host: &mut impl Host,
    account: &WalletAccount,
) -> Result<serde_json::Value, DispatchResponse> {
    let address = account.address_evm_from(host)?;
    Ok(serde_json::json!({
        "wallet": account.wallet(),
        "account": account.number(),
        "address": address,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use petal::testing::{FakeHost, wallet_account};

    #[test]
    fn shows_the_account_s_address_from_bloom_s_projection() {
        let account = wallet_account("main", 0);
        let mut host = FakeHost::new();
        host.with_file(
            &account.address_evm_path(),
            b"0x52908400098527886E0F7030069857D2E4169EE7\n",
        );
        assert_eq!(
            account_address(&mut host, &account).unwrap(),
            serde_json::json!({
                "wallet": "main",
                "account": 0,
                "address": "0x52908400098527886E0F7030069857D2E4169EE7",
            })
        );
        // The fake host logs every call, so a test can say what was read.
        assert_eq!(host.calls(), ["vfs_read wallets/main/0/address.evm"]);
    }

    #[test]
    fn an_account_without_an_evm_key_is_not_found() {
        let mut host = FakeHost::new();
        let error = account_address(&mut host, &wallet_account("main", 1)).unwrap_err();
        assert!(matches!(error, DispatchResponse::Error { code: -1, .. }), "{error:?}");
    }
}
