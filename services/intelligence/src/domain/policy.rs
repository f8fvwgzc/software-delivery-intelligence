//! The default policy table from CONTRACT.md §5.2. The execution service owns
//! the live table; this copy lets proposals and documents spell out which
//! steps will stop for a human without a network round trip.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Auto,
    Approval,
    Required,
    Blocked,
}

impl Gate {
    pub fn label(self) -> &'static str {
        match self {
            Gate::Auto => "auto",
            Gate::Approval => "approval",
            Gate::Required => "required",
            Gate::Blocked => "blocked",
        }
    }

    pub fn needs_human(self) -> bool {
        matches!(self, Gate::Approval | Gate::Required)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PolicyRow {
    pub action_type: &'static str,
    pub label: &'static str,
    pub risk: &'static str,
    pub gate: Gate,
}

const TABLE: &[PolicyRow] = &[
    row("repo.read", "Read repository", "safe", Gate::Auto),
    row("repo.analyze", "Analyze repository", "safe", Gate::Auto),
    row("tests.run_local", "Run tests locally", "safe", Gate::Auto),
    row(
        "docs.generate",
        "Generate documentation",
        "safe",
        Gate::Auto,
    ),
    row("deps.install", "Install dependencies", "medium", Gate::Auto),
    row(
        "container.build",
        "Build container image",
        "medium",
        Gate::Auto,
    ),
    row("container.run", "Run container", "medium", Gate::Approval),
    row(
        "env.create_temporary",
        "Create temporary environment",
        "medium",
        Gate::Auto,
    ),
    row(
        "db.create_temporary",
        "Create temporary database",
        "medium",
        Gate::Approval,
    ),
    row(
        "cloud.create_dev_resource",
        "Create development cloud resource",
        "medium",
        Gate::Approval,
    ),
    row(
        "cloud.modify_resource",
        "Modify existing cloud resource",
        "high",
        Gate::Approval,
    ),
    row(
        "deploy.staging",
        "Deploy to staging",
        "high",
        Gate::Approval,
    ),
    row(
        "deploy.production",
        "Deploy to production",
        "high",
        Gate::Required,
    ),
    row(
        "db.migrate_production",
        "Run production database migration",
        "high",
        Gate::Required,
    ),
    row(
        "backup.restore_production",
        "Restore production backup",
        "high",
        Gate::Required,
    ),
    row("resource.delete", "Delete resource", "high", Gate::Required),
    row("firewall.modify", "Modify firewall", "high", Gate::Required),
    row("secrets.rotate", "Rotate secrets", "high", Gate::Required),
    row(
        "db.destroy_production",
        "Destroy production database",
        "blocked",
        Gate::Blocked,
    ),
];

const fn row(
    action_type: &'static str,
    label: &'static str,
    risk: &'static str,
    gate: Gate,
) -> PolicyRow {
    PolicyRow {
        action_type,
        label,
        risk,
        gate,
    }
}

/// Unknown action types are treated as approval-gated: safer than guessing "auto".
pub fn lookup(action_type: &str) -> PolicyRow {
    TABLE
        .iter()
        .copied()
        .find(|row| row.action_type == action_type)
        .unwrap_or(PolicyRow {
            action_type: "unknown",
            label: "Unknown action",
            risk: "high",
            gate: Gate::Approval,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_deploy_always_needs_a_human() {
        assert_eq!(lookup("deploy.production").gate, Gate::Required);
        assert_eq!(lookup("db.destroy_production").gate, Gate::Blocked);
        assert_eq!(lookup("repo.read").gate, Gate::Auto);
        assert!(lookup("made.up").gate.needs_human());
    }
}
