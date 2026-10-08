//! Thin helpers over `serde_yaml_ng::Value` so detectors read like prose.

use serde::Deserialize;
use serde_yaml_ng::Value;

/// Every document of a (possibly multi-document) YAML file up to the first
/// broken one. The deserializer keeps yielding the same error after a parse
/// failure, so iteration must stop there rather than skip.
pub(crate) fn docs(content: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for doc in serde_yaml_ng::Deserializer::from_str(content).take(256) {
        match Value::deserialize(doc) {
            Ok(v) if v.is_mapping() => out.push(v),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    out
}

pub(crate) fn first(content: &str) -> Option<Value> {
    docs(content).into_iter().next()
}

pub(crate) fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Mapping(m) => m.get(Value::String(key.to_string())),
        _ => None,
    }
}

pub(crate) fn str_of(v: Option<&Value>) -> Option<&str> {
    match v? {
        Value::String(s) => Some(s.as_str()),
        _ => None,
    }
}

pub(crate) fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    str_of(get(v, key))
}

/// A scalar or a sequence of scalars as strings; a mapping yields its keys.
pub(crate) fn str_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Number(n)) => vec![n.to_string()],
        Some(Value::Bool(b)) => vec![b.to_string()],
        Some(Value::Sequence(seq)) => seq.iter().filter_map(scalar).collect(),
        Some(Value::Mapping(m)) => m.keys().filter_map(scalar).collect(),
        _ => Vec::new(),
    }
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// `(key, value)` pairs of a mapping with string keys.
pub(crate) fn entries(v: Option<&Value>) -> Vec<(String, &Value)> {
    match v {
        Some(Value::Mapping(m)) => m
            .iter()
            .filter_map(|(k, v)| scalar(k).map(|k| (k, v)))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_yaml_terminates() {
        assert!(docs("{{{ not: [yaml").is_empty());
        assert_eq!(docs("a: 1\n---\nb: 2\n").len(), 2);
    }
}
