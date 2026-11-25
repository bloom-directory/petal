//! Concrete regressions for SPEC.md SPLIT-R1..SPLIT-R3. These are tests, not
//! proofs; they run against the host build of the route crate.

use crate::split_even;

fn holds(total: u64) {
    let (first, second) = split_even(total);
    // Mathematical integers: u128 cannot overflow for two u64 summands.
    assert_eq!(u128::from(first) + u128::from(second), u128::from(total));
    assert!(first <= second);
    assert!(u128::from(second) - u128::from(first) <= 1);
}

#[test]
fn zero() {
    assert_eq!(split_even(0), (0, 0));
    holds(0);
}

#[test]
fn odd_totals() {
    assert_eq!(split_even(1), (0, 1));
    assert_eq!(split_even(7), (3, 4));
    for total in [3, 5, 1_000_001, u64::MAX - 2] {
        holds(total);
    }
}

#[test]
fn maximum() {
    assert_eq!(split_even(u64::MAX), (u64::MAX / 2, u64::MAX / 2 + 1));
    holds(u64::MAX);
}

#[test]
fn even_totals() {
    assert_eq!(split_even(10), (5, 5));
    holds(u64::MAX - 1);
}
