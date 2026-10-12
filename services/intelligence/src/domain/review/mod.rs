//! Code review (CONTRACT.md §20), the pure part: the input model built
//! from the knowledge service's answers, the deterministic analyzers
//! (one file per category under `rules/`) and the templated walkthrough.
//! Nothing here touches a database, GitHub or a model.

pub mod input;
pub mod rules;
pub mod summary;

pub use input::ReviewInput;

/// The five steps of the pipeline, in order, with their labels.
pub const STEPS: [(&str, &str); 5] = [
    ("context", "Context"),
    ("analyze", "Analyze"),
    ("findings", "Findings"),
    ("summary", "Summary"),
    ("publish", "Publish"),
];
