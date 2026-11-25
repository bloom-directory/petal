//! Domain code shared by this Petal's route components.
//!
//! `split` holds the verification target. See `../../SPEC.md` (SPLIT-*).

mod split;

pub use split::{Shares, split_even, split_even_target};

#[cfg(test)]
mod tests;
