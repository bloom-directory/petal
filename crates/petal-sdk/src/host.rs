//! The one seam between a Petal's domain code and Bloom.
//!
//! [`Host`] and [`BloomHost`] are reached as `petal::host::Host` and
//! `petal::host::BloomHost`.

use crate::sdk;
use crate::{
    EvmTransaction, HostStatus, HttpRequest, HttpResponse, OutboxInspection,
    PayloadBatchSignRequest, PayloadSignRequest, PetalKeyOutcome, PetalKeyRequest, SdkError,
    SignBatchOutcome, SignOutcome, StagedTransaction, VfsRead,
};

/// Every call a Petal makes to Bloom, one method per [`crate::sdk`] function.
///
/// Host imports are `unreachable!()` off `wasm32`, so a function that calls
/// `petal::sdk` directly cannot be unit-tested. Write domain code against
/// `&mut impl Host` instead. A route file passes [`BloomHost`]; a test passes
/// `petal::testing::FakeHost`, which the `testing` feature adds.
///
/// Errors are [`SdkError`], as from `petal::sdk`, so the status survives for
/// [`crate::host_error`] and Bloom's reason for [`sdk::describe_error`].
///
/// An implementation supplies the calls that reach Bloom. The methods with a
/// body are built from those, so a test host answers them the way the SDK
/// does.
pub trait Host {
    fn http_fetch(&mut self, req: &HttpRequest, max_bytes: usize)
    -> Result<HttpResponse, SdkError>;

    fn request_key(&mut self, request_jcs: &[u8]) -> Result<Vec<u8>, SdkError>;

    fn derive_key(&mut self, request: &PetalKeyRequest) -> Result<PetalKeyOutcome, SdkError> {
        let outcome = self.request_key(&sdk::key_request_jcs(request)?)?;
        sdk::key_outcome(&outcome)
    }

    fn sign_payload(&mut self, req: &PayloadSignRequest) -> Result<SignOutcome, SdkError>;

    fn sign_payload_batch(
        &mut self,
        req: &PayloadBatchSignRequest,
    ) -> Result<SignBatchOutcome, SdkError>;

    fn tx_stage(&mut self, req: &EvmTransaction) -> Result<StagedTransaction, SdkError>;

    fn tx_confirm(
        &mut self,
        wallet: &str,
        chain: &str,
        outbox_id: &str,
        acknowledge_warnings: bool,
    ) -> Result<StagedTransaction, SdkError>;

    fn tx_inspect(
        &mut self,
        wallet: &str,
        chain: &str,
        outbox_id: &str,
    ) -> Result<OutboxInspection, SdkError>;

    fn chain_read(
        &mut self,
        chain: &str,
        method: &str,
        params_json: &str,
    ) -> Result<String, SdkError>;

    /// See [`sdk::eth_call`].
    fn eth_call(&mut self, chain: &str, to: &str, data: &[u8]) -> Result<Vec<u8>, SdkError> {
        let result = self.chain_read(chain, "eth_call", &sdk::eth_call_params(to, data)?)?;
        sdk::eth_call_result(&result)
    }

    /// See [`sdk::store_get`].
    fn store_get(&mut self, key: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        self.store_get_opt(key, max_bytes)?
            .ok_or(SdkError::Host(HostStatus::NotFound))
    }

    /// See [`sdk::store_get_opt`].
    fn store_get_opt(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, SdkError>;

    /// See [`sdk::store_get_secret`].
    fn store_get_secret(
        &mut self,
        key: &str,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, SdkError>;

    fn store_put(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), SdkError>;

    fn store_put_new(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), SdkError>;

    fn store_del(&mut self, key: &str) -> Result<(), SdkError>;

    fn store_del_if_value(&mut self, key: &str, expected: &[u8]) -> Result<(), SdkError>;

    fn store_list(&mut self, prefix: &str, max_bytes: usize) -> Result<Vec<String>, SdkError>;

    fn vfs_read(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError>;

    fn vfs_write(&mut self, path: &str, body: &[u8]) -> Result<(), SdkError>;

    fn vfs_list(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>, SdkError>;

    /// See [`sdk::now_ms`]: `0` when Bloom cannot tell the time. Prefer
    /// [`Host::try_now_ms`] wherever a wrong time changes a decision, such as
    /// an expiry.
    fn now_ms(&mut self) -> u64 {
        self.try_now_ms().unwrap_or(0)
    }

    fn try_now_ms(&mut self) -> Result<u64, SdkError>;

    fn random_bytes(&mut self, len: usize) -> Result<Vec<u8>, SdkError>;

    fn runtime_setting(&mut self, key: &str) -> Result<Option<String>, SdkError>;
}

/// Bloom itself: every method is the `petal::sdk` function of the same name.
#[derive(Clone, Copy, Debug, Default)]
pub struct BloomHost;

impl Host for BloomHost {
    fn http_fetch(
        &mut self,
        req: &HttpRequest,
        max_bytes: usize,
    ) -> Result<HttpResponse, SdkError> {
        sdk::http_fetch(req, max_bytes)
    }

    fn request_key(&mut self, request_jcs: &[u8]) -> Result<Vec<u8>, SdkError> {
        sdk::request_key(request_jcs)
    }

    fn sign_payload(&mut self, req: &PayloadSignRequest) -> Result<SignOutcome, SdkError> {
        sdk::sign_payload(req)
    }

    fn sign_payload_batch(
        &mut self,
        req: &PayloadBatchSignRequest,
    ) -> Result<SignBatchOutcome, SdkError> {
        sdk::sign_payload_batch(req)
    }

    fn tx_stage(&mut self, req: &EvmTransaction) -> Result<StagedTransaction, SdkError> {
        sdk::tx_stage(req)
    }

    fn tx_confirm(
        &mut self,
        wallet: &str,
        chain: &str,
        outbox_id: &str,
        acknowledge_warnings: bool,
    ) -> Result<StagedTransaction, SdkError> {
        sdk::tx_confirm(wallet, chain, outbox_id, acknowledge_warnings)
    }

    fn tx_inspect(
        &mut self,
        wallet: &str,
        chain: &str,
        outbox_id: &str,
    ) -> Result<OutboxInspection, SdkError> {
        sdk::tx_inspect(wallet, chain, outbox_id)
    }

    fn chain_read(
        &mut self,
        chain: &str,
        method: &str,
        params_json: &str,
    ) -> Result<String, SdkError> {
        sdk::chain_read(chain, method, params_json)
    }

    fn store_get_opt(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, SdkError> {
        sdk::store_get_opt(key, max_bytes)
    }

    fn store_get_secret(
        &mut self,
        key: &str,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, SdkError> {
        sdk::store_get_secret(key, max_bytes)
    }

    fn store_put(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), SdkError> {
        sdk::store_put(key, value, secret)
    }

    fn store_put_new(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), SdkError> {
        sdk::store_put_new(key, value, secret)
    }

    fn store_del(&mut self, key: &str) -> Result<(), SdkError> {
        sdk::store_del(key)
    }

    fn store_del_if_value(&mut self, key: &str, expected: &[u8]) -> Result<(), SdkError> {
        sdk::store_del_if_value(key, expected)
    }

    fn store_list(&mut self, prefix: &str, max_bytes: usize) -> Result<Vec<String>, SdkError> {
        sdk::store_list(prefix, max_bytes)
    }

    fn vfs_read(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        sdk::vfs_read(path, max_bytes)
    }

    fn vfs_write(&mut self, path: &str, body: &[u8]) -> Result<(), SdkError> {
        sdk::vfs_write(path, body)
    }

    fn vfs_list(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>, SdkError> {
        sdk::vfs_list(path, max_bytes)
    }

    fn try_now_ms(&mut self) -> Result<u64, SdkError> {
        sdk::try_now_ms()
    }

    fn random_bytes(&mut self, len: usize) -> Result<Vec<u8>, SdkError> {
        sdk::random_bytes(len)
    }

    fn runtime_setting(&mut self, key: &str) -> Result<Option<String>, SdkError> {
        sdk::runtime_setting(key)
    }
}

/// Any host reads the VFS, so [`crate::WalletAccount::address_evm_from`] and
/// its Solana twin take a `Host` as they take a [`VfsRead`].
impl<H: Host> VfsRead for H {
    fn read(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        self.vfs_read(path, max_bytes)
    }
}
