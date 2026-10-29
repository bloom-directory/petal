//! Prepared actions and the shared signing lifecycle.
//! The host derives route and account authority from the executing context.
use crate::{Ctx, validate_wallet_id};
pub use bloom_petal_contract::prepared::{
    Asset, CryptoSuite, Debit, ErrorCode, PreparedAction, ReviewArtifact, SigningError,
    SigningOutcome, SigningPayload, Spend,
};
use bloom_petal_contract::prepared::{MAX_REQUEST, Request, StatusRequest, VERSION};

fn invalid(message: impl Into<String>) -> SigningError {
    SigningError {
        code: ErrorCode::Invalid,
        message: message.into(),
    }
}
fn backend(message: impl Into<String>) -> SigningError {
    SigningError {
        code: ErrorCode::Backend,
        message: message.into(),
    }
}
fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, SigningError> {
    let bytes = serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > MAX_REQUEST {
        return Err(invalid("prepared request exceeds limit"));
    }
    Ok(bytes)
}
fn decode(result: Result<Vec<u8>, Vec<u8>>) -> Result<SigningOutcome, SigningError> {
    match result {
        Ok(bytes) if bytes.len() <= MAX_REQUEST => {
            serde_json::from_slice(&bytes).map_err(|_| backend("invalid prepared signing response"))
        }
        Err(bytes) if bytes.len() <= MAX_REQUEST => Err(serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| backend("invalid prepared signing error"))),
        _ => Err(backend("prepared signing response exceeds limit")),
    }
}
/// Sign a single prepared action. Retrying the same operation key resumes it;
/// changing a frozen action is a host-reported conflict.
pub fn sign(
    _ctx: &Ctx,
    wallet: &str,
    action: &PreparedAction,
    operation_key: &str,
) -> Result<SigningOutcome, SigningError> {
    validate_wallet_id(wallet).map_err(invalid)?;
    let request = Request {
        version: VERSION,
        wallet: wallet.into(),
        operation_key: operation_key.into(),
        action: action.clone(),
    };
    request.validate()?;
    decode(crate::bindings::bloom::prepared_sign::signing::sign(
        &encode(&request)?,
    ))
}
/// Read an operation using the executing route's authenticated namespace.
pub fn status(
    _ctx: &Ctx,
    wallet: &str,
    operation_key: &str,
) -> Result<SigningOutcome, SigningError> {
    validate_wallet_id(wallet).map_err(invalid)?;
    let request = StatusRequest {
        version: VERSION,
        wallet: wallet.into(),
        operation_key: operation_key.into(),
    };
    request.validate()?;
    decode(crate::bindings::bloom::prepared_sign::status::status(
        &encode(&request)?,
    ))
}
