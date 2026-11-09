//! Domain code shared by this Petal's route components.
//!
//! Route files under `route/files/` stay thin: they read their parameters,
//! call into modules here for reusable domain logic, and assemble the
//! response. Take a `petal::WalletAccount` rather than a bare wallet string,
//! and a `petal::VfsRead` where a test needs to stand in for Bloom.

#[cfg(test)]
mod tests {
    use petal::testing::{MapVfs, wallet_account};

    #[test]
    fn reads_an_account_address_from_bloom_s_projection() {
        let account = wallet_account("main", 0);
        let mut vfs = MapVfs::new().file(
            &account.address_evm_path(),
            b"0x52908400098527886E0F7030069857D2E4169EE7\n",
        );
        assert_eq!(
            account.address_evm_from(&mut vfs).unwrap(),
            "0x52908400098527886E0F7030069857D2E4169EE7"
        );
    }
}
