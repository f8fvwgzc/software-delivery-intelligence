//! Repositories, one per aggregate. Role and kind columns hold the proto
//! name without its prefix; `convert` and `roles` translate them.

pub mod audit;
pub mod controls;
pub mod memberships;
pub mod sessions;
pub mod users;
pub mod workspaces;
