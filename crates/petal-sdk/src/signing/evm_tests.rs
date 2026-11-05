use super::*;
use alloy_primitives::{Address, U256};
#[test]
fn eip712_standard_vector_binds_review_and_payload() {
    let mut data = serde_json::json!({
        "types": {
            "EIP712Domain": [{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],
            "Person": [{"name":"name","type":"string"},{"name":"wallet","type":"address"}],
            "Mail": [{"name":"from","type":"Person"},{"name":"to","type":"Person"},{"name":"contents","type":"string"}]
        },
        "primaryType":"Mail",
        "domain":{"name":"Ether Mail","version":"1","chainId":1,"verifyingContract":"0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC"},
        "message": {"from":{"name":"Cow","wallet":"0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826"},"to":{"name":"Bob","wallet":"0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB"},"contents":"Hello, Bob!"}
    });
    let a = eip712_action("mail", &data).unwrap();
    assert_eq!(
        format!("{:x}", alloy_primitives::B256::from(a.payload.hash)),
        "be609aee343fb3c4b28e1df9e632fca64fcfaede20f02e86244efddf30957bd2"
    );
    assert_eq!(a.payload.preimage.len(), 66);
    assert_eq!(a.spend, Spend::Unspecified);
    let review: serde_json::Value = serde_json::from_slice(&a.review.bytes).unwrap();
    assert_eq!(review["typed_data"], data);
    assert_eq!(review["schema"], "bloom.message.review.v1");
    data["message"]["contents"] = "different".into();
    assert_ne!(
        a.payload.hash,
        eip712_action("mail", &data).unwrap().payload.hash
    );
}
#[test]
fn raw_erc20_debit_is_canonical_and_nonzero() {
    let token = Address::repeat_byte(0xab);
    let amount = U256::MAX;
    assert_eq!(
        erc20_spend(137, token, amount).unwrap(),
        Spend::erc20(
            "evm-137",
            "0xabababababababababababababababababababab",
            amount.to_string()
        )
        .unwrap()
    );
    assert!(erc20_spend(137, token, U256::ZERO).is_err());
    assert!(erc20_spend(0, token, U256::from(1)).is_err());
}
