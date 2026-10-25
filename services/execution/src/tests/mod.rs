//! In-process tests against a throwaway schema in the dev Postgres. They
//! skip (and say so) when DATABASE_URL is unset.

mod autopilot;
mod delivery;
mod e2e;
pub(crate) mod fakes;
mod gitops;
