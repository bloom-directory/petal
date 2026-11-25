//! The verification target for SPEC.md SPLIT-1..SPLIT-4.
//!
//! The emitted implementation of `split_even_target` inside the packaged
//! component is what the bytecode verifier analyzes. It is exported from the
//! core module under a stable name only so that a decoder can locate it in a
//! symbol-stripped build; the route reaches the same function through an
//! ordinary call. Do not change it without refreshing the evidence.

/// Two shares of an allocation total, first then second.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shares {
    pub first: u64,
    pub second: u64,
}

/// Splits `total` into two shares whose sum is `total`, with the first no
/// larger than the second and the two differing by at most one.
#[inline(never)]
#[unsafe(export_name = "bloom_verify_split_even")]
pub extern "C" fn split_even_target(total: u64) -> Shares {
    let first = total / 2;
    Shares {
        first,
        second: total - first,
    }
}

/// The benchmark's Rust-level signature, a view of [`split_even_target`].
pub fn split_even(total: u64) -> (u64, u64) {
    let shares = split_even_target(total);
    (shares.first, shares.second)
}
