//! Domain code shared by the canonical conformance routes.

pub fn typed_mail() -> petal::evm_messages::TypedData {
    serde_json::from_str(include_str!(
        "../../../crates/petal-sdk/examples/eip712-mail.json"
    ))
    .expect("the checked-in public Ether Mail vector is valid typed data")
}
