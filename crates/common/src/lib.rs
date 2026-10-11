//! Plumbing shared by every SDI service. Domain logic never lives here.

pub mod config;
pub mod db;
pub mod embed;
pub mod grpc;
pub mod principal;
pub mod shutdown;
pub mod telemetry;
pub mod time;
