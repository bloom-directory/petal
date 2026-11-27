//! Fixtures for testing route and domain code off `wasm32`.
//!
//! Every Petal that tests a `Ctx`-taking function has hand-rolled these. The
//! local `impl RouteIdentity` stays with the test, since it documents the route
//! under test, but the `RawCtx` assembly and a fake host do not need repeating.
//!
//! `FakeHost` and its helpers exist only with the SDK's `testing` feature.
//! Enable it under `[dev-dependencies]`, so they never compile into a
//! component. The older fixtures here, [`RouteCtx`], [`MapVfs`] and
//! [`wallet_account`], still compile without the feature, so tests written
//! against earlier SDKs keep building. A later release moves them behind it
//! too.
//!
//! Production code must not reach for this module: [`crate::wallet_account`]
//! is the only way a route should obtain a [`WalletAccount`].

use crate::{Ctx, HostStatus, RawCtx, RouteIdentity, SdkError, VfsRead, WalletAccount};
use std::collections::BTreeMap;

#[cfg(any(test, feature = "testing"))]
mod fake_host;
#[cfg(any(test, feature = "testing"))]
pub use fake_host::{FakeHost, bloom_error, eth_call_reply, http_response};

/// A well-formed package hash for tests: 64 lowercase hex digits, as Bloom
/// supplies.
pub const PACKAGE_HASH: &str = "aa00000000000000000000000000000000000000000000000000000000000001";
/// The trusted route id [`RouteCtx`] supplies unless told otherwise.
pub const ROUTE_ID: &str = "r000001";

/// Assemble a [`Ctx`] for a route under test. It starts with a valid
/// [`PACKAGE_HASH`] and the trusted `bloom.route_id` [`ROUTE_ID`], so a
/// signing route can build its claim; change them with
/// [`RouteCtx::package_hash`], [`RouteCtx::route_id`] and
/// [`RouteCtx::without_route_id`].
pub struct RouteCtx {
    path: String,
    params: Vec<(String, String)>,
    petal_root: String,
    package_hash: String,
    actor: Option<String>,
}

impl RouteCtx {
    /// `path` is the concrete request path, e.g. `swaps/alice/1/new`.
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            params: Vec::new(),
            petal_root: "test".to_owned(),
            package_hash: PACKAGE_HASH.to_owned(),
            actor: None,
        }
        .route_id(ROUTE_ID)
    }

    /// Set the trusted `bloom.route_id`, replacing any earlier one.
    pub fn route_id(self, route_id: &str) -> Self {
        let mut ctx = self.without_route_id();
        ctx.params
            .push(("bloom.route_id".to_owned(), route_id.to_owned()));
        ctx
    }

    /// Drop the trusted `bloom.route_id`, as on a context Bloom did not
    /// dispatch.
    pub fn without_route_id(mut self) -> Self {
        self.params.retain(|(name, _)| name != "bloom.route_id");
        self
    }

    /// Inject the trusted `bloom.wallet` and `bloom.account` pair Bloom
    /// supplies on an account-mounted route.
    pub fn account(mut self, wallet: &str, number: u32) -> Self {
        self.params
            .push(("bloom.wallet".to_owned(), wallet.to_owned()));
        self.params
            .push(("bloom.account".to_owned(), number.to_string()));
        self
    }

    /// Add one raw parameter, trusted or caller-supplied.
    pub fn param(mut self, name: &str, value: &str) -> Self {
        self.params.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn package_hash(mut self, package_hash: &str) -> Self {
        self.package_hash = package_hash.to_owned();
        self
    }

    pub fn actor(mut self, actor: &str) -> Self {
        self.actor = Some(actor.to_owned());
        self
    }

    pub fn raw(self) -> RawCtx {
        RawCtx {
            petal_root: self.petal_root,
            package_hash: self.package_hash,
            path: self.path,
            params: self.params,
            actor: self.actor,
        }
    }

    pub fn bind<I: RouteIdentity>(self) -> Ctx {
        Ctx::bind::<I>(self.raw())
    }
}

/// An in-memory [`VfsRead`] so address resolution can be tested where the
/// real host imports are `unreachable!()`.
#[derive(Default)]
pub struct MapVfs {
    files: BTreeMap<String, Result<Vec<u8>, SdkError>>,
}

impl MapVfs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn file(mut self, path: &str, bytes: &[u8]) -> Self {
        self.files.insert(path.to_owned(), Ok(bytes.to_vec()));
        self
    }

    /// Make a path fail with a specific host status.
    pub fn failing(mut self, path: &str, error: SdkError) -> Self {
        self.files.insert(path.to_owned(), Err(error));
        self
    }
}

impl VfsRead for MapVfs {
    fn read(&mut self, path: &str, _max_bytes: usize) -> Result<Vec<u8>, SdkError> {
        match self.files.get(path) {
            Some(Ok(bytes)) => Ok(bytes.clone()),
            Some(Err(error)) => Err(error.clone()),
            None => Err(SdkError::Host(HostStatus::NotFound)),
        }
    }
}

/// A [`WalletAccount`] for domain code under test, bypassing [`Ctx`].
pub fn wallet_account(wallet: &str, number: u32) -> WalletAccount {
    WalletAccount::for_test(wallet, number)
}
