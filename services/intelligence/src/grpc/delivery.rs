//! `GenerateDeliveryArtifacts` (CONTRACT.md §22 "Setup files"): the files
//! that wire a project's CI to its registry and its environments'
//! deployers, generated on demand from the project's facts and what the
//! execution service holds for it. Nothing is stored: the same request
//! answers the same files until a scan, a registry or an environment changes.
//!
//! Proposals of kind `ci_cd` and `deployment` carry the same files once the
//! project has environments ([`for_proposal`]), so opening the pull request
//! of an accepted option ships the real setup.

use std::sync::Arc;

use sdi_common::principal::Principal;
use sdi_proto::{
    intelligence::v1::{DeliveryArtifacts, GenerateDeliveryArtifactsRequest, ProposalKind},
    knowledge::v1::ProjectContext,
};
use tonic::Status;

use super::{Caller, IntelligenceGrpc, require};
use crate::{
    convert,
    gateway::ExecutionGateway,
    generators::delivery::{self, Setup},
};

pub(super) async fn generate(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: GenerateDeliveryArtifactsRequest,
) -> Result<DeliveryArtifacts, Status> {
    let project_id = require("project_id", &request.project_id)?;
    let context = svc
        .knowledge
        .project_context(&caller.principal, &project_id)
        .await
        .map_err(|e| e.into_status())?;
    let (registries, environments) = tokio::try_join!(
        svc.execution.list_registries(&caller.principal),
        svc.execution
            .list_environments(&caller.principal, &project_id),
    )
    .map_err(|e| e.into_status())?;
    // Rendering is CPU work on strings; keep it off the async runtime.
    let files = tokio::task::spawn_blocking(move || {
        delivery::for_project(&context, &registries, &environments)
    })
    .await
    .map_err(|error| sdi_common::grpc::internal(format!("delivery generation failed: {error}")))?;
    Ok(convert::delivery_artifacts(&files))
}

/// Kinds whose options ship the delivery setup once environments exist.
pub(super) fn carries_delivery(kind: ProposalKind) -> bool {
    matches!(kind, ProposalKind::CiCd | ProposalKind::Deployment)
}

/// What a proposal's options are wired to: `None` for other kinds, for a
/// project without environments, and when execution cannot be reached. A
/// proposal never fails on this: the options then keep the files of their
/// own pattern.
pub(super) async fn for_proposal(
    execution: &Arc<dyn ExecutionGateway>,
    principal: &Principal,
    kind: ProposalKind,
    context: &ProjectContext,
) -> Option<Setup> {
    if !carries_delivery(kind) {
        return None;
    }
    let project_id = context.project.as_ref().map(|p| p.id.as_str())?;
    let environments = match execution.list_environments(principal, project_id).await {
        Ok(environments) if environments.is_empty() => return None,
        Ok(environments) => environments,
        Err(error) => {
            tracing::warn!(%error, project_id, "environments unavailable; options keep their pattern's files");
            return None;
        }
    };
    let registries = match execution.list_registries(principal).await {
        Ok(registries) => registries,
        Err(error) => {
            tracing::warn!(%error, project_id, "registries unavailable; options keep their pattern's files");
            return None;
        }
    };
    Some(Setup {
        registries,
        environments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::ranking::fixtures, gateway::fake::FakeExecution};
    use sdi_common::principal::Role;

    #[tokio::test]
    async fn only_ci_cd_and_deployment_proposals_ask_for_the_delivery_setup() {
        let fake = Arc::new(FakeExecution::default());
        *fake.environments.lock().unwrap() = vec![sdi_proto::execution::v1::Environment {
            project_id: "proj-1".into(),
            name: "production".into(),
            kind: "github_actions".into(),
            ..Default::default()
        }];
        let execution: Arc<dyn ExecutionGateway> = fake.clone();
        let principal = Principal {
            user_id: "u-1".into(),
            workspace_id: "ws-1".into(),
            role: Role::Member,
        };
        let context = fixtures::context();
        for kind in [
            ProposalKind::Infrastructure,
            ProposalKind::Testing,
            ProposalKind::Security,
            ProposalKind::Documentation,
            ProposalKind::Dependency,
            ProposalKind::Modernization,
            ProposalKind::Unspecified,
        ] {
            assert!(!carries_delivery(kind), "{kind:?}");
            assert!(
                for_proposal(&execution, &principal, kind, &context)
                    .await
                    .is_none()
            );
        }
        assert!(
            fake.callers.lock().unwrap().is_empty(),
            "other kinds never call execution"
        );
        for kind in [ProposalKind::CiCd, ProposalKind::Deployment] {
            let setup = for_proposal(&execution, &principal, kind, &context)
                .await
                .expect("environments exist");
            assert_eq!(setup.environments.len(), 1);
            assert_eq!(setup.environments[0].name, "production");
            assert!(setup.registries.is_empty());
        }
        // The caller's principal travels with both calls.
        assert!(fake.callers.lock().unwrap().iter().all(|p| *p == principal));

        // No environments: nothing to ship. An outage: nothing either.
        fake.environments.lock().unwrap().clear();
        assert!(
            for_proposal(&execution, &principal, ProposalKind::Deployment, &context)
                .await
                .is_none()
        );
        fake.delivery_down
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            for_proposal(&execution, &principal, ProposalKind::CiCd, &context)
                .await
                .is_none()
        );
    }
}
