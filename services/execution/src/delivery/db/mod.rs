//! Repositories of the delivery tables (migration `0005_delivery`). Like
//! the rest of `crate::db`, every function takes the workspace it operates
//! in (CONTRACT.md §18): an id alone never addresses a row.

pub mod environments;
pub mod registries;
pub mod releases;
pub mod runs;
