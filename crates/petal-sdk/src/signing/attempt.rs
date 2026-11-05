//! Immutable attempts keep retries on the same Machine operation.
use super::*;
use crate::{DispatchResponse, HostStatus, SdkError, sdk};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
const MAX_GENERATIONS: u32 = 32;
const MAX_CONTEXT: usize = 64 * 1024;
const SCHEMA: &str = "bloom.petal.attempt.v1";

/// Host seam for production routes and deterministic lifecycle tests.
pub trait SigningHost {
    fn package_hash(&self) -> &str;
    fn store_get(&mut self, key: &str) -> Result<Option<Vec<u8>>, SigningError>;
    fn store_put_new(&mut self, key: &str, value: &[u8]) -> Result<bool, SigningError>;
    fn sign(
        &mut self,
        wallet: &str,
        action: &PreparedAction,
        operation_key: &str,
    ) -> Result<SigningOutcome, SigningError>;
    fn status(&mut self, wallet: &str, operation_key: &str)
    -> Result<SigningOutcome, SigningError>;
    fn now_ms(&mut self) -> Result<u64, SigningError>;
}
pub struct CtxHost<'a>(pub &'a Ctx);
impl SigningHost for CtxHost<'_> {
    fn package_hash(&self) -> &str {
        &self.0.package_hash
    }
    fn store_get(&mut self, key: &str) -> Result<Option<Vec<u8>>, SigningError> {
        match sdk::store_get(key, MAX_REQUEST + MAX_CONTEXT) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(SdkError::Host(HostStatus::NotFound)) => Ok(None),
            Err(e) => Err(backend(e.to_string())),
        }
    }
    fn store_put_new(&mut self, key: &str, value: &[u8]) -> Result<bool, SigningError> {
        match sdk::store_put_new(key, value, false) {
            Ok(()) => Ok(true),
            Err(SdkError::Host(HostStatus::Denied)) => Ok(false),
            Err(e) => Err(backend(e.to_string())),
        }
    }
    fn sign(
        &mut self,
        wallet: &str,
        action: &PreparedAction,
        key: &str,
    ) -> Result<SigningOutcome, SigningError> {
        super::sign(self.0, wallet, action, key)
    }
    fn status(&mut self, wallet: &str, key: &str) -> Result<SigningOutcome, SigningError> {
        super::status(self.0, wallet, key)
    }
    fn now_ms(&mut self) -> Result<u64, SigningError> {
        sdk::try_now_ms().map_err(|e| backend(e.to_string()))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record<C> {
    schema: String,
    package_hash: String,
    generation: u32,
    action: PreparedAction,
    context: C,
}
/// Frozen signing input and Petal-owned submission context. Never stores outcomes.
#[derive(Debug)]
pub struct Attempt<C = ()> {
    scope: String,
    record: Record<C>,
}
impl<C: Serialize + DeserializeOwned> Attempt<C> {
    pub fn open(
        h: &mut impl SigningHost,
        scope: &str,
        build: impl FnOnce() -> Result<(PreparedAction, C), SigningError>,
    ) -> Result<Self, SigningError> {
        if let Some(existing) = Self::load(h, scope)? {
            return Ok(existing);
        }
        Self::create(h, scope, 0, build()?)
    }
    pub fn load(h: &mut impl SigningHost, scope: &str) -> Result<Option<Self>, SigningError> {
        // Reserve enough room for every generation's operation key.
        if scope.is_empty() || scope.len() + 3 > 256 || scope.chars().any(char::is_control) {
            return Err(invalid("invalid attempt scope"));
        }
        let mut latest = None;
        for n in 0..MAX_GENERATIONS {
            let Some(bytes) = h.store_get(&format!("{scope}.{n}.json"))? else {
                break;
            };
            let record: Record<C> =
                serde_json::from_slice(&bytes).map_err(|_| backend("invalid attempt record"))?;
            if record.schema != SCHEMA
                || record.generation != n
                || record.package_hash.is_empty()
                || serde_json::to_vec(&record.context)
                    .map_err(|e| backend(e.to_string()))?
                    .len()
                    > MAX_CONTEXT
            {
                return Err(backend("invalid attempt record"));
            }
            record
                .action
                .validate()
                .map_err(|_| backend("invalid attempt action"))?;
            latest = Some(Self {
                scope: scope.into(),
                record,
            });
        }
        Ok(latest)
    }
    fn create(
        h: &mut impl SigningHost,
        scope: &str,
        n: u32,
        (action, context): (PreparedAction, C),
    ) -> Result<Self, SigningError> {
        action.validate()?;
        if serde_json::to_vec(&context)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > MAX_CONTEXT
        {
            return Err(invalid("attempt context exceeds 64 KiB"));
        }
        let attempt = Self {
            scope: scope.into(),
            record: Record {
                schema: SCHEMA.into(),
                package_hash: h.package_hash().into(),
                generation: n,
                action,
                context,
            },
        };
        let bytes = serde_json::to_vec(&attempt.record).map_err(|e| invalid(e.to_string()))?;
        if h.store_put_new(&format!("{scope}.{n}.json"), &bytes)? {
            return Ok(attempt);
        }
        Self::load(h, scope)?
            .ok_or_else(|| backend("attempt write lost without an existing record"))
    }
    pub fn action(&self) -> &PreparedAction {
        &self.record.action
    }
    pub fn context(&self) -> &C {
        &self.record.context
    }
    pub fn generation(&self) -> u32 {
        self.record.generation
    }
    pub fn operation_key(&self) -> String {
        format!("{}#{}", self.scope, self.generation())
    }
    pub fn state(
        &self,
        h: &mut impl SigningHost,
        wallet: &str,
    ) -> Result<AttemptState, SigningError> {
        if self.record.package_hash != h.package_hash() {
            return Ok(AttemptState::PackageChanged);
        }
        normalize(h.status(wallet, &self.operation_key()))
    }
    pub fn renew(
        self,
        h: &mut impl SigningHost,
        wallet: &str,
        build: impl FnOnce(&Self) -> Result<(PreparedAction, C), SigningError>,
    ) -> Result<Self, SignHalt> {
        let latest = Self::load(h, &self.scope)?.ok_or_else(|| backend("attempt history lost"))?;
        if latest.generation() < self.generation() {
            return Err(backend("attempt history lost").into());
        }
        if latest.generation() > self.generation() {
            return Ok(latest);
        }
        let state = latest.state(h, wallet)?;
        if !matches!(state, AttemptState::Expired | AttemptState::Cancelled)
            && !(matches!(state, AttemptState::Signed | AttemptState::PackageChanged)
                && latest.action().spend == Spend::None)
        {
            return Err(SignHalt::state(
                state,
                "cannot renew; reconcile or retry the same write",
            ));
        }
        let n = latest.generation() + 1;
        if n >= MAX_GENERATIONS {
            return Err(invalid("attempt generation limit; create a new scope").into());
        }
        let built = build(&latest)?;
        // Zero-spend refresh must not become a new spending authorization.
        if matches!(state, AttemptState::Signed | AttemptState::PackageChanged)
            && built.0.spend != Spend::None
        {
            return Err(invalid("zero-spend renewal must remain zero spend").into());
        }
        Ok(Self::create(h, &latest.scope, n, built)?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "retry_state", rename_all = "snake_case")]
pub enum AttemptState {
    NotStarted,
    Pending { action_id: String, expires_ms: u64 },
    Approved,
    Signed,
    Expired,
    Cancelled,
    Denied,
    RecoveryRequired { status: String },
    PackageChanged,
}
fn normalize(result: Result<SigningOutcome, SigningError>) -> Result<AttemptState, SigningError> {
    Ok(match result {
        Ok(SigningOutcome::Signed { .. }) => AttemptState::Signed,
        Ok(SigningOutcome::ApprovalPending {
            action_id,
            expires_ms,
        }) => AttemptState::Pending {
            action_id,
            expires_ms,
        },
        Ok(SigningOutcome::RecoveryRequired { status })
            if status == "approved_retry" || status == "eligibility_approved_retry" =>
        {
            AttemptState::Approved
        }
        Ok(SigningOutcome::RecoveryRequired { status }) => {
            AttemptState::RecoveryRequired { status }
        }
        Err(e) => match e.code {
            ErrorCode::NotFound => AttemptState::NotStarted,
            ErrorCode::Expired => AttemptState::Expired,
            ErrorCode::Cancelled => AttemptState::Cancelled,
            ErrorCode::Denied => AttemptState::Denied,
            _ => return Err(e),
        },
    })
}
#[derive(Clone, Debug)]
pub struct SignHalt {
    pub state: Option<AttemptState>,
    pub message: String,
    pub code: i32,
}
impl SignHalt {
    fn state(state: AttemptState, message: &str) -> Self {
        let code = if state == AttemptState::Denied {
            -2
        } else {
            -3
        };
        Self {
            message: format!("{state:?}: {message}"),
            state: Some(state),
            code,
        }
    }
}
impl From<SigningError> for SignHalt {
    fn from(e: SigningError) -> Self {
        let code = match e.code {
            ErrorCode::Denied => -2,
            ErrorCode::Backend | ErrorCode::Conflict => -4,
            _ => -3,
        };
        Self {
            state: None,
            message: e.message,
            code,
        }
    }
}
impl From<SignHalt> for DispatchResponse {
    fn from(e: SignHalt) -> Self {
        crate::error(e.code, e.message)
    }
}
impl From<SigningError> for DispatchResponse {
    fn from(e: SigningError) -> Self {
        SignHalt::from(e).into()
    }
}
pub fn sign_or_resume<C: Serialize + DeserializeOwned>(
    h: &mut impl SigningHost,
    wallet: &str,
    attempt: &Attempt<C>,
) -> Result<Vec<u8>, SignHalt> {
    if attempt.record.package_hash != h.package_hash() {
        return Err(SignHalt::state(
            AttemptState::PackageChanged,
            "earlier approval may remain open; reconcile before creating a new action",
        ));
    }
    match h.sign(wallet, attempt.action(), &attempt.operation_key()) {
        Ok(SigningOutcome::Signed { signature }) => Ok(signature),
        result => match normalize(result) {
            Ok(state) => Err(SignHalt::state(
                state,
                "retry the same write (retry_write_body) after approval; do not create a replacement",
            )),
            Err(e) => Err(e.into()),
        },
    }
}
/// Read-only projection for prepared-signing approval routes.
pub fn approval_read(ctx: &Ctx, wallet: &str, scopes: &[&str]) -> DispatchResponse {
    approval_read_with_host(&mut CtxHost(ctx), wallet, scopes)
}
fn approval_read_with_host(
    h: &mut impl SigningHost,
    wallet: &str,
    scopes: &[&str],
) -> DispatchResponse {
    let result = (|| -> Result<Option<serde_json::Value>, SigningError> {
        for scope in scopes {
            let Some(attempt) = Attempt::<serde_json::Value>::load(h, scope)? else {
                continue;
            };
            let state = attempt.state(h, wallet)?;
            if matches!(
                state,
                AttemptState::NotStarted | AttemptState::Signed | AttemptState::Denied
            ) {
                continue;
            }
            let mut json = serde_json::to_value(&state).map_err(|e| backend(e.to_string()))?;
            if let AttemptState::Pending { expires_ms, .. } = state {
                json["expired_locally"] = (h.now_ms()? >= expires_ms).into();
            }
            if state == AttemptState::PackageChanged {
                json["message"] = "earlier approval may still be open or charged; reconcile before creating a new action".into();
            }
            return Ok(Some(json));
        }
        Ok(None)
    })();
    match result {
        Ok(Some(json)) => crate::read_json_value(&json),
        Ok(None) => crate::error(-3, "no pending signing approval"),
        Err(e) => crate::error(-4, e.message),
    }
}

#[cfg(test)]
mod tests;
