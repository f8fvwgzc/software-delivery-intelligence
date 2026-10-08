//! Conversions between `chrono` and protobuf timestamps.

use chrono::{DateTime, TimeZone, Utc};
use prost_types::Timestamp;

pub fn to_proto(at: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: at.timestamp(),
        nanos: at.timestamp_subsec_nanos() as i32,
    }
}

pub fn opt_to_proto(at: Option<DateTime<Utc>>) -> Option<Timestamp> {
    at.map(to_proto)
}

pub fn from_proto(at: &Timestamp) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(at.seconds, at.nanos.max(0) as u32)
        .single()
}

pub fn opt_from_proto(at: Option<&Timestamp>) -> Option<DateTime<Utc>> {
    at.and_then(from_proto)
}
