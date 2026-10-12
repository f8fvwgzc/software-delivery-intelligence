//! `.sdi.yml` `delivery.environments` → the execution service (CONTRACT.md
//! §22 "Config as code"). Once a scan has stored what the repository
//! declares, execution is told which environments the file manages
//! (`source = "config"`, read-only in the UI), in promotion order.
//!
//! The file is the source of truth only while it can be trusted: a config
//! with errors changes nothing, so a typo never deletes an environment and
//! its history. A file that used to declare environments and no longer does
//! clears them. Nothing here can fail a scan: the outcome is reported in
//! the delivery stage and logged.

use std::collections::HashMap;

use sdi_analyzer::{DeliveryConfigDraft, DeliveryRegistryDraft, ProjectConfigDraft};
use sdi_common::principal::Principal;
use sdi_proto::execution::v1::{Environment, Registry};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{db, execution::DeliveryGateway};

/// `Environment.source` of everything this module writes.
pub const SOURCE_CONFIG: &str = "config";

/// What the delivery stage reports about the configured environments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to tell execution: no environments declared now or before,
    /// a config with errors, or no execution service configured.
    Skipped,
    /// Execution now holds exactly these config-managed environments.
    /// `notes` name what could not be honoured (an unknown registry).
    Synced {
        environments: usize,
        notes: Vec<String>,
    },
    /// Execution could not be told; the scan goes on.
    Failed(String),
}

impl Outcome {
    /// What the delivery stage appends to its detail; `None` when nothing
    /// was attempted.
    pub fn detail(&self) -> Option<String> {
        match self {
            Outcome::Skipped => None,
            Outcome::Synced {
                environments,
                notes,
            } => {
                let mut detail = format!(
                    "{environments} configured environment{} synced",
                    if *environments == 1 { "" } else { "s" }
                );
                if !notes.is_empty() {
                    detail.push_str(&format!(" ({})", notes.join("; ")));
                }
                Some(detail)
            }
            Outcome::Failed(reason) => {
                Some(format!("configured environments not synced: {reason}"))
            }
        }
    }
}

/// A usable config that declares at least one delivery environment.
pub fn declares_environments(config: &ProjectConfigDraft) -> bool {
    sdi_analyzer::config_is_usable(config)
        && DeliveryConfigDraft::from_json(&config.delivery_json)
            .is_some_and(|d| !d.environments.is_empty())
}

/// Whether the config stored by the previous scan declared environments.
/// Read before the scan replaces it; an unreadable row counts as "no".
pub async fn declared_before(pool: &PgPool, workspace_id: Uuid, project_id: Uuid) -> bool {
    match db::config::get(pool, workspace_id, project_id).await {
        Ok(row) => row.is_some_and(|r| declares_environments(&r.config.0)),
        Err(error) => {
            tracing::warn!(%error, "could not read the previous project config");
            false
        }
    }
}

/// The delivery section execution should mirror, or `None` to leave its
/// environments alone.
fn plan(config: &ProjectConfigDraft, previously_declared: bool) -> Option<DeliveryConfigDraft> {
    if config.present && !sdi_analyzer::config_is_usable(config) {
        // A file that is ignored entirely (§12) must not delete anything.
        return None;
    }
    let delivery = DeliveryConfigDraft::from_json(&config.delivery_json).unwrap_or_default();
    if delivery.environments.is_empty() && !previously_declared {
        return None;
    }
    Some(delivery)
}

/// The workspace registry an environment should pull from: the one it
/// names (by name, id or kind), else the one `delivery.registry` describes.
/// `Ok("")` means the workspace default; `Err` names what was not found.
fn resolve_registry(
    named: &str,
    declared: &DeliveryRegistryDraft,
    registries: &[Registry],
) -> Result<String, String> {
    // Among several candidates the workspace default wins.
    let pick = |candidates: Vec<&Registry>| {
        candidates
            .iter()
            .find(|r| r.is_default)
            .or(candidates.first())
            .map(|r| r.id.clone())
    };
    let named = named.trim();
    if !named.is_empty() {
        let exact: Vec<&Registry> = registries
            .iter()
            .filter(|r| r.id == named || r.name.eq_ignore_ascii_case(named))
            .collect();
        let by_kind: Vec<&Registry> = registries
            .iter()
            .filter(|r| r.kind.eq_ignore_ascii_case(named))
            .collect();
        return pick(exact)
            .or_else(|| pick(by_kind))
            .ok_or_else(|| format!("registry `{named}` is not configured in this workspace"));
    }
    if declared.kind.is_empty() {
        return Ok(String::new());
    }
    let matching: Vec<&Registry> = registries
        .iter()
        .filter(|r| r.kind == declared.kind)
        .filter(|r| declared.host.is_empty() || r.host.eq_ignore_ascii_case(&declared.host))
        .filter(|r| declared.namespace.is_empty() || r.namespace == declared.namespace)
        .collect();
    pick(matching).ok_or_else(|| {
        let address = if declared.namespace.is_empty() {
            declared.host.clone()
        } else {
            format!("{}/{}", declared.host, declared.namespace)
        };
        format!(
            "{} registry {address} is not configured in this workspace",
            declared.kind
        )
    })
}

/// The `Environment` messages of a delivery section, in promotion order.
/// `registries` is `None` when they could not be listed: every environment
/// then falls back to the workspace default without a note of its own.
pub fn environments_of(
    project_id: Uuid,
    delivery: &DeliveryConfigDraft,
    registries: Option<&[Registry]>,
    notes: &mut Vec<String>,
) -> Vec<Environment> {
    delivery
        .environments
        .iter()
        .enumerate()
        .map(|(position, env)| {
            let registry_id = match registries {
                Some(registries) => {
                    match resolve_registry(&env.registry, &delivery.registry, registries) {
                        Ok(id) => id,
                        Err(problem) => {
                            let note = format!("{problem}; the default registry is used");
                            if !notes.contains(&note) {
                                notes.push(note);
                            }
                            String::new()
                        }
                    }
                }
                None => String::new(),
            };
            Environment {
                id: String::new(),
                project_id: project_id.to_string(),
                name: env.name.clone(),
                position: position as u32,
                kind: env.kind.clone(),
                config: env
                    .config
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<_, _>>(),
                url: env.url.clone(),
                health_path: env.health_path.clone(),
                registry_id,
                protected: env.protected,
                auto_promote: env.auto_promote,
                source: SOURCE_CONFIG.to_owned(),
                current_release_id: String::new(),
                created_at: None,
                updated_at: None,
            }
        })
        .collect()
}

/// Tells execution which environments the scanned `.sdi.yml` manages. Runs
/// as the system principal of the project's workspace: a scan may have been
/// started by a member, and syncing what the repository declares is the
/// platform's doing, not theirs.
pub async fn sync(
    gateway: &dyn DeliveryGateway,
    workspace_id: Uuid,
    project_id: Uuid,
    config: &ProjectConfigDraft,
    previously_declared: bool,
) -> Outcome {
    let Some(delivery) = plan(config, previously_declared) else {
        return Outcome::Skipped;
    };
    let principal = Principal::system(workspace_id.to_string());
    let mut notes = Vec::new();
    let names_registry = !delivery.registry.kind.is_empty()
        || delivery.environments.iter().any(|e| !e.registry.is_empty());
    let registries = if names_registry && !delivery.environments.is_empty() {
        match gateway.list_registries(&principal).await {
            Ok(registries) => Some(registries),
            Err(status) => {
                notes.push(format!(
                    "registries could not be read ({}); the default registry is used",
                    status.message()
                ));
                None
            }
        }
    } else {
        Some(Vec::new())
    };
    let environments = environments_of(project_id, &delivery, registries.as_deref(), &mut notes);
    let count = environments.len();
    match gateway
        .sync_environments(&principal, &project_id.to_string(), environments)
        .await
    {
        Ok(_) => Outcome::Synced {
            environments: count,
            notes,
        },
        Err(status) => Outcome::Failed(format!(
            "{} ({:?})",
            if status.message().is_empty() {
                "execution service unavailable"
            } else {
                status.message()
            },
            status.code()
        )),
    }
}

#[cfg(test)]
mod tests {
    use sdi_common::principal::{Role, SYSTEM_USER};

    use super::*;
    use crate::execution::fake::FakeExecution;

    const CONFIG: &str = r#"version: 1
delivery:
  registry: { kind: acr, host: acme.azurecr.io }
  build_workflow: build.yml
  environments:
    - name: staging
      kind: github_actions
      url: https://staging.example.com
      health_path: /health
      auto_promote: true
      config: { workflow: deploy.yml }
    - name: production
      kind: gitops_flux
      protected: true
      registry: Fleet
      config: { repo: acme/fleet, path: apps/api/kustomization.yaml, strategy: kustomize_image }
"#;

    fn registry(id: &str, name: &str, kind: &str, host: &str, is_default: bool) -> Registry {
        Registry {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            host: host.into(),
            is_default,
            ..Default::default()
        }
    }

    fn registries() -> Vec<Registry> {
        vec![
            registry("reg-ghcr", "GitHub", "ghcr", "ghcr.io", true),
            registry("reg-acr", "Azure", "acr", "acme.azurecr.io", false),
            registry("reg-fleet", "fleet", "generic", "registry.fleet.dev", false),
        ]
    }

    #[tokio::test]
    async fn a_usable_delivery_config_is_synced_as_the_system_principal() {
        let execution = FakeExecution {
            registries: registries(),
            ..Default::default()
        };
        let (workspace, project) = (Uuid::now_v7(), Uuid::now_v7());
        let config = sdi_analyzer::parse_config(CONFIG);
        assert!(declares_environments(&config));
        let outcome = sync(&execution, workspace, project, &config, false).await;
        assert_eq!(
            outcome,
            Outcome::Synced {
                environments: 2,
                notes: Vec::new()
            }
        );
        assert_eq!(
            outcome.detail().as_deref(),
            Some("2 configured environments synced")
        );

        let calls = execution.calls();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.project_id, project.to_string());
        // The system principal of the project's workspace, never the member
        // who happened to start the scan.
        assert_eq!(call.principal.user_id, SYSTEM_USER);
        assert_eq!(call.principal.workspace_id, workspace.to_string());
        assert_eq!(call.principal.role, Role::Owner);
        assert_eq!(
            execution.registry_lookups.lock().unwrap()[0],
            call.principal
        );

        let [staging, production] = call.environments.as_slice() else {
            panic!("two environments, got {:?}", call.environments);
        };
        assert_eq!(staging.name, "staging");
        assert_eq!(staging.position, 0);
        assert_eq!(staging.kind, "github_actions");
        assert_eq!(staging.url, "https://staging.example.com");
        assert_eq!(staging.health_path, "/health");
        assert!(staging.auto_promote && !staging.protected);
        assert_eq!(staging.config["workflow"], "deploy.yml");
        // No registry of its own: the one `delivery.registry` describes.
        assert_eq!(staging.registry_id, "reg-acr");
        assert_eq!(production.name, "production");
        assert_eq!(production.position, 1);
        assert_eq!(production.kind, "gitops_flux");
        assert!(production.protected && !production.auto_promote);
        assert_eq!(production.config["repo"], "acme/fleet");
        assert_eq!(production.config["strategy"], "kustomize_image");
        // Named by the workspace registry's name, case-insensitively.
        assert_eq!(production.registry_id, "reg-fleet");
        for env in &call.environments {
            assert_eq!(env.source, SOURCE_CONFIG);
            assert_eq!(env.project_id, project.to_string());
            assert!(env.id.is_empty() && env.current_release_id.is_empty());
        }
    }

    #[tokio::test]
    async fn an_unknown_registry_falls_back_to_the_default_and_says_so() {
        let execution = FakeExecution {
            registries: vec![registry("reg-ghcr", "GitHub", "ghcr", "ghcr.io", true)],
            ..Default::default()
        };
        let config = sdi_analyzer::parse_config(CONFIG);
        let outcome = sync(&execution, Uuid::now_v7(), Uuid::now_v7(), &config, false).await;
        let Outcome::Synced {
            environments: 2,
            notes,
        } = &outcome
        else {
            panic!("{outcome:?}");
        };
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("acr registry acme.azurecr.io is not configured"));
        assert!(notes[1].contains("registry `Fleet` is not configured"));
        assert!(
            execution.calls()[0]
                .environments
                .iter()
                .all(|e| e.registry_id.is_empty())
        );
        assert!(
            outcome
                .detail()
                .unwrap()
                .contains("the default registry is used")
        );

        // Without any registry named, registries are not even read.
        let plain = sdi_analyzer::parse_config(
            "version: 1\ndelivery:\n  environments:\n    - { name: staging, kind: github_actions }\n",
        );
        let quiet = FakeExecution::default();
        sync(&quiet, Uuid::now_v7(), Uuid::now_v7(), &plain, false).await;
        assert!(quiet.registry_lookups.lock().unwrap().is_empty());
        assert_eq!(quiet.calls()[0].environments.len(), 1);
    }

    #[tokio::test]
    async fn nothing_declared_and_broken_configs_leave_execution_alone() {
        let execution = FakeExecution::default();
        let (workspace, project) = (Uuid::now_v7(), Uuid::now_v7());
        for (content, previously) in [
            // No delivery section, never had one.
            ("version: 1\ndeploy:\n  target: vm\n", false),
            // A registry alone declares no environment.
            ("version: 1\ndelivery:\n  registry: { kind: ghcr }\n", false),
            // Errors: the file is ignored, whatever it used to say.
            (
                "version: 1\ndelivery:\n  environments:\n    - { name: a, kind: teleport }\n",
                true,
            ),
            ("{{{ not yaml", true),
        ] {
            let config = sdi_analyzer::parse_config(content);
            let outcome = sync(&execution, workspace, project, &config, previously).await;
            assert_eq!(outcome, Outcome::Skipped, "{content}");
            assert_eq!(outcome.detail(), None);
        }
        // No `.sdi.yml` at all.
        let absent = ProjectConfigDraft::default();
        assert_eq!(
            sync(&execution, workspace, project, &absent, false).await,
            Outcome::Skipped
        );
        assert!(execution.calls().is_empty());
        assert!(execution.registry_lookups.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn removing_the_section_clears_the_config_managed_environments() {
        let execution = FakeExecution::default();
        let (workspace, project) = (Uuid::now_v7(), Uuid::now_v7());
        // The file still exists but no longer declares environments.
        let config = sdi_analyzer::parse_config("version: 1\n");
        let outcome = sync(&execution, workspace, project, &config, true).await;
        assert_eq!(
            outcome,
            Outcome::Synced {
                environments: 0,
                notes: Vec::new()
            }
        );
        // The whole file was deleted.
        sync(
            &execution,
            workspace,
            project,
            &ProjectConfigDraft::default(),
            true,
        )
        .await;
        let calls = execution.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|c| c.environments.is_empty()));
        assert!(calls.iter().all(|c| c.project_id == project.to_string()));
    }

    #[tokio::test]
    async fn an_execution_outage_is_reported_not_raised() {
        let execution = FakeExecution {
            fail_with: Some(tonic::Code::Unavailable),
            ..Default::default()
        };
        let config = sdi_analyzer::parse_config(CONFIG);
        let outcome = sync(&execution, Uuid::now_v7(), Uuid::now_v7(), &config, false).await;
        let Outcome::Failed(reason) = &outcome else {
            panic!("{outcome:?}");
        };
        assert!(reason.contains("execution is down"), "{reason}");
        assert!(reason.contains("Unavailable"), "{reason}");
        assert!(
            outcome
                .detail()
                .unwrap()
                .starts_with("configured environments not synced: ")
        );
        // The registries failed too; the sync was still attempted once.
        assert_eq!(execution.calls().len(), 1);
    }

    #[test]
    fn the_default_registry_wins_among_several_of_a_kind() {
        let registries = vec![
            registry(
                "a",
                "one",
                "ecr",
                "1.dkr.ecr.eu-west-1.amazonaws.com",
                false,
            ),
            registry("b", "two", "ecr", "2.dkr.ecr.eu-west-1.amazonaws.com", true),
        ];
        let none = DeliveryRegistryDraft::default();
        assert_eq!(resolve_registry("ecr", &none, &registries).unwrap(), "b");
        assert_eq!(resolve_registry("one", &none, &registries).unwrap(), "a");
        assert_eq!(resolve_registry("a", &none, &registries).unwrap(), "a");
        assert_eq!(resolve_registry("", &none, &registries).unwrap(), "");
        let declared = DeliveryRegistryDraft {
            kind: "ecr".into(),
            host: "1.dkr.ecr.eu-west-1.amazonaws.com".into(),
            namespace: String::new(),
        };
        assert_eq!(resolve_registry("", &declared, &registries).unwrap(), "a");
        assert!(resolve_registry("quay", &none, &registries).is_err());
    }
}
