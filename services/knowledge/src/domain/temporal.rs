//! Temporal edge diff. A rescan never rewrites history: edges that vanished
//! are closed (`valid_to = now`), new ones opened, unchanged ones untouched.

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeKey {
    pub source_key: String,
    pub target_key: String,
    pub relation: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub closed: Vec<EdgeKey>,
    pub opened: Vec<EdgeKey>,
}

pub fn diff(current: &[EdgeKey], wanted: &[EdgeKey]) -> Diff {
    let current_set: HashSet<&EdgeKey> = current.iter().collect();
    let wanted_set: HashSet<&EdgeKey> = wanted.iter().collect();

    let mut closed: Vec<EdgeKey> = current
        .iter()
        .filter(|e| !wanted_set.contains(e))
        .cloned()
        .collect();
    let mut opened: Vec<EdgeKey> = wanted
        .iter()
        .filter(|e| !current_set.contains(e))
        .cloned()
        .collect();
    closed.sort();
    closed.dedup();
    opened.sort();
    opened.dedup();
    Diff { closed, opened }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str, t: &str, r: &str) -> EdgeKey {
        EdgeKey {
            source_key: s.into(),
            target_key: t.into(),
            relation: r.into(),
        }
    }

    #[test]
    fn closes_missing_opens_new_keeps_same() {
        let current = vec![key("a", "b", "DEPENDS_ON"), key("a", "c", "DEPENDS_ON")];
        let wanted = vec![key("a", "b", "DEPENDS_ON"), key("a", "d", "DEPENDS_ON")];
        let diff = diff(&current, &wanted);
        assert_eq!(diff.closed, vec![key("a", "c", "DEPENDS_ON")]);
        assert_eq!(diff.opened, vec![key("a", "d", "DEPENDS_ON")]);
    }

    #[test]
    fn relation_change_is_close_plus_open() {
        let diff = diff(&[key("a", "b", "USES")], &[key("a", "b", "DEPENDS_ON")]);
        assert_eq!(diff.closed.len(), 1);
        assert_eq!(diff.opened.len(), 1);
    }

    #[test]
    fn duplicates_collapse() {
        let diff = diff(&[], &[key("a", "b", "X"), key("a", "b", "X")]);
        assert_eq!(diff.opened.len(), 1);
    }
}
