//! What the analyzers see (CONTRACT.md §20): the pull request's metadata,
//! its changed files with parsed hunks, and the slice of the project's
//! knowledge that matters to a change (test framework, maturity axes,
//! touched facts, impacted graph nodes). Built once from the knowledge
//! service's answers; every rule is a pure function of it.

use std::collections::BTreeSet;

use sdi_proto::knowledge::v1::{
    ChangedFile as ProtoChangedFile, FactCategory, ProjectContext, PullRequest as ProtoPullRequest,
};
use serde::{Deserialize, Serialize};

/// One `@@` section with its lines (leading ` `, `+` or `-` kept).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub previous_path: String,
    /// "added" | "modified" | "removed" | "renamed"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub language: String,
    pub hunks: Vec<Hunk>,
    /// The patch was over the limit; `hunks` are empty.
    pub truncated: bool,
}

impl ChangedFile {
    /// Added lines with their line number in the new file.
    pub fn added_lines(&self) -> Vec<(u32, &str)> {
        let mut out = Vec::new();
        for hunk in &self.hunks {
            let mut line_no = hunk.new_start;
            for line in &hunk.lines {
                match line.as_bytes().first() {
                    Some(b'+') => {
                        out.push((line_no, &line[1..]));
                        line_no += 1;
                    }
                    Some(b'-') => {}
                    _ => line_no += 1,
                }
            }
        }
        out
    }

    /// Removed lines with their line number in the old file.
    pub fn removed_lines(&self) -> Vec<(u32, &str)> {
        let mut out = Vec::new();
        for hunk in &self.hunks {
            let mut line_no = hunk.old_start;
            for line in &hunk.lines {
                match line.as_bytes().first() {
                    Some(b'-') => {
                        out.push((line_no, &line[1..]));
                        line_no += 1;
                    }
                    Some(b'+') => {}
                    _ => line_no += 1,
                }
            }
        }
        out
    }

    /// Every added line joined, for whole-file checks (a workflow without a
    /// `permissions` block, a Dockerfile without a `HEALTHCHECK`).
    pub fn added_text(&self) -> String {
        self.added_lines()
            .iter()
            .map(|(_, l)| *l)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The first added line's number, or 1: where a file-level finding points.
    pub fn first_line(&self) -> u32 {
        self.added_lines().first().map_or(1, |(n, _)| *n)
    }

    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    pub fn extension(&self) -> &str {
        self.file_name().rsplit_once('.').map_or("", |(_, e)| e)
    }

    pub fn is_added(&self) -> bool {
        self.status == "added"
    }

    pub fn is_removed(&self) -> bool {
        self.status == "removed"
    }

    /// Whether the path is a test (by directory or by name, in the common
    /// conventions of the languages the analyzer knows).
    pub fn is_test(&self) -> bool {
        is_test_path(&self.path)
    }

    /// Whether the file is source code (something the tests rule expects
    /// tests for), as opposed to config, docs, data or generated output.
    pub fn is_source(&self) -> bool {
        !self.is_test()
            && !self.is_docs()
            && !self.is_lockfile()
            && matches!(
                self.extension(),
                "rs" | "ts"
                    | "tsx"
                    | "js"
                    | "jsx"
                    | "mjs"
                    | "cjs"
                    | "py"
                    | "go"
                    | "java"
                    | "kt"
                    | "rb"
                    | "php"
                    | "cs"
                    | "swift"
                    | "scala"
                    | "ex"
                    | "exs"
                    | "c"
                    | "cc"
                    | "cpp"
                    | "h"
                    | "hpp"
            )
            && !self.path.contains("/generated/")
            && !self.file_name().ends_with(".d.ts")
            && !self.file_name().ends_with(".min.js")
    }

    pub fn is_docs(&self) -> bool {
        matches!(self.extension(), "md" | "mdx" | "rst" | "adoc" | "txt")
            || self.path.starts_with("docs/")
    }

    pub fn is_lockfile(&self) -> bool {
        matches!(
            self.file_name(),
            "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "bun.lockb"
                | "bun.lock"
                | "Cargo.lock"
                | "poetry.lock"
                | "uv.lock"
                | "Pipfile.lock"
                | "go.sum"
                | "Gemfile.lock"
                | "composer.lock"
                | "mix.lock"
                | "packages.lock.json"
        )
    }

    pub fn is_dockerfile(&self) -> bool {
        let name = self.file_name();
        name == "Dockerfile"
            || name.starts_with("Dockerfile.")
            || name.ends_with(".dockerfile")
            || name == "Containerfile"
    }

    pub fn is_workflow(&self) -> bool {
        self.path.starts_with(".github/workflows/") && matches!(self.extension(), "yml" | "yaml")
    }

    pub fn is_iac(&self) -> bool {
        matches!(self.extension(), "tf" | "bicep" | "tfvars")
            || self.path.contains("cloudformation")
            || self.path.starts_with("k8s/")
            || self.path.starts_with("kubernetes/")
            || self.path.starts_with("helm/")
            || self.path.starts_with("deploy/")
    }
}

pub fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    lower.starts_with("tests/")
        || lower.starts_with("test/")
        || lower.starts_with("spec/")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.contains("/spec/")
        || lower.contains("/__tests__/")
        || lower.contains("/__mocks__/")
        || lower.contains("/testdata/")
        || lower.contains("/fixtures/")
        || name.starts_with("test_")
        || name.ends_with("_test.go")
        || name.ends_with("_test.py")
        || name.ends_with("_test.rs")
        || name.ends_with("_spec.rb")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name == "conftest.py"
}

/// The pull request as the rules and the summary describe it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullRequestMeta {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub base_ref: String,
    pub head_ref: String,
    pub head_sha: String,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub html_url: String,
}

/// A fact whose evidence lives in a changed file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FactRef {
    pub id: String,
    pub key: String,
    pub label: String,
    pub value: String,
    pub source_path: String,
}

/// A graph node the change reaches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeRef {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub source_path: String,
}

/// One delivery-maturity axis as the last scan scored it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AxisScore {
    pub key: String,
    pub label: String,
    pub score: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewInput {
    pub project_id: String,
    pub project_name: String,
    pub repo_full_name: String,
    pub primary_language: String,
    pub technologies: Vec<String>,
    pub capabilities: Vec<String>,
    /// The scan found a test framework or test files (`tests:*` capability
    /// or a testing fact), so source changes are expected to bring tests.
    pub has_test_framework: bool,
    pub maturity_axes: Vec<AxisScore>,
    pub pr: PullRequestMeta,
    pub files: Vec<ChangedFile>,
    /// The file list was cut at the limit.
    pub files_truncated: bool,
    pub touched_facts: Vec<FactRef>,
    pub impacted_nodes: Vec<NodeRef>,
}

impl ReviewInput {
    /// Joins the knowledge service's two answers. Facts and nodes are
    /// resolved by id against the context so the rules see labels, not ids.
    pub fn from_proto(pr: &ProtoPullRequest, context: &ProjectContext) -> Self {
        let summary = pr.summary.clone().unwrap_or_default();
        let project = context.project.clone().unwrap_or_default();
        let touched: BTreeSet<&str> = pr.touched_fact_ids.iter().map(String::as_str).collect();
        let impacted: Vec<&str> = pr.impacted_node_ids.iter().map(String::as_str).collect();
        let has_test_framework = context.capabilities.iter().any(|c| c.starts_with("tests:"))
            || context
                .facts
                .iter()
                .any(|f| f.category() == FactCategory::Testing);
        let maturity = context.maturity.as_ref().or(context
            .understanding
            .as_ref()
            .and_then(|u| u.maturity.as_ref()));
        let touched_facts = context
            .facts
            .iter()
            .filter(|f| touched.contains(f.id.as_str()))
            .map(|f| FactRef {
                id: f.id.clone(),
                key: f.key.clone(),
                label: f.label.clone(),
                value: f.value.clone(),
                source_path: f.source_path.clone(),
            })
            .collect();
        let nodes = context
            .graph
            .as_ref()
            .map(|g| g.nodes.as_slice())
            .unwrap_or_default();
        let impacted_nodes = impacted
            .iter()
            .filter_map(|id| nodes.iter().find(|n| n.id == *id))
            .map(|n| NodeRef {
                id: n.id.clone(),
                kind: n
                    .kind()
                    .as_str_name()
                    .trim_start_matches("NODE_KIND_")
                    .to_lowercase(),
                label: n.label.clone(),
                source_path: n.source_path.clone(),
            })
            .collect();
        let files_truncated =
            (pr.files.len() as u32) < summary.changed_files && !pr.files.is_empty();
        Self {
            project_id: project.id,
            project_name: project.name,
            repo_full_name: project.repo_full_name,
            primary_language: project.primary_language,
            technologies: context.technologies.clone(),
            capabilities: context.capabilities.clone(),
            has_test_framework,
            maturity_axes: maturity
                .map(|m| {
                    m.axes
                        .iter()
                        .map(|a| AxisScore {
                            key: a.key.clone(),
                            label: a.label.clone(),
                            score: a.score,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            pr: PullRequestMeta {
                number: summary.number,
                title: summary.title,
                body: pr.body.clone(),
                author: summary.author,
                base_ref: summary.base_ref,
                head_ref: summary.head_ref,
                head_sha: summary.head_sha,
                changed_files: summary.changed_files,
                additions: summary.additions,
                deletions: summary.deletions,
                html_url: summary.html_url,
            },
            files: pr.files.iter().map(changed_file).collect(),
            files_truncated,
            touched_facts,
            impacted_nodes,
        }
    }

    #[cfg(test)]
    pub fn file(&self, path: &str) -> Option<&ChangedFile> {
        self.files.iter().find(|f| f.path == path)
    }

    pub fn has_file(&self, predicate: impl Fn(&ChangedFile) -> bool) -> bool {
        self.files.iter().any(predicate)
    }

    pub fn axis(&self, key: &str) -> Option<&AxisScore> {
        self.maturity_axes.iter().find(|a| a.key == key)
    }
}

fn changed_file(file: &ProtoChangedFile) -> ChangedFile {
    ChangedFile {
        path: file.path.clone(),
        previous_path: file.previous_path.clone(),
        status: file.status.clone(),
        additions: file.additions,
        deletions: file.deletions,
        language: file.language.clone(),
        hunks: file
            .hunks
            .iter()
            .map(|h| Hunk {
                old_start: h.old_start,
                old_lines: h.old_lines,
                new_start: h.new_start,
                new_lines: h.new_lines,
                lines: h.lines.clone(),
            })
            .collect(),
        truncated: file.truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdi_proto::knowledge::v1::{
        DiffHunk, Fact, Graph, GraphNode, MaturityAxis, MaturityScore, NodeKind, Project,
        PullRequestSummary,
    };

    #[test]
    fn test_paths_are_recognised() {
        for path in [
            "tests/integration.rs",
            "src/lib/__tests__/a.ts",
            "web/src/App.test.tsx",
            "pkg/x_test.go",
            "spec/models/user_spec.rb",
            "services/api/src/tests/mod.rs",
            "test_main.py",
            "conftest.py",
        ] {
            assert!(is_test_path(path), "{path}");
        }
        for path in ["src/main.rs", "web/src/App.tsx", "README.md", "testing.rs"] {
            assert!(!is_test_path(path), "{path}");
        }
    }

    #[test]
    fn file_classification() {
        let f = |path: &str| ChangedFile {
            path: path.into(),
            ..Default::default()
        };
        assert!(f("Dockerfile").is_dockerfile());
        assert!(f("deploy/Dockerfile.prod").is_dockerfile());
        assert!(f(".github/workflows/ci.yml").is_workflow());
        assert!(!f(".github/dependabot.yml").is_workflow());
        assert!(f("infra/main.tf").is_iac());
        assert!(f("pnpm-lock.yaml").is_lockfile());
        assert!(f("src/main.rs").is_source());
        assert!(!f("src/types.d.ts").is_source());
        assert!(!f("src/main.test.ts").is_source());
        assert!(f("docs/guide.md").is_docs());
        assert_eq!(f("a/b.tar.gz").extension(), "gz");
    }

    #[test]
    fn builds_from_the_knowledge_answers() {
        let pr = ProtoPullRequest {
            summary: Some(PullRequestSummary {
                number: 4,
                title: "Add CI".into(),
                author: "ana".into(),
                head_sha: "abc".into(),
                changed_files: 3,
                ..Default::default()
            }),
            body: "Because.".into(),
            files: vec![ProtoChangedFile {
                path: ".github/workflows/ci.yml".into(),
                status: "added".into(),
                additions: 2,
                hunks: vec![DiffHunk {
                    new_start: 1,
                    new_lines: 2,
                    lines: vec!["+name: ci".into(), "+on: push".into()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            touched_fact_ids: vec!["f1".into(), "nope".into()],
            impacted_node_ids: vec!["n1".into()],
        };
        let context = ProjectContext {
            project: Some(Project {
                id: "p1".into(),
                name: "repo".into(),
                repo_full_name: "o/repo".into(),
                ..Default::default()
            }),
            facts: vec![Fact {
                id: "f1".into(),
                key: "ci:github_actions".into(),
                label: "GitHub Actions".into(),
                source_path: ".github/workflows/ci.yml".into(),
                ..Default::default()
            }],
            graph: Some(Graph {
                nodes: vec![GraphNode {
                    id: "n1".into(),
                    kind: NodeKind::Pipeline as i32,
                    label: "CI".into(),
                    ..Default::default()
                }],
                edges: vec![],
            }),
            capabilities: vec!["tests:unit".into()],
            maturity: Some(MaturityScore {
                axes: vec![MaturityAxis {
                    key: "ci".into(),
                    label: "CI".into(),
                    score: 10,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let input = ReviewInput::from_proto(&pr, &context);
        assert_eq!(input.pr.number, 4);
        assert_eq!(input.repo_full_name, "o/repo");
        assert!(input.has_test_framework);
        assert_eq!(input.touched_facts.len(), 1);
        assert_eq!(input.impacted_nodes[0].kind, "pipeline");
        assert_eq!(input.axis("ci").unwrap().score, 10);
        assert!(input.files_truncated);
        let file = input.file(".github/workflows/ci.yml").unwrap();
        assert_eq!(file.added_lines(), vec![(1, "name: ci"), (2, "on: push")]);
        assert_eq!(file.first_line(), 1);
        assert_eq!(file.added_text(), "name: ci\non: push");
    }

    #[test]
    fn line_numbers_follow_the_hunks() {
        let file = ChangedFile {
            hunks: vec![Hunk {
                old_start: 5,
                old_lines: 3,
                new_start: 7,
                new_lines: 3,
                lines: vec![" a".into(), "-b".into(), "+c".into(), " d".into()],
            }],
            ..Default::default()
        };
        assert_eq!(file.added_lines(), vec![(8, "c")]);
        assert_eq!(file.removed_lines(), vec![(6, "b")]);
    }
}
