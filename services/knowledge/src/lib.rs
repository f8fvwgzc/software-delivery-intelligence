//! Knowledge service: the Project Knowledge Model behind `sdi.knowledge.v1`.
//!
//! A library crate so the integration tests can reach the repositories and
//! the gRPC service; `main.rs` only wires the binary together.

pub mod auth;
pub mod catalog;
pub mod config;
pub mod controls;
pub mod convert;
pub mod db;
pub mod domain;
pub mod events;
pub mod github;
pub mod grpc;
pub mod identity;
pub mod scanner;
pub mod sweeper;
