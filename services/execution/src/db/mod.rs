//! Repositories, one per aggregate. Enum-like columns hold the proto name
//! without its prefix; `convert` translates them. Every function takes the
//! workspace it operates in (CONTRACT.md §18): an id alone never addresses
//! a row. The one exception, `executions::with_status`, is startup recovery
//! across every workspace.

pub mod approvals;
pub mod events;
pub mod executions;
pub mod identities;
pub mod policies;
pub mod steps;
pub mod workspace;

pub type Tx = sqlx::Transaction<'static, sqlx::Postgres>;
