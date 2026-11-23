//! Concrete regressions; tests, not proofs.

use crate::byte_sum;

#[test]
fn sums() {
    assert_eq!(byte_sum(b""), 0);
    assert_eq!(byte_sum(b"a"), 97);
    assert_eq!(byte_sum(&[255; 1000]), 255_000);
    let v: Vec<u8> = (0..=255).collect();
    assert_eq!(byte_sum(&v), 255 * 256 / 2);
}
