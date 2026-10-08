//! Pure logic: no database, no network, no clock. Everything here is
//! unit-tested with plain values so the repositories stay thin.

pub mod failure;
pub mod impact;
pub mod similarity;
pub mod stats;
pub mod temporal;
