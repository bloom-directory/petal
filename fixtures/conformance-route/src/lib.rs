//! Domain code shared by the canonical conformance routes.

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes).into()
}

pub use alloy_primitives;
pub use serde_json;
