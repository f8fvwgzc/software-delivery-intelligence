//! Pure logic: no database, no network, no clock. Everything here is
//! unit-tested with plain values so the repositories stay thin.

pub mod diff;
pub mod failure;
pub mod impact;
pub mod merge;
pub mod security;
pub mod similarity;
pub mod stats;
pub mod temporal;
