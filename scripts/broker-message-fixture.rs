//! Test adapter inserted into a disposable copy of the actual Broker reader.
//! No SDK decoding algorithm, custody state or authorization is used here.
use std::{fs, path::PathBuf};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    preimage_hex: String,
    hash_hex: String,
    witness: serde_json::Value,
}

#[test]
fn producer_bytes_match_actual_broker_reading() {
    let source = PathBuf::from(std::env::var_os("PETAL_PUBLIC_FIXTURE").unwrap());
    assert!(fs::metadata(&source).unwrap().len() <= 8 * 1024 * 1024);
    let fixtures: Vec<Fixture> = serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
    assert!(!fixtures.is_empty() && fixtures.len() <= 128);
    let signer = std::env::var("PETAL_PREVIEW_SIGNER").unwrap().parse().unwrap();
    let mut readings = Vec::new();
    for fixture in fixtures {
        let bytes = serde_json::to_vec(&fixture.witness).unwrap();
        let (preimage, review) = super::decode(&bytes, signer).unwrap();
        assert_eq!(preimage, hex::decode(&fixture.preimage_hex).unwrap(),
            "producer payload differs from the independently rebuilt review");
        assert_eq!(review.payload_keccak.trim_start_matches("0x"), fixture.hash_hex,
            "producer hash differs from the independently rebuilt review");
        readings.push(review);
    }
    fs::write(std::env::var_os("PETAL_BROKER_READINGS").unwrap(),
        serde_json::to_vec_pretty(&readings).unwrap()).unwrap();
}
