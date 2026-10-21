//! Test adapter inserted into a disposable copy of the actual Broker reader.
//! No SDK decoding algorithm, custody state or authorization is used here.
use std::{fs, path::PathBuf};

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    preimage_hex: String,
    hash_hex: String,
    witness: serde_json::Value,
}

fn read_fixture(
    fixture: &Fixture,
    signer: alloy::primitives::Address,
) -> Result<super::MessageReview, String> {
    let bytes = serde_json::to_vec(&fixture.witness).map_err(|error| error.to_string())?;
    let (preimage, review) = super::decode(&bytes, signer).map_err(|error| format!("{error:?}"))?;
    if preimage != hex::decode(&fixture.preimage_hex).map_err(|error| error.to_string())? {
        return Err("producer payload differs from the independently rebuilt review".into());
    }
    if review.payload_keccak.trim_start_matches("0x") != fixture.hash_hex {
        return Err("producer hash differs from the independently rebuilt review".into());
    }
    Ok(review)
}

#[test]
fn producer_bytes_match_actual_broker_reading() {
    let source = PathBuf::from(std::env::var_os("PETAL_PUBLIC_FIXTURE").unwrap());
    assert!(fs::metadata(&source).unwrap().len() <= 8 * 1024 * 1024);
    let fixtures: Vec<Fixture> = serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
    assert!(!fixtures.is_empty() && fixtures.len() <= 128);
    let signer = std::env::var("PETAL_PREVIEW_SIGNER")
        .unwrap()
        .parse()
        .unwrap();
    let mut readings = Vec::new();
    for fixture in fixtures {
        let review = read_fixture(&fixture, signer).unwrap();
        let mut changed = fixture.clone();
        changed.preimage_hex.push_str("00");
        assert!(read_fixture(&changed, signer).is_err());
        let mut changed = fixture.clone();
        changed.hash_hex = "00".repeat(32);
        assert!(read_fixture(&changed, signer).is_err());
        let mut changed = fixture.clone();
        changed.witness["kind"] = "unsupported_sdk_example".into();
        assert!(read_fixture(&changed, signer).is_err());
        let mut changed = fixture.clone();
        let field = match fixture.witness["kind"].as_str() {
            Some("personal") => Some(&mut changed.witness["message"]),
            Some("eip712") => Some(&mut changed.witness["typed_data"]["message"]["contents"]),
            _ => None,
        };
        if let Some(field) = field {
            *field = serde_json::Value::String(format!("{field} changed"));
            assert!(read_fixture(&changed, signer).is_err());
        }
        if fixture.witness["kind"] == "eip712" {
            let mut changed = fixture.clone();
            changed.witness["typed_data"]["domain"]["chainId"] = "0x2".into();
            assert!(read_fixture(&changed, signer).is_err());
        }
        readings.push(review);
    }
    fs::write(
        std::env::var_os("PETAL_BROKER_READINGS").unwrap(),
        serde_json::to_vec_pretty(&readings).unwrap(),
    )
    .unwrap();
}
