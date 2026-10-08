//! The plan is a DAG over step keys. Validation runs once at create time so
//! the runner can trust `depends_on` blindly afterwards.

use std::collections::{BTreeSet, HashMap, HashSet};

use thiserror::Error;

/// The slice of a step the graph functions need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node<'a> {
    pub key: &'a str,
    pub depends_on: &'a [String],
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DagError {
    #[error("plan has no steps")]
    Empty,
    #[error("step key must not be empty")]
    EmptyKey,
    #[error("duplicate step key: {0}")]
    DuplicateKey(String),
    #[error("step {step} depends on unknown step {dependency}")]
    UnknownDependency { step: String, dependency: String },
    #[error("step {0} depends on itself")]
    SelfDependency(String),
    #[error("dependency cycle through step {0}")]
    Cycle(String),
}

/// Rejects empty plans, duplicate or unknown keys and cycles. The error
/// message is meant to be returned verbatim as `INVALID_ARGUMENT`.
pub fn validate(nodes: &[Node<'_>]) -> Result<(), DagError> {
    if nodes.is_empty() {
        return Err(DagError::Empty);
    }
    let mut seen = HashSet::new();
    for node in nodes {
        if node.key.trim().is_empty() {
            return Err(DagError::EmptyKey);
        }
        if !seen.insert(node.key) {
            return Err(DagError::DuplicateKey(node.key.to_owned()));
        }
    }
    for node in nodes {
        for dependency in node.depends_on {
            if dependency == node.key {
                return Err(DagError::SelfDependency(node.key.to_owned()));
            }
            if !seen.contains(dependency.as_str()) {
                return Err(DagError::UnknownDependency {
                    step: node.key.to_owned(),
                    dependency: dependency.clone(),
                });
            }
        }
    }

    // Iterative DFS with three colours; the first back edge names the cycle.
    let index: HashMap<&str, &Node<'_>> = nodes.iter().map(|n| (n.key, n)).collect();
    let mut state: HashMap<&str, u8> = HashMap::new();
    for start in nodes {
        if state.contains_key(start.key) {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(start.key, 0)];
        state.insert(start.key, 1);
        while let Some((key, next)) = stack.last_mut() {
            let deps = index[*key].depends_on;
            if *next < deps.len() {
                let dependency = deps[*next].as_str();
                *next += 1;
                match state.get(dependency) {
                    Some(1) => return Err(DagError::Cycle(dependency.to_owned())),
                    Some(_) => {}
                    None => {
                        state.insert(dependency, 1);
                        stack.push((dependency, 0));
                    }
                }
            } else {
                state.insert(*key, 2);
                stack.pop();
            }
        }
    }
    Ok(())
}

/// Keys whose dependencies are all satisfied and that are still pending,
/// in the order given (plan position).
pub fn ready<'a>(
    nodes: &[Node<'a>],
    satisfied: &HashSet<&str>,
    pending: &HashSet<&str>,
) -> Vec<&'a str> {
    nodes
        .iter()
        .filter(|node| pending.contains(node.key))
        .filter(|node| {
            node.depends_on
                .iter()
                .all(|d| satisfied.contains(d.as_str()))
        })
        .map(|node| node.key)
        .collect()
}

/// Every step that transitively depends on `key`, in plan order. These are
/// the steps that become BLOCKED when `key` fails.
pub fn dependents<'a>(nodes: &[Node<'a>], key: &str) -> Vec<&'a str> {
    let mut blocked: BTreeSet<&str> = BTreeSet::new();
    blocked.insert(key);
    // Nodes are in position order and dependencies point backwards in a valid
    // plan, but a single pass is not guaranteed for arbitrary orderings.
    loop {
        let before = blocked.len();
        for node in nodes {
            if node.depends_on.iter().any(|d| blocked.contains(d.as_str())) {
                blocked.insert(node.key);
            }
        }
        if blocked.len() == before {
            break;
        }
    }
    nodes
        .iter()
        .filter(|n| n.key != key && blocked.contains(n.key))
        .map(|n| n.key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(spec: &[(&'static str, &[&'static str])]) -> Vec<(String, Vec<String>)> {
        spec.iter()
            .map(|(k, d)| (k.to_string(), d.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    fn nodes(owned: &[(String, Vec<String>)]) -> Vec<Node<'_>> {
        owned
            .iter()
            .map(|(k, d)| Node {
                key: k,
                depends_on: d,
            })
            .collect()
    }

    #[test]
    fn accepts_a_linear_plan() {
        let owned = plan(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);
        assert_eq!(validate(&nodes(&owned)), Ok(()));
    }

    #[test]
    fn rejects_empty_plan_and_keys() {
        assert_eq!(validate(&[]), Err(DagError::Empty));
        let owned = plan(&[("", &[])]);
        assert_eq!(validate(&nodes(&owned)), Err(DagError::EmptyKey));
    }

    #[test]
    fn rejects_duplicates_unknowns_and_self_loops() {
        let owned = plan(&[("a", &[]), ("a", &[])]);
        assert_eq!(
            validate(&nodes(&owned)),
            Err(DagError::DuplicateKey("a".into()))
        );

        let owned = plan(&[("a", &["ghost"])]);
        assert_eq!(
            validate(&nodes(&owned)),
            Err(DagError::UnknownDependency {
                step: "a".into(),
                dependency: "ghost".into()
            })
        );

        let owned = plan(&[("a", &["a"])]);
        assert_eq!(
            validate(&nodes(&owned)),
            Err(DagError::SelfDependency("a".into()))
        );
    }

    #[test]
    fn rejects_cycles() {
        let owned = plan(&[("a", &["c"]), ("b", &["a"]), ("c", &["b"])]);
        assert!(matches!(validate(&nodes(&owned)), Err(DagError::Cycle(_))));
    }

    #[test]
    fn ready_set_follows_checkpoints_in_plan_order() {
        let owned = plan(&[("a", &[]), ("b", &["a"]), ("c", &["a"]), ("d", &["b", "c"])]);
        let n = nodes(&owned);
        let all: HashSet<&str> = ["a", "b", "c", "d"].into();
        let after = |done: &[&'static str]| -> (HashSet<&'static str>, HashSet<&'static str>) {
            let satisfied: HashSet<&'static str> = done.iter().copied().collect();
            let pending = all.difference(&satisfied).copied().collect();
            (satisfied, pending)
        };
        let (s, p) = after(&["a"]);
        assert_eq!(ready(&n, &s, &p), vec!["b", "c"]);
        let (s, p) = after(&["a", "b"]);
        assert_eq!(ready(&n, &s, &p), vec!["c"]);
        let (s, p) = after(&["a", "b", "c"]);
        assert_eq!(ready(&n, &s, &p), vec!["d"]);
        let (s, p) = after(&[]);
        assert_eq!(ready(&n, &s, &p), vec!["a"]);
        let (s, p) = after(&["a", "b", "c", "d"]);
        assert!(ready(&n, &s, &p).is_empty());
    }

    #[test]
    fn dependents_are_transitive_and_ordered() {
        let owned = plan(&[
            ("a", &[]),
            ("b", &["a"]),
            ("c", &["b"]),
            ("d", &["a"]),
            ("e", &["c", "d"]),
        ]);
        let n = nodes(&owned);
        assert_eq!(dependents(&n, "b"), vec!["c", "e"]);
        assert_eq!(dependents(&n, "a"), vec!["b", "c", "d", "e"]);
        assert!(dependents(&n, "e").is_empty());
    }
}
