//! Emit the existing Broker producer-fixture shape. Encoding is not authority:
//! a real route obtains its ClaimOrigin and account through trusted Ctx helpers.
use petal::{
    ClaimEffects, ClaimNonce, ClaimOrigin,
    evm_messages::{EvmMessage, TypedData},
};

fn main() -> Result<(), String> {
    let origin = ClaimOrigin::new(&"ab".repeat(32), "r000001")?;
    let mail: TypedData = serde_json::from_str(include_str!("eip712-mail.json"))
        .map_err(|error| error.to_string())?;
    let mut fixtures = Vec::new();
    for (message, class) in [
        (
            EvmMessage::personal("Bloom supported-message example"),
            "example.personal",
        ),
        (EvmMessage::eip712(&mail), "example.typed_data"),
    ] {
        let request = message
            .and_then(|message| {
                message.prepare(
                    &origin,
                    "example",
                    class,
                    &ClaimEffects::moves_nothing(),
                    ClaimNonce::Explicit([1; 16]),
                )
            })
            .map_err(|error| format!("{error:?}"))?;
        let request = request.request();
        let review: serde_json::Value = serde_json::from_slice(request.action.as_ref().unwrap())
            .map_err(|error| error.to_string())?;
        fixtures.push(serde_json::json!({
            "preimage_hex": alloy_primitives::hex::encode(&request.preimage),
            "hash_hex": alloy_primitives::hex::encode(request.claimed_hash),
            "witness": review
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&fixtures).map_err(|error| error.to_string())?
    );
    Ok(())
}
