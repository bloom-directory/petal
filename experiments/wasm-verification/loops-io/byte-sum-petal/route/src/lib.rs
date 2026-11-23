//! Domain code for this Petal's route components.
//!
//! `sum` holds the loop verification target (loops-io experiment 1).

mod sum;

pub use sum::{byte_sum, byte_sum_target};

#[cfg(test)]
mod tests;
