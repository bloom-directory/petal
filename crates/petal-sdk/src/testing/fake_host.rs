//! An in-memory [`Host`]. Compiled only with the `testing` feature.

use crate::host::Host;
use crate::sdk;
use crate::{
    EvmTransaction, HostStatus, HttpRequest, HttpResponse, OutboxInspection,
    PayloadBatchSignRequest, PayloadSignRequest, PetalKeyOutcome, SdkError, SignBatchOutcome,
    SignOutcome, StagedTransaction,
};
use std::collections::{BTreeMap, VecDeque};

/// A 200 or other response with no headers, for [`FakeHost::reply_http`].
pub fn http_response(status: u16, body: &[u8]) -> HttpResponse {
    HttpResponse {
        status,
        headers: Vec::new(),
        body: body.to_vec(),
    }
}

/// What Bloom's chain host answers an `eth_call` with when the contract
/// returns `bytes`, for [`FakeHost::reply_chain`].
pub fn eth_call_reply(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("\"0x{hex}\"")
}

/// The error the SDK returns when Bloom answers a host call with `message`,
/// such as `"denied: store key already exists"`. A Petal's own test host
/// builds its failures with this, so they are classified, and their reasons
/// recorded, as Bloom's are.
pub fn bloom_error(message: &str) -> SdkError {
    sdk::host_err(message.to_owned())
}

type HttpHandler = Box<dyn FnMut(&HttpRequest) -> Option<Result<HttpResponse, String>>>;
type ChainHandler = Box<dyn FnMut(&str, &str, &str) -> Option<Result<String, String>>>;
type Replies<T> = VecDeque<Result<T, String>>;

/// The calls [`FakeHost::fail_next`] can fail: the [`Host`] methods that
/// reach Bloom.
const HOST_CALLS: &[&str] = &[
    "http_fetch",
    "request_key",
    "sign_payload",
    "sign_payload_batch",
    "tx_stage",
    "tx_confirm",
    "tx_inspect",
    "chain_read",
    "store_get_opt",
    "store_get_secret",
    "store_put",
    "store_put_new",
    "store_del",
    "store_del_if_value",
    "store_list",
    "vfs_read",
    "vfs_write",
    "vfs_list",
    "try_now_ms",
    "random_bytes",
    "runtime_setting",
];

/// An in-memory [`Host`] for tests.
///
/// The store, the secret store and the VFS hold what the test seeds and what
/// the code under test writes. HTTP, chain reads, signing, key requests and
/// the outbox answer with replies the test scripts beforehand, and a call
/// with no reply left fails with an error that says which. Every call is
/// logged in order.
///
/// It behaves as the SDK does in front of Bloom:
///
/// - A scripted failure is the message Bloom would send, such as
///   `"denied: USER_REJECTED: the owner declined"`. It is classified into an
///   [`SdkError`], and with the `host-error-detail` feature its reason is
///   recorded for [`sdk::describe_error`], exactly as from the real host.
/// - Reads enforce `max_bytes`, and signing, key and outbox calls run the
///   checks the SDK runs before it calls Bloom, such as the
///   [`crate::MessageReview`] requirement.
/// - A value written with `secret = true` is in the secret namespace, where
///   [`Host::store_get`] finds it only under `creds/`. Read it with
///   [`Host::store_get_secret`].
/// - A store key that exists refuses `store_put_new` with
///   `denied: store key already exists`, and `store_del_if_value` answers
///   `not found` for a missing key and `denied: store key value changed` for
///   another value.
///
/// The methods [`Host`] builds from others are logged, scripted and failed
/// under the call that reaches Bloom: `store_get` as `store_get_opt`,
/// `eth_call` as `chain_read`, `derive_key` as `request_key` and `now_ms` as
/// `try_now_ms`.
///
/// ```ignore
/// let mut host = FakeHost::new();
/// host.with_state("drafts/alice/7.json", br#"{"amount":"5"}"#)
///     .reply_http("GET", "https://api.example/price", Ok(http_response(200, b"{}")));
/// my_petal::quote(&mut host, &wallet_account("alice", 0), "7")?;
/// assert_eq!(host.calls(), ["store_get_opt drafts/alice/7.json", "http_fetch GET https://api.example/price"]);
/// ```
pub struct FakeHost {
    state: BTreeMap<String, Vec<u8>>,
    secrets: BTreeMap<String, Vec<u8>>,
    files: BTreeMap<String, Vec<u8>>,
    settings: BTreeMap<String, String>,
    now_ms: u64,
    random_calls: u8,
    http: BTreeMap<(String, String), Replies<HttpResponse>>,
    http_handler: Option<HttpHandler>,
    chain: BTreeMap<(String, String), Replies<String>>,
    chain_handler: Option<ChainHandler>,
    keys: Replies<PetalKeyOutcome>,
    signatures: Replies<SignOutcome>,
    batch_signatures: Replies<SignBatchOutcome>,
    stages: Replies<StagedTransaction>,
    confirms: Replies<StagedTransaction>,
    inspections: Replies<OutboxInspection>,
    faults: Vec<(String, String)>,
    calls: Vec<String>,
    http_requests: Vec<HttpRequest>,
    key_requests: Vec<Vec<u8>>,
    sign_requests: Vec<PayloadSignRequest>,
    batch_sign_requests: Vec<PayloadBatchSignRequest>,
    staged: Vec<EvmTransaction>,
    vfs_writes: Vec<(String, Vec<u8>)>,
}

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeHost {
    /// The time a new host reports: 14 November 2023, 22:13:20 UTC.
    pub const START_MS: u64 = 1_700_000_000_000;

    pub fn new() -> Self {
        Self {
            state: BTreeMap::new(),
            secrets: BTreeMap::new(),
            files: BTreeMap::new(),
            settings: BTreeMap::new(),
            now_ms: Self::START_MS,
            random_calls: 0,
            http: BTreeMap::new(),
            http_handler: None,
            chain: BTreeMap::new(),
            chain_handler: None,
            keys: VecDeque::new(),
            signatures: VecDeque::new(),
            batch_signatures: VecDeque::new(),
            stages: VecDeque::new(),
            confirms: VecDeque::new(),
            inspections: VecDeque::new(),
            faults: Vec::new(),
            calls: Vec::new(),
            http_requests: Vec::new(),
            key_requests: Vec::new(),
            sign_requests: Vec::new(),
            batch_sign_requests: Vec::new(),
            staged: Vec::new(),
            vfs_writes: Vec::new(),
        }
    }

    // --- What the host holds -------------------------------------------

    /// Seed a state key, as `store_put(key, value, false)` would. A key
    /// under `creds/` is seeded in the secret namespace, where the SDK keeps
    /// it.
    pub fn with_state(&mut self, key: &str, value: &[u8]) -> &mut Self {
        self.namespace(key, false)
            .insert(key.to_owned(), value.to_vec());
        self
    }

    /// Seed a secret, as `store_put(key, value, true)` would.
    pub fn with_secret(&mut self, key: &str, value: &[u8]) -> &mut Self {
        self.secrets.insert(key.to_owned(), value.to_vec());
        self
    }

    /// Seed a VFS file, such as an account's `address.evm`.
    pub fn with_file(&mut self, path: &str, bytes: &[u8]) -> &mut Self {
        self.files.insert(path.to_owned(), bytes.to_vec());
        self
    }

    /// Seed a runtime setting.
    pub fn with_setting(&mut self, key: &str, value: &str) -> &mut Self {
        self.settings.insert(key.to_owned(), value.to_owned());
        self
    }

    /// Set the time `try_now_ms` reports. It does not advance by itself.
    pub fn set_now_ms(&mut self, now_ms: u64) -> &mut Self {
        self.now_ms = now_ms;
        self
    }

    /// Move the clock forward.
    pub fn advance_ms(&mut self, by_ms: u64) -> &mut Self {
        self.now_ms = self.now_ms.saturating_add(by_ms);
        self
    }

    // --- What the host answers -----------------------------------------

    /// Script the next reply to `method` on exactly `url`. Replies to one
    /// URL are used in the order scripted, once each.
    pub fn reply_http(
        &mut self,
        method: &str,
        url: &str,
        reply: Result<HttpResponse, &str>,
    ) -> &mut Self {
        self.http
            .entry((method.to_owned(), url.to_owned()))
            .or_default()
            .push_back(reply.map_err(str::to_owned));
        self
    }

    /// Answer the HTTP requests no [`FakeHost::reply_http`] covers. Return
    /// `None` for a request the test does not expect; it fails as an
    /// unscripted call.
    pub fn on_http(
        &mut self,
        handler: impl FnMut(&HttpRequest) -> Option<Result<HttpResponse, String>> + 'static,
    ) -> &mut Self {
        self.http_handler = Some(Box::new(handler));
        self
    }

    /// Script the next reply to `method` on `chain`: the JSON-RPC result as
    /// JSON text, such as `"\"0x1\""`. For an `eth_call`, build it with
    /// [`eth_call_reply`].
    pub fn reply_chain(
        &mut self,
        chain: &str,
        method: &str,
        reply: Result<&str, &str>,
    ) -> &mut Self {
        self.chain
            .entry((chain.to_owned(), method.to_owned()))
            .or_default()
            .push_back(reply.map(str::to_owned).map_err(str::to_owned));
        self
    }

    /// Answer the chain reads no [`FakeHost::reply_chain`] covers, from the
    /// chain, method and parameters. Return `None` for an unexpected read.
    pub fn on_chain(
        &mut self,
        handler: impl FnMut(&str, &str, &str) -> Option<Result<String, String>> + 'static,
    ) -> &mut Self {
        self.chain_handler = Some(Box::new(handler));
        self
    }

    /// Script the next `request_key` or `derive_key` outcome.
    pub fn reply_key(&mut self, reply: Result<PetalKeyOutcome, &str>) -> &mut Self {
        self.keys.push_back(reply.map_err(str::to_owned));
        self
    }

    /// Script the next `sign_payload` outcome.
    pub fn reply_sign(&mut self, reply: Result<SignOutcome, &str>) -> &mut Self {
        self.signatures.push_back(reply.map_err(str::to_owned));
        self
    }

    /// Script the next `sign_payload_batch` outcome.
    pub fn reply_sign_batch(&mut self, reply: Result<SignBatchOutcome, &str>) -> &mut Self {
        self.batch_signatures
            .push_back(reply.map_err(str::to_owned));
        self
    }

    /// Script the next `tx_stage` outcome.
    pub fn reply_tx_stage(&mut self, reply: Result<StagedTransaction, &str>) -> &mut Self {
        self.stages.push_back(reply.map_err(str::to_owned));
        self
    }

    /// Script the next `tx_confirm` outcome.
    pub fn reply_tx_confirm(&mut self, reply: Result<StagedTransaction, &str>) -> &mut Self {
        self.confirms.push_back(reply.map_err(str::to_owned));
        self
    }

    /// Script the next `tx_inspect` outcome.
    pub fn reply_tx_inspect(&mut self, reply: Result<OutboxInspection, &str>) -> &mut Self {
        self.inspections.push_back(reply.map_err(str::to_owned));
        self
    }

    /// Make the next call of one [`Host`] method fail with the message Bloom
    /// would send, such as `fail_next("store_put", "backend: store put: disk
    /// full")`. Name the call that reaches Bloom: `store_get_opt` for a
    /// `store_get`, `chain_read` for an `eth_call`.
    ///
    /// Panics on a name that is not such a call, so a typo cannot leave a
    /// test passing for the wrong reason.
    pub fn fail_next(&mut self, call: &str, host_message: &str) -> &mut Self {
        assert!(
            HOST_CALLS.contains(&call),
            "FakeHost::fail_next: {call:?} is not a host call; use one of {HOST_CALLS:?}"
        );
        self.faults.push((call.to_owned(), host_message.to_owned()));
        self
    }

    // --- What the code under test did ----------------------------------

    /// Every call that reached the host, in order, as the method name and
    /// what it addressed: `"store_put drafts/alice/7.json"`,
    /// `"http_fetch GET https://api.example/price"`,
    /// `"chain_read base eth_call"`, `"sign_payload alice transfer.send"`,
    /// `"tx_stage alice base 0x…"`. Values are never logged.
    pub fn calls(&self) -> &[String] {
        &self.calls
    }

    /// The value at a state key, or under `creds/` at that secret key.
    pub fn state(&self, key: &str) -> Option<&[u8]> {
        let namespace = if sdk::is_secret_key(key, false) {
            &self.secrets
        } else {
            &self.state
        };
        namespace.get(key).map(Vec::as_slice)
    }

    /// The value at a key in the secret namespace.
    pub fn secret(&self, key: &str) -> Option<&[u8]> {
        self.secrets.get(key).map(Vec::as_slice)
    }

    /// The HTTP requests made, in order.
    pub fn http_requests(&self) -> &[HttpRequest] {
        &self.http_requests
    }

    /// The key requests made, in order, as the canonical JSON sent.
    pub fn key_requests(&self) -> &[Vec<u8>] {
        &self.key_requests
    }

    /// The single-payload sign requests made, in order.
    pub fn sign_requests(&self) -> &[PayloadSignRequest] {
        &self.sign_requests
    }

    /// The batch sign requests made, in order.
    pub fn batch_sign_requests(&self) -> &[PayloadBatchSignRequest] {
        &self.batch_sign_requests
    }

    /// The transactions staged, in order.
    pub fn staged(&self) -> &[EvmTransaction] {
        &self.staged
    }

    /// The VFS writes made, in order. A write is recorded and changes no
    /// file: in Bloom a write is a request to the node at that path.
    pub fn vfs_writes(&self) -> &[(String, Vec<u8>)] {
        &self.vfs_writes
    }

    // --- Internals ------------------------------------------------------

    fn namespace(&mut self, key: &str, secret: bool) -> &mut BTreeMap<String, Vec<u8>> {
        if sdk::is_secret_key(key, secret) {
            &mut self.secrets
        } else {
            &mut self.state
        }
    }

    /// Log a call and apply a fault scripted for it.
    fn call(&mut self, name: &'static str, detail: &str) -> Result<(), SdkError> {
        self.calls.push(if detail.is_empty() {
            name.to_owned()
        } else {
            format!("{name} {detail}")
        });
        sdk::clear_host_reason();
        match self.faults.iter().position(|(call, _)| call == name) {
            Some(index) => Err(sdk::host_err(self.faults.remove(index).1)),
            None => Ok(()),
        }
    }
}

/// A scripted reply, or the error for a call the test did not script.
fn next<T>(replies: &mut Replies<T>, call: &str) -> Result<T, SdkError> {
    reply(replies.pop_front(), call)
}

fn reply<T>(scripted: Option<Result<T, String>>, call: &str) -> Result<T, SdkError> {
    match scripted {
        Some(Ok(value)) => Ok(value),
        Some(Err(message)) => Err(sdk::host_err(message)),
        None => Err(SdkError::Message(format!(
            "FakeHost: no reply scripted for {call}"
        ))),
    }
}

fn too_large(needed: usize) -> SdkError {
    SdkError::Host(HostStatus::BufferTooSmall { needed })
}

impl Host for FakeHost {
    fn http_fetch(
        &mut self,
        req: &HttpRequest,
        max_bytes: usize,
    ) -> Result<HttpResponse, SdkError> {
        let detail = format!("{} {}", req.method, req.url);
        self.call("http_fetch", &detail)?;
        self.http_requests.push(req.clone());
        let scripted = self
            .http
            .get_mut(&(req.method.clone(), req.url.clone()))
            .and_then(VecDeque::pop_front)
            .or_else(|| self.http_handler.as_mut().and_then(|handler| handler(req)));
        let response = reply(scripted, &format!("http_fetch {detail}"))?;
        if response.body.len() > max_bytes {
            return Err(too_large(response.body.len()));
        }
        Ok(response)
    }

    fn request_key(&mut self, request_jcs: &[u8]) -> Result<Vec<u8>, SdkError> {
        sdk::check_key_request(request_jcs)?;
        self.call("request_key", "")?;
        self.key_requests.push(request_jcs.to_vec());
        let outcome = next(&mut self.keys, "request_key")?;
        serde_json::to_vec(&outcome)
            .map_err(|error| SdkError::Message(format!("FakeHost: encode key outcome: {error}")))
    }

    fn sign_payload(&mut self, req: &PayloadSignRequest) -> Result<SignOutcome, SdkError> {
        sdk::check_sign_payload(req)?;
        self.call(
            "sign_payload",
            &format!("{} {}", req.wallet, req.operation_class),
        )?;
        self.sign_requests.push(req.clone());
        next(&mut self.signatures, "sign_payload")
    }

    fn sign_payload_batch(
        &mut self,
        req: &PayloadBatchSignRequest,
    ) -> Result<SignBatchOutcome, SdkError> {
        sdk::check_sign_payload_batch(req)?;
        self.call(
            "sign_payload_batch",
            &format!("{} {}", req.wallet, req.operation_class),
        )?;
        self.batch_sign_requests.push(req.clone());
        match next(&mut self.batch_signatures, "sign_payload_batch")? {
            SignBatchOutcome::Signatures(signatures) if signatures.len() != req.payloads.len() => {
                Err(SdkError::Message(
                    "host returned the wrong payload batch signature count".into(),
                ))
            }
            outcome => Ok(outcome),
        }
    }

    fn tx_stage(&mut self, req: &EvmTransaction) -> Result<StagedTransaction, SdkError> {
        sdk::check_wallet(&req.wallet)?;
        self.call(
            "tx_stage",
            &format!("{} {} {}", req.wallet, req.chain, req.to),
        )?;
        self.staged.push(req.clone());
        next(&mut self.stages, "tx_stage")
    }

    fn tx_confirm(
        &mut self,
        wallet: &str,
        chain: &str,
        outbox_id: &str,
        _acknowledge_warnings: bool,
    ) -> Result<StagedTransaction, SdkError> {
        sdk::check_wallet(wallet)?;
        self.call("tx_confirm", &format!("{wallet} {chain} {outbox_id}"))?;
        next(&mut self.confirms, "tx_confirm")
    }

    fn tx_inspect(
        &mut self,
        wallet: &str,
        chain: &str,
        outbox_id: &str,
    ) -> Result<OutboxInspection, SdkError> {
        sdk::check_wallet(wallet)?;
        self.call("tx_inspect", &format!("{wallet} {chain} {outbox_id}"))?;
        next(&mut self.inspections, "tx_inspect")
    }

    fn chain_read(
        &mut self,
        chain: &str,
        method: &str,
        params_json: &str,
    ) -> Result<String, SdkError> {
        let detail = format!("{chain} {method}");
        self.call("chain_read", &detail)?;
        let scripted = self
            .chain
            .get_mut(&(chain.to_owned(), method.to_owned()))
            .and_then(VecDeque::pop_front)
            .or_else(|| {
                self.chain_handler
                    .as_mut()
                    .and_then(|handler| handler(chain, method, params_json))
            });
        reply(scripted, &format!("chain_read {detail}"))
    }

    fn store_get_opt(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, SdkError> {
        self.call("store_get_opt", key)?;
        match self.namespace(key, false).get(key) {
            Some(value) if value.len() > max_bytes => Err(too_large(value.len())),
            value => Ok(value.cloned()),
        }
    }

    fn store_get_secret(
        &mut self,
        key: &str,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, SdkError> {
        self.call("store_get_secret", key)?;
        match self.secrets.get(key) {
            Some(value) if value.len() > max_bytes => Err(too_large(value.len())),
            value => Ok(value.cloned()),
        }
    }

    fn store_put(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), SdkError> {
        self.call("store_put", key)?;
        self.namespace(key, secret)
            .insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn store_put_new(&mut self, key: &str, value: &[u8], secret: bool) -> Result<(), SdkError> {
        self.call("store_put_new", key)?;
        let namespace = self.namespace(key, secret);
        if namespace.contains_key(key) {
            return Err(sdk::host_err("denied: store key already exists".into()));
        }
        namespace.insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn store_del(&mut self, key: &str) -> Result<(), SdkError> {
        self.call("store_del", key)?;
        self.namespace(key, false).remove(key);
        Ok(())
    }

    fn store_del_if_value(&mut self, key: &str, expected: &[u8]) -> Result<(), SdkError> {
        self.call("store_del_if_value", key)?;
        let namespace = self.namespace(key, false);
        match namespace.get(key) {
            None => Err(sdk::host_err(format!("not found: {key}"))),
            Some(value) if value != expected => {
                Err(sdk::host_err("denied: store key value changed".into()))
            }
            Some(_) => {
                namespace.remove(key);
                Ok(())
            }
        }
    }

    fn store_list(&mut self, prefix: &str, max_bytes: usize) -> Result<Vec<String>, SdkError> {
        self.call("store_list", prefix)?;
        let keys: Vec<String> = self
            .namespace(prefix, false)
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect();
        let size = keys.iter().map(String::len).sum::<usize>();
        if size > max_bytes {
            return Err(too_large(size));
        }
        Ok(keys)
    }

    fn vfs_read(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        self.call("vfs_read", path)?;
        match self.files.get(path) {
            None => Err(sdk::host_err(format!("not found: {path}"))),
            Some(bytes) if bytes.len() > max_bytes => Err(too_large(bytes.len())),
            Some(bytes) => Ok(bytes.clone()),
        }
    }

    fn vfs_write(&mut self, path: &str, body: &[u8]) -> Result<(), SdkError> {
        self.call("vfs_write", path)?;
        self.vfs_writes.push((path.to_owned(), body.to_vec()));
        Ok(())
    }

    fn vfs_list(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>, SdkError> {
        self.call("vfs_list", path)?;
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let mut names: Vec<String> = self
            .files
            .keys()
            .filter_map(|file| file.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.dedup();
        if names.is_empty() {
            return Err(sdk::host_err(format!("not found: {path}")));
        }
        let size = names.iter().map(String::len).sum::<usize>();
        if size > max_bytes {
            return Err(too_large(size));
        }
        Ok(names)
    }

    fn try_now_ms(&mut self) -> Result<u64, SdkError> {
        self.call("try_now_ms", "")?;
        Ok(self.now_ms)
    }

    /// The first call returns `len` bytes of `0x01`, the second of `0x02`,
    /// and so on, so two ids drawn in one test differ and a test can name
    /// them.
    fn random_bytes(&mut self, len: usize) -> Result<Vec<u8>, SdkError> {
        if u32::try_from(len).is_err() {
            sdk::clear_host_reason();
            return Err(SdkError::Host(HostStatus::Invalid));
        }
        self.call("random_bytes", &len.to_string())?;
        self.random_calls = self.random_calls.wrapping_add(1);
        Ok(vec![self.random_calls; len])
    }

    fn runtime_setting(&mut self, key: &str) -> Result<Option<String>, SdkError> {
        self.call("runtime_setting", key)?;
        Ok(self.settings.get(key).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::wallet_account;
    use crate::{PayloadSignItem, PetalKeyRequest, SignSelector};

    fn code(response: &crate::DispatchResponse) -> i32 {
        match response {
            crate::DispatchResponse::Error { code, .. } => *code,
            other => panic!("{other:?}"),
        }
    }

    fn sign_request(preimage: &[u8], action: Option<&[u8]>) -> PayloadSignRequest {
        PayloadSignRequest {
            wallet: "alice".into(),
            preimage: preimage.to_vec(),
            claimed_hash: [0; 32],
            signature_algorithm: "secp256k1-keccak256-recoverable".into(),
            operation_class: "transfer.send".into(),
            petal_use_claim_jcs: Vec::new(),
            claim_assurance_evidence: None,
            approval_hint: None,
            action: action.map(<[u8]>::to_vec),
            advisory: None,
            selector: SignSelector::Exact,
            key_ref_jcs: None,
        }
    }

    fn transaction() -> EvmTransaction {
        EvmTransaction {
            wallet: "alice".into(),
            chain: "base".into(),
            to: "0x1111111111111111111111111111111111111111".into(),
            value_wei: "0".into(),
            data_hex: "0x".into(),
            nonce: None,
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
        }
    }

    #[test]
    fn a_secret_is_read_back_only_through_the_secret_call() {
        let mut host = FakeHost::new();
        host.store_put("agent/alice.key", b"k", true).unwrap();
        // The copy every Petal got wrong: `store_get` looks in the state
        // namespace for any key outside `creds/`.
        assert_eq!(host.store_get_opt("agent/alice.key", 64).unwrap(), None);
        assert_eq!(
            host.store_get("agent/alice.key", 64).unwrap_err(),
            SdkError::Host(HostStatus::NotFound)
        );
        assert_eq!(
            host.store_get_secret("agent/alice.key", 64).unwrap(),
            Some(b"k".to_vec())
        );
        assert_eq!(host.secret("agent/alice.key"), Some(&b"k"[..]));
        assert_eq!(host.state("agent/alice.key"), None);
    }

    #[test]
    fn a_creds_key_is_secret_however_it_is_written_and_read() {
        let mut host = FakeHost::new();
        host.store_put("creds/alice/api.json", b"{}", false)
            .unwrap();
        assert_eq!(host.secret("creds/alice/api.json"), Some(&b"{}"[..]));
        assert_eq!(host.store_get("creds/alice/api.json", 64).unwrap(), b"{}");
        assert_eq!(
            host.store_list("creds/", 1024).unwrap(),
            ["creds/alice/api.json"]
        );
        host.store_del("creds/alice/api.json").unwrap();
        assert_eq!(host.secret("creds/alice/api.json"), None);
    }

    #[test]
    fn a_taken_key_refuses_put_new_as_bloom_does() {
        let mut host = FakeHost::new();
        host.store_put_new("drafts/7.json", b"a", false).unwrap();
        let error = host
            .store_put_new("drafts/7.json", b"b", false)
            .unwrap_err();
        assert_eq!(error, SdkError::Host(HostStatus::Denied));
        if cfg!(feature = "host-error-detail") {
            assert_eq!(
                sdk::describe_error(&error),
                "denied: store key already exists"
            );
        }
        assert_eq!(host.state("drafts/7.json"), Some(&b"a"[..]));
    }

    #[test]
    fn delete_if_value_removes_only_the_value_it_was_shown() {
        let mut host = FakeHost::new();
        host.with_state("locks/7", b"mine");
        assert_eq!(
            host.store_del_if_value("locks/7", b"theirs").unwrap_err(),
            SdkError::Host(HostStatus::Denied)
        );
        assert_eq!(host.state("locks/7"), Some(&b"mine"[..]));
        host.store_del_if_value("locks/7", b"mine").unwrap();
        assert_eq!(
            host.store_del_if_value("locks/7", b"mine").unwrap_err(),
            SdkError::Host(HostStatus::NotFound)
        );
        // A plain delete of a missing key succeeds, as in Bloom.
        host.store_del("locks/7").unwrap();
    }

    #[test]
    fn reads_enforce_the_callers_limit() {
        let mut host = FakeHost::new();
        host.with_state("a", b"12345")
            .with_secret("s", b"12345")
            .with_file("wallets/alice/0/account.json", b"12345")
            .reply_http("GET", "https://x/", Ok(http_response(200, b"12345")));
        let too_large = SdkError::Host(HostStatus::BufferTooSmall { needed: 5 });
        assert_eq!(host.store_get_opt("a", 4).unwrap_err(), too_large);
        assert_eq!(host.store_get_secret("s", 4).unwrap_err(), too_large);
        assert_eq!(
            host.vfs_read("wallets/alice/0/account.json", 4)
                .unwrap_err(),
            too_large
        );
        let request = HttpRequest {
            method: "GET".into(),
            url: "https://x/".into(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert_eq!(host.http_fetch(&request, 4).unwrap_err(), too_large);
        assert_eq!(
            host.store_list("", 0).unwrap_err(),
            SdkError::Host(HostStatus::BufferTooSmall { needed: 1 })
        );
        assert_eq!(host.store_get_opt("a", 5).unwrap(), Some(b"12345".to_vec()));
    }

    #[test]
    fn http_replies_are_used_in_order_and_an_unscripted_request_fails() {
        let mut host = FakeHost::new();
        host.reply_http(
            "GET",
            "https://x/status",
            Ok(http_response(200, b"pending")),
        )
        .reply_http("GET", "https://x/status", Ok(http_response(200, b"done")))
        .reply_http("POST", "https://x/submit", Err("backend: connection reset"));
        let get = |url: &str, method: &str| HttpRequest {
            method: method.into(),
            url: url.into(),
            headers: vec![("accept".into(), "application/json".into())],
            body: b"{}".to_vec(),
        };
        let status = get("https://x/status", "GET");
        assert_eq!(host.http_fetch(&status, 64).unwrap().body, b"pending");
        assert_eq!(host.http_fetch(&status, 64).unwrap().body, b"done");
        assert_eq!(
            host.http_fetch(&status, 64).unwrap_err(),
            SdkError::Message(
                "FakeHost: no reply scripted for http_fetch GET https://x/status".into()
            )
        );
        assert_eq!(
            host.http_fetch(&get("https://x/submit", "POST"), 64)
                .unwrap_err(),
            SdkError::Message("backend: connection reset".into())
        );
        assert_eq!(host.http_requests().len(), 4);
        assert_eq!(host.http_requests()[3].body, b"{}");
    }

    #[test]
    fn a_handler_answers_what_no_scripted_reply_covers() {
        let mut host = FakeHost::new();
        host.reply_http("GET", "https://x/a", Ok(http_response(200, b"scripted")))
            .on_http(|request| {
                request
                    .url
                    .starts_with("https://x/")
                    .then(|| Ok(http_response(200, request.url.as_bytes())))
            });
        let get = |url: &str| HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert_eq!(
            host.http_fetch(&get("https://x/a"), 64).unwrap().body,
            b"scripted"
        );
        assert_eq!(
            host.http_fetch(&get("https://x/a"), 64).unwrap().body,
            b"https://x/a"
        );
        assert!(matches!(
            host.http_fetch(&get("https://y/"), 64).unwrap_err(),
            SdkError::Message(message) if message.contains("no reply scripted")
        ));
    }

    #[test]
    fn eth_call_sends_bloom_s_shape_and_decodes_the_answer() {
        use std::{cell::RefCell, rc::Rc};
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let mut host = FakeHost::new();
        host.on_chain(move |chain, method, params| {
            log.borrow_mut().push(format!("{chain} {method} {params}"));
            Some(Ok(eth_call_reply(&[0, 0, 0, 7])))
        });
        let token = "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238";
        assert_eq!(
            host.eth_call("sepolia", token, &[0x70, 0xa0, 0x82, 0x31])
                .unwrap(),
            [0, 0, 0, 7]
        );
        let seen = seen.borrow();
        let (prefix, params) = seen[0].split_at("sepolia eth_call ".len());
        assert_eq!(prefix, "sepolia eth_call ");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(params).unwrap(),
            serde_json::json!([{ "to": token, "data": "0x70a08231" }, "latest"])
        );
        assert_eq!(host.calls(), ["chain_read sepolia eth_call"]);
    }

    #[test]
    fn eth_call_refuses_a_bad_address_and_a_result_that_is_not_hex() {
        let mut host = FakeHost::new();
        let token = "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238";
        for to in [
            "",
            "1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
            " 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
            "vitalik.eth",
        ] {
            assert!(
                matches!(host.eth_call("base", to, &[]), Err(SdkError::Message(message)) if message.contains("not an EVM address")),
                "{to:?}"
            );
        }
        // Nothing reached the host for those.
        assert!(host.calls().is_empty());
        for result in ["\"0x0\"", "\"12\"", "\"0xzz\"", "12", "null", "{}"] {
            host.reply_chain("base", "eth_call", Ok(result));
            assert_eq!(
                host.eth_call("base", token, &[]).unwrap_err(),
                SdkError::Message("eth_call result is not a 0x hex string".into()),
                "{result}"
            );
        }
        host.reply_chain("base", "eth_call", Ok("\"0x\""));
        assert_eq!(host.eth_call("base", token, &[]).unwrap(), Vec::<u8>::new());
        host.reply_chain("base", "eth_call", Ok("\"0xAbCd\""));
        assert_eq!(host.eth_call("base", token, &[]).unwrap(), [0xab, 0xcd]);
        // A revert is Bloom's error, with its reason.
        host.reply_chain(
            "base",
            "eth_call",
            Err("backend: eth_call: execution reverted"),
        );
        assert_eq!(
            host.eth_call("base", token, &[]).unwrap_err(),
            SdkError::Message("backend: eth_call: execution reverted".into())
        );
    }

    #[test]
    fn signing_runs_the_sdk_s_checks_before_the_host_is_asked() {
        let mut host = FakeHost::new();
        host.reply_sign(Ok(SignOutcome::Signature(vec![7; 65])));
        // An EIP-712 payload with no MessageReview never reaches Bloom.
        let error = host
            .sign_payload(&sign_request(&[0x19, 0x01, 0xaa], None))
            .unwrap_err();
        assert!(
            matches!(&error, SdkError::Message(message) if message.contains("petal::MessageReview")),
            "{error:?}"
        );
        let mut other_wallet = sign_request(b"plain", None);
        other_wallet.wallet = "0x1111111111111111111111111111111111111111".into();
        assert!(host.sign_payload(&other_wallet).is_err());
        assert!(host.calls().is_empty());
        assert!(host.sign_requests().is_empty());

        assert_eq!(
            host.sign_payload(&sign_request(b"plain", None)).unwrap(),
            SignOutcome::Signature(vec![7; 65])
        );
        assert_eq!(host.calls(), ["sign_payload alice transfer.send"]);
        assert_eq!(host.sign_requests()[0].preimage, b"plain");
    }

    #[test]
    fn a_scripted_refusal_is_classified_and_explained_as_from_bloom() {
        let mut host = FakeHost::new();
        host.reply_sign(Err("denied: USER_REJECTED: the owner declined"))
            .reply_sign(Ok(SignOutcome::ApprovalPending {
                action_id: "act-1".into(),
                expires_ms: 5,
            }));
        let error = host
            .sign_payload(&sign_request(b"plain", None))
            .unwrap_err();
        assert_eq!(error, SdkError::Host(HostStatus::Denied));
        let response = crate::host_error(&error);
        assert_eq!(code(&response), -2);
        if cfg!(feature = "host-error-detail") {
            assert_eq!(
                response,
                crate::denied("denied: USER_REJECTED: the owner declined")
            );
        }
        // The next call replaces the reason, as on the real host.
        host.sign_payload(&sign_request(b"plain", None)).unwrap();
        assert_eq!(sdk::last_host_reason(), None);
        assert_eq!(
            host.sign_payload(&sign_request(b"plain", None))
                .unwrap_err(),
            SdkError::Message("FakeHost: no reply scripted for sign_payload".into())
        );
    }

    #[test]
    fn a_batch_must_come_back_with_one_signature_per_payload() {
        let mut host = FakeHost::new();
        let request = PayloadBatchSignRequest {
            wallet: "alice".into(),
            payloads: vec![
                PayloadSignItem {
                    preimage: b"a".to_vec(),
                    claimed_hash: [1; 32],
                },
                PayloadSignItem {
                    preimage: b"b".to_vec(),
                    claimed_hash: [2; 32],
                },
            ],
            signature_algorithm: "ed25519-message".into(),
            operation_class: "transfer.send".into(),
            petal_use_claim_jcs: Vec::new(),
            claim_assurance_evidence: None,
            approval_hint: None,
            action: None,
            advisory: None,
            selector: SignSelector::Reusable,
            key_ref_jcs: None,
        };
        host.reply_sign_batch(Ok(SignBatchOutcome::Signatures(vec![vec![1]])))
            .reply_sign_batch(Ok(SignBatchOutcome::Signatures(vec![vec![1], vec![2]])));
        assert!(matches!(
            host.sign_payload_batch(&request).unwrap_err(),
            SdkError::Message(message) if message.contains("wrong payload batch signature count")
        ));
        assert_eq!(
            host.sign_payload_batch(&request).unwrap(),
            SignBatchOutcome::Signatures(vec![vec![1], vec![2]])
        );
        // The SDK refuses an empty batch before Bloom sees it.
        let empty = PayloadBatchSignRequest {
            payloads: Vec::new(),
            ..request
        };
        assert!(host.sign_payload_batch(&empty).is_err());
        assert_eq!(host.batch_sign_requests().len(), 2);
    }

    #[test]
    fn the_outbox_answers_with_the_scripted_stages() {
        let mut host = FakeHost::new();
        let staged = StagedTransaction {
            outbox_id: "0001-abc".into(),
            plan_md: "send".into(),
            approval: None,
        };
        host.reply_tx_stage(Ok(staged.clone()))
            .reply_tx_confirm(Err("denied: APPROVAL_NOT_FOUND: approval does not exist"))
            .reply_tx_inspect(Ok(OutboxInspection {
                outbox_id: "0001-abc".into(),
                state: "pending".into(),
                tx_hash: None,
                receipt_json: None,
            }));
        assert_eq!(host.tx_stage(&transaction()).unwrap(), staged);
        assert_eq!(
            host.tx_confirm("alice", "base", "0001-abc", false)
                .unwrap_err(),
            SdkError::Host(HostStatus::Denied)
        );
        assert_eq!(
            host.tx_inspect("alice", "base", "0001-abc").unwrap().state,
            "pending"
        );
        assert_eq!(host.staged(), [transaction()]);
        assert_eq!(
            host.calls(),
            [
                "tx_stage alice base 0x1111111111111111111111111111111111111111",
                "tx_confirm alice base 0001-abc",
                "tx_inspect alice base 0001-abc",
            ]
        );
        // An on-chain address is not a wallet id; the SDK says so first.
        let mut wrong = transaction();
        wrong.wallet = wrong.to.clone();
        assert!(matches!(host.tx_stage(&wrong), Err(SdkError::Message(_))));
        assert_eq!(host.staged().len(), 1);
    }

    #[test]
    fn a_key_request_round_trips_through_derive_key() {
        let mut host = FakeHost::new();
        let outcome = PetalKeyOutcome::Pending {
            operation_id: "op-1".into(),
            scope_digest: "d".into(),
        };
        host.reply_key(Ok(outcome.clone()));
        let request = PetalKeyRequest {
            wallet_id: "alice".into(),
            key_slot: "agent".into(),
            allowed_routes: vec!["r000001".into()],
            allowed_operation_classes: vec!["trade.order".into()],
            allowed_crypto_suites: vec!["secp256k1-keccak256-recoverable".into()],
            maximum_lifetime_ms: 1,
        };
        assert_eq!(host.derive_key(&request).unwrap(), outcome);
        let sent: PetalKeyRequest = serde_json::from_slice(&host.key_requests()[0]).unwrap();
        assert_eq!(sent, request);
        assert_eq!(host.calls(), ["request_key"]);
    }

    #[test]
    fn fail_next_fails_one_call_with_bloom_s_message() {
        let mut host = FakeHost::new();
        host.fail_next("store_put", "backend: store put: disk full")
            .fail_next("store_get_opt", "denied: store");
        assert_eq!(
            host.store_put("a", b"1", false).unwrap_err(),
            SdkError::Message("backend: store put: disk full".into())
        );
        assert_eq!(host.state("a"), None);
        host.store_put("a", b"1", false).unwrap();
        // `store_get` is built on `store_get_opt`, so it fails with it.
        assert_eq!(
            host.store_get("a", 8).unwrap_err(),
            SdkError::Host(HostStatus::Denied)
        );
        assert_eq!(host.store_get("a", 8).unwrap(), b"1");
        assert_eq!(
            host.calls(),
            [
                "store_put a",
                "store_put a",
                "store_get_opt a",
                "store_get_opt a"
            ]
        );
    }

    #[test]
    fn a_petal_s_own_test_host_builds_bloom_s_errors() {
        assert_eq!(
            bloom_error("not found: chain bogus"),
            SdkError::Host(HostStatus::NotFound)
        );
        if cfg!(feature = "host-error-detail") {
            assert_eq!(
                sdk::describe_error(&SdkError::Host(HostStatus::NotFound)),
                "not found: chain bogus"
            );
        }
        assert_eq!(
            bloom_error("backend: timeout"),
            SdkError::Message("backend: timeout".into())
        );
    }

    #[test]
    #[should_panic(expected = "is not a host call")]
    fn fail_next_refuses_a_name_that_is_not_a_host_call() {
        FakeHost::new().fail_next("store_get", "denied: store");
    }

    #[test]
    fn every_host_method_can_be_failed_by_name() {
        // The list `fail_next` accepts must be the names `call` is given.
        let mut host = FakeHost::new();
        for name in HOST_CALLS {
            host.fail_next(name, "denied: scripted");
        }
        let denied = SdkError::Host(HostStatus::Denied);
        let request = HttpRequest {
            method: "GET".into(),
            url: "https://x/".into(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        let key = br#"{"wallet_id":"alice","key_slot":"a","allowed_routes":[],"allowed_operation_classes":[],"allowed_crypto_suites":[],"maximum_lifetime_ms":1}"#;
        let batch = PayloadBatchSignRequest {
            wallet: "alice".into(),
            payloads: vec![PayloadSignItem {
                preimage: b"a".to_vec(),
                claimed_hash: [1; 32],
            }],
            signature_algorithm: "ed25519-message".into(),
            operation_class: "transfer.send".into(),
            petal_use_claim_jcs: Vec::new(),
            claim_assurance_evidence: None,
            approval_hint: None,
            action: None,
            advisory: None,
            selector: SignSelector::Reusable,
            key_ref_jcs: None,
        };
        assert_eq!(host.http_fetch(&request, 1).unwrap_err(), denied);
        assert_eq!(host.request_key(key).unwrap_err(), denied);
        assert_eq!(
            host.sign_payload(&sign_request(b"p", None)).unwrap_err(),
            denied
        );
        assert_eq!(host.sign_payload_batch(&batch).unwrap_err(), denied);
        assert_eq!(host.tx_stage(&transaction()).unwrap_err(), denied);
        assert_eq!(
            host.tx_confirm("alice", "base", "1", false).unwrap_err(),
            denied
        );
        assert_eq!(host.tx_inspect("alice", "base", "1").unwrap_err(), denied);
        assert_eq!(
            host.chain_read("base", "eth_chainId", "[]").unwrap_err(),
            denied
        );
        assert_eq!(host.store_get_opt("a", 1).unwrap_err(), denied);
        assert_eq!(host.store_get_secret("a", 1).unwrap_err(), denied);
        assert_eq!(host.store_put("a", b"", false).unwrap_err(), denied);
        assert_eq!(host.store_put_new("a", b"", false).unwrap_err(), denied);
        assert_eq!(host.store_del("a").unwrap_err(), denied);
        assert_eq!(host.store_del_if_value("a", b"").unwrap_err(), denied);
        assert_eq!(host.store_list("", 1).unwrap_err(), denied);
        assert_eq!(host.vfs_read("a", 1).unwrap_err(), denied);
        assert_eq!(host.vfs_write("a", b"").unwrap_err(), denied);
        assert_eq!(host.vfs_list("a", 1).unwrap_err(), denied);
        assert_eq!(host.try_now_ms().unwrap_err(), denied);
        assert_eq!(host.random_bytes(1).unwrap_err(), denied);
        assert_eq!(host.runtime_setting("a").unwrap_err(), denied);
        assert_eq!(host.calls().len(), HOST_CALLS.len());
        // `now_ms` hides a failed clock as 0, as the SDK's does.
        host.fail_next("try_now_ms", "backend: clock");
        assert_eq!(host.now_ms(), 0);
        assert_eq!(host.now_ms(), FakeHost::START_MS);
    }

    #[test]
    fn the_log_names_what_was_addressed_and_never_a_value() {
        let mut host = FakeHost::new();
        host.store_put("agent/alice.key", b"SECRET-BYTES", true)
            .unwrap();
        host.store_get_secret("agent/alice.key", 64).unwrap();
        host.vfs_write("wallets/alice/0/note", b"SECRET-BYTES")
            .unwrap();
        let log = host.calls().join("\n");
        assert_eq!(
            log,
            "store_put agent/alice.key\nstore_get_secret agent/alice.key\nvfs_write wallets/alice/0/note"
        );
        assert!(!log.contains("SECRET"));
    }

    #[test]
    fn the_vfs_serves_seeded_files_and_records_writes() {
        let mut host = FakeHost::new();
        let account = wallet_account("alice", 0);
        host.with_file(
            &account.address_evm_path(),
            b"0x52908400098527886E0F7030069857D2E4169EE7\n",
        )
        .with_file("wallets/alice/0/chains/base/balance", b"5");
        // Any host is a `VfsRead`.
        assert_eq!(
            account.address_evm_from(&mut host).unwrap(),
            "0x52908400098527886E0F7030069857D2E4169EE7"
        );
        assert_eq!(code(&account.address_sol_from(&mut host).unwrap_err()), -1);
        assert_eq!(
            host.vfs_list("wallets/alice/0", 1024).unwrap(),
            ["address.evm", "chains"]
        );
        assert_eq!(
            host.vfs_list("wallets/bob", 1024).unwrap_err(),
            SdkError::Host(HostStatus::NotFound)
        );
        host.vfs_write(
            "wallets/alice/0/chains/base/outbox/pending/1/confirm",
            b"confirm",
        )
        .unwrap();
        assert_eq!(host.vfs_writes().len(), 1);
        // A write is a request, not a file.
        assert!(
            host.vfs_read("wallets/alice/0/chains/base/outbox/pending/1/confirm", 64)
                .is_err()
        );
    }

    #[test]
    fn the_clock_settings_and_random_bytes_are_deterministic() {
        let mut host = FakeHost::new();
        assert_eq!(host.try_now_ms().unwrap(), FakeHost::START_MS);
        host.advance_ms(1_000);
        assert_eq!(host.now_ms(), FakeHost::START_MS + 1_000);
        host.set_now_ms(5);
        assert_eq!(host.now_ms(), 5);
        host.with_setting("api_base", "https://x");
        assert_eq!(
            host.runtime_setting("api_base").unwrap().as_deref(),
            Some("https://x")
        );
        assert_eq!(host.runtime_setting("missing").unwrap(), None);
        assert_eq!(host.random_bytes(3).unwrap(), [1, 1, 1]);
        assert_eq!(host.random_bytes(2).unwrap(), [2, 2]);
    }
}
