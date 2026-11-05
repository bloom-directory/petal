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

mod attempt;
pub use attempt::{
    Attempt, AttemptState, CtxHost, SignHalt, SigningHost, approval_read, sign_or_resume,
};

/// Construct payload and review from the same EIP-712 typed data.
#[cfg(feature = "evm")]
pub fn eip712_action(
    class: &str,
    typed_data: &serde_json::Value,
) -> Result<PreparedAction, SigningError> {
    let typed: alloy_dyn_abi::TypedData =
        serde_json::from_value(typed_data.clone()).map_err(|e| invalid(e.to_string()))?;
    let mut preimage = vec![0x19, 0x01];
    preimage.extend_from_slice(typed.domain.separator().as_slice());
    if typed.primary_type != "EIP712Domain" {
        preimage.extend_from_slice(
            typed
                .hash_struct()
                .map_err(|e| invalid(e.to_string()))?
                .as_slice(),
        );
    }
    let payload = SigningPayload::keccak256(preimage)?;
    let review = ReviewArtifact::new("bloom.message.review", 1, serde_json::to_vec(&serde_json::json!({"schema":"bloom.message.review.v1", "kind":"eip712", "typed_data":typed_data})).map_err(|e| invalid(e.to_string()))?)?;
    PreparedAction::new(class, payload, review)
}
/// Declare a canonical raw-unit ERC-20 debit.
#[cfg(feature = "evm")]
pub fn erc20_spend(
    chain_id: u64,
    token: alloy_primitives::Address,
    amount: alloy_primitives::U256,
) -> Result<Spend, SigningError> {
    Spend::erc20(
        format!("evm-{chain_id}"),
        format!("{token:#x}"),
        amount.to_string(),
    )
}

#[cfg(all(test, feature = "evm"))]
mod evm_tests;
