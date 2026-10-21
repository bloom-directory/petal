//! Emit the existing Broker producer-fixture shape. Encoding is not authority:
//! a real route obtains its ClaimOrigin and account through trusted Ctx helpers.
use petal::{ClaimEffects, ClaimNonce, ClaimOrigin, evm_messages::EvmMessage};

fn main() -> Result<(), String> {
    let origin = ClaimOrigin::new(&"ab".repeat(32), "r000001")?;
    let request = EvmMessage::personal("Bloom supported-message example")
        .and_then(|message| {
            message.prepare(
                &origin,
                "example",
                "example.personal",
                &ClaimEffects::moves_nothing(),
                ClaimNonce::Explicit([1; 16]),
            )
        })
        .map_err(|error| format!("{error:?}"))?;
    let request = request.request();
    let review: serde_json::Value = serde_json::from_slice(request.action.as_ref().unwrap())
        .map_err(|error| error.to_string())?;
    let fixture = serde_json::json!([{
        "preimage_hex": alloy_primitives::hex::encode(&request.preimage),
        "hash_hex": alloy_primitives::hex::encode(request.claimed_hash),
        "witness": review
    }]);
    println!(
        "{}",
        serde_json::to_string_pretty(&fixture).map_err(|error| error.to_string())?
    );
    Ok(())
}
