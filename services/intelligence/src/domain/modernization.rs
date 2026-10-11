//! CONTRACT.md §6 Modernization: a modernization pattern is scored by the
//! maturity axes it raises. The catalog seeds those axes as
//! `Pattern.targets_axes`; a learned pattern carries none, so its text is
//! matched against the axis vocabulary as a fallback.

use sdi_proto::knowledge::v1::{MaturityScore, Pattern};

/// Maturity axes a pattern raises, best first.
pub fn targets_axes(pattern: &Pattern) -> Vec<String> {
    if !pattern.targets_axes.is_empty() {
        return pattern.targets_axes.clone();
    }
    let text = format!("{} {} {}", pattern.id, pattern.name, pattern.summary).to_lowercase();
    let mut axes = Vec::new();
    let keyword = |axis: &str, words: &[&str]| {
        words
            .iter()
            .any(|w| text.contains(w))
            .then(|| axis.to_owned())
    };
    axes.extend(keyword(
        "containerization",
        &["container", "docker", "healthcheck"],
    ));
    axes.extend(keyword(
        "ci",
        &["ci ", "pipeline", "workflow", "github actions"],
    ));
    axes.extend(keyword("tests", &["test"]));
    axes.extend(keyword(
        "iac",
        &["terraform", "bicep", "infrastructure", "managed database"],
    ));
    axes.extend(keyword(
        "dependencies",
        &["dependabot", "dependenc", "lockfile"],
    ));
    axes.extend(keyword("docs", &["readme", "document", "architecture"]));
    axes.extend(keyword(
        "secrets",
        &["secret", "environment variable", "env"],
    ));
    axes.extend(keyword(
        "runtime",
        &["runtime", "end-of-life", "eol", "upgrade"],
    ));
    axes
}

fn axis_score(maturity: Option<&MaturityScore>, axis: &str) -> Option<u32> {
    maturity?
        .axes
        .iter()
        .find(|a| a.key == axis)
        .map(|a| a.score.min(100))
}

/// `1 − score/100` of the weakest axis the pattern targets, so the lowest
/// axis is addressed first. A pattern that targets nothing, or a project
/// without a maturity report, scores a neutral 0.5. Returns the axis too so
/// the evidence can name it.
pub fn fit(pattern: &Pattern, maturity: Option<&MaturityScore>) -> (f64, Option<(String, u32)>) {
    let weakest = targets_axes(pattern)
        .into_iter()
        .filter_map(|axis| axis_score(maturity, &axis).map(|score| (axis, score)))
        .min_by_key(|(_, score)| *score);
    match weakest {
        Some((axis, score)) => (1.0 - f64::from(score) / 100.0, Some((axis, score))),
        None => (0.5, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdi_proto::knowledge::v1::MaturityAxis;

    fn maturity(scores: &[(&str, u32)]) -> MaturityScore {
        MaturityScore {
            project_id: "p".into(),
            overall: 40,
            stage: "transitional".into(),
            axes: scores
                .iter()
                .map(|(key, score)| MaturityAxis {
                    key: (*key).into(),
                    label: (*key).into(),
                    score: *score,
                    ..Default::default()
                })
                .collect(),
            computed_at: None,
        }
    }

    #[test]
    fn weakest_targeted_axis_drives_the_fit() {
        let m = maturity(&[
            ("containerization", 10),
            ("ci", 60),
            ("tests", 20),
            ("runtime", 30),
        ]);
        // Seeded axes (the catalog's add-ci-baseline) win over the text.
        let mut p = Pattern {
            id: "add-ci-baseline".into(),
            targets_axes: vec!["ci".into(), "tests".into()],
            ..Default::default()
        };
        let (fit_ci, axis) = fit(&p, Some(&m));
        assert!((fit_ci - 0.8).abs() < 1e-9);
        assert_eq!(axis, Some(("tests".to_owned(), 20)));
        p.targets_axes = vec!["containerization".into()];
        assert!((fit(&p, Some(&m)).0 - 0.9).abs() < 1e-9);
        // A learned pattern has no seeded axes: its text decides.
        p.id = "learned-thing".into();
        p.targets_axes.clear();
        p.summary = "Pin the runtime and upgrade Node".into();
        assert_eq!(targets_axes(&p), vec!["runtime"]);
        assert!((fit(&p, Some(&m)).0 - 0.7).abs() < 1e-9);
        p.summary = "nothing relevant".into();
        assert_eq!(fit(&p, Some(&m)), (0.5, None));
        assert_eq!(fit(&p, None), (0.5, None));
    }
}
