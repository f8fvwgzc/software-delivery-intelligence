//! Token budget (CONTRACT.md §19): the context an LLM sees is trimmed
//! deterministically to a fixed number of estimated tokens (4 characters
//! each), by priority, so a large project never turns one reply into a
//! long, expensive prompt. Replies are capped per use.

use serde_json::{Map, Value};

/// Estimated tokens the structured context may take.
pub const CONTEXT_TOKENS: usize = 6_000;
/// Reply caps per use.
pub const SUMMARY_MAX_TOKENS: u32 = 600;
pub const NEGOTIATION_MAX_TOKENS: u32 = 400;
pub const DIAGRAM_MAX_TOKENS: u32 = 250;
/// A drafted fix (CONTRACT.md §23.2): the replacement for a finding's lines.
pub const FIX_MAX_TOKENS: u32 = 800;

/// Top-level keys in the order they are worth keeping: findings and ranked
/// options first, the facts of the project next, graph and summary
/// material last. Keys not listed come after these, alphabetically.
const PRIORITY: &[&str] = &[
    "findings",
    "options",
    "after",
    "before",
    "effects",
    "recorded_decision",
    "user_message",
    "verify",
    "weights_before",
    "weights_after",
    "pull_request",
    "project",
    "facts",
    "touched_facts",
    "areas",
    "impacted_components",
    "diagram",
    "graph",
    "summary",
];

/// Characters ÷ 4, rounded up. Deterministic and provider-independent.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn rank(key: &str) -> (usize, &str) {
    (
        PRIORITY
            .iter()
            .position(|k| *k == key)
            .unwrap_or(PRIORITY.len()),
        key,
    )
}

fn size_of(value: &Value) -> usize {
    serde_json::to_string(value).map(|s| s.len()).unwrap_or(0)
}

/// Trims `context` to at most `max_tokens` estimated tokens. Objects keep
/// their keys in priority order until the budget is spent; the key that
/// overflows is shortened (arrays lose their tail, strings are cut, nested
/// objects recurse) and the keys that follow are dropped. A `_truncated`
/// list names what was cut so the model knows the context is partial.
/// Returns the context and whether anything was removed.
pub fn trim_context(context: &Value, max_tokens: usize) -> (Value, bool) {
    let budget = max_tokens.saturating_mul(4);
    if size_of(context) <= budget {
        return (context.clone(), false);
    }
    (fit(context, budget), true)
}

fn fit(value: &Value, budget: usize) -> Value {
    match value {
        Value::Object(map) => fit_object(map, budget),
        Value::Array(items) => fit_array(items, budget),
        Value::String(s) => fit_string(s, budget),
        other => other.clone(),
    }
}

fn fit_object(map: &Map<String, Value>, budget: usize) -> Value {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by_key(|k| rank(k));
    let mut out = Map::new();
    let mut truncated: Vec<String> = Vec::new();
    // Braces, plus room for the `_truncated` note.
    let mut used = 2 + 24;
    for key in keys {
        let value = &map[key];
        let overhead = key.len() + 4;
        let remaining = budget.saturating_sub(used + overhead);
        let whole = size_of(value);
        if whole <= remaining {
            out.insert(key.clone(), value.clone());
            used += overhead + whole;
            continue;
        }
        truncated.push(key.clone());
        if remaining < 16 {
            continue;
        }
        let cut = fit(value, remaining);
        let cut_size = size_of(&cut);
        if cut_size > 2 && cut_size <= remaining {
            out.insert(key.clone(), cut);
            used += overhead + cut_size;
        }
    }
    if !truncated.is_empty() {
        truncated.sort();
        out.insert(
            "_truncated".to_owned(),
            Value::Array(truncated.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(out)
}

fn fit_array(items: &[Value], budget: usize) -> Value {
    let mut out = Vec::new();
    let mut used = 2;
    for item in items {
        let size = size_of(item) + 1;
        if used + size > budget {
            break;
        }
        out.push(item.clone());
        used += size;
    }
    Value::Array(out)
}

fn fit_string(s: &str, budget: usize) -> Value {
    // Quotes and an ellipsis.
    let keep = budget.saturating_sub(4);
    let mut end = keep.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    Value::String(format!("{}…", &s[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn small_contexts_are_untouched() {
        let ctx = json!({"findings": [1, 2], "graph": {"nodes": 3}});
        let (out, trimmed) = trim_context(&ctx, 100);
        assert!(!trimmed);
        assert_eq!(out, ctx);
    }

    #[test]
    fn trims_by_priority_and_says_so() {
        let big: Vec<Value> = (0..400)
            .map(|i| json!({"node": i, "label": "x".repeat(20)}))
            .collect();
        let ctx = json!({
            "graph": big,
            "findings": [{"rule": "a.b", "line": 3}],
            "options": [{"id": "opt-1"}],
            "facts": (0..100).map(|i| json!({"key": format!("k{i}")})).collect::<Vec<_>>(),
        });
        let (out, trimmed) = trim_context(&ctx, 500);
        assert!(trimmed);
        assert!(estimate_tokens(&serde_json::to_string(&out).unwrap()) <= 500);
        assert_eq!(out["findings"], ctx["findings"]);
        assert_eq!(out["options"], ctx["options"]);
        let truncated: Vec<&str> = out["_truncated"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(truncated.contains(&"graph"), "{truncated:?}");
        // Facts rank above the graph: they are kept (whole or cut) before it.
        assert!(
            out.get("facts")
                .is_some_and(|f| !f.as_array().unwrap().is_empty())
        );
        // Deterministic.
        assert_eq!(trim_context(&ctx, 500).0, out);
    }

    #[test]
    fn strings_and_arrays_are_cut_not_dropped() {
        let ctx = json!({"summary": "é".repeat(5000)});
        let (out, trimmed) = trim_context(&ctx, 50);
        assert!(trimmed);
        let s = out["summary"].as_str().unwrap();
        assert!(s.ends_with('…'));
        assert!(estimate_tokens(&serde_json::to_string(&out).unwrap()) <= 50);
        let (out, _) = trim_context(&json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), 3);
        assert!(out.as_array().unwrap().len() < 10);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }
}
