//! Pure execution logic: no database, no transport. Everything here is
//! unit-tested without tokio or Postgres.

pub mod dag;
pub mod diagnosis;
pub mod policy;
pub mod state;
