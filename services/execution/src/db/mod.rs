//! Repositories, one per aggregate. Enum-like columns hold the proto name
//! without its prefix; `convert` translates them.

pub mod approvals;
pub mod events;
pub mod executions;
pub mod identities;
pub mod policies;
pub mod steps;

pub type Tx = sqlx::Transaction<'static, sqlx::Postgres>;
