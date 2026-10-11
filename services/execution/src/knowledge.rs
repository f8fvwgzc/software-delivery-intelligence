//! The calls this service makes to knowledge: closing the loop with the
//! outcome of a pattern-backed run, and naming the project an execution
//! belongs to. Trait-abstracted so tests need no peer.

#[cfg(test)]
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use async_trait::async_trait;
use sdi_proto::knowledge::v1::{
    GetProjectRequest, RecordPatternOutcomeRequest,
    knowledge_service_client::KnowledgeServiceClient,
};
use tonic::transport::Channel;

#[async_trait]
pub trait PatternOutcomes: Send + Sync {
    async fn record(&self, pattern_id: &str, project_id: &str, succeeded: bool) -> Result<()>;

    /// Display name of a project, or empty when knowledge cannot say: the
    /// execution list and header show it next to the title, and
    /// `CreateExecutionRequest` only carries the id.
    async fn project_name(&self, _project_id: &str) -> String {
        String::new()
    }
}

pub struct GrpcPatternOutcomes {
    channel: Channel,
}

impl GrpcPatternOutcomes {
    pub fn new(url: &str) -> Result<Self> {
        Ok(Self {
            channel: sdi_common::grpc::lazy_channel(url)?,
        })
    }
}

#[async_trait]
impl PatternOutcomes for GrpcPatternOutcomes {
    async fn record(&self, pattern_id: &str, project_id: &str, succeeded: bool) -> Result<()> {
        let mut client = KnowledgeServiceClient::new(self.channel.clone());
        client
            .record_pattern_outcome(RecordPatternOutcomeRequest {
                pattern_id: pattern_id.to_owned(),
                project_id: project_id.to_owned(),
                succeeded,
                incident: false,
            })
            .await
            .context("RecordPatternOutcome failed")?;
        Ok(())
    }

    async fn project_name(&self, project_id: &str) -> String {
        let mut client = KnowledgeServiceClient::new(self.channel.clone());
        match client
            .get_project(GetProjectRequest {
                id: project_id.to_owned(),
            })
            .await
        {
            Ok(project) => project.into_inner().name,
            Err(status) => {
                tracing::warn!(project_id, %status, "could not resolve the project name");
                String::new()
            }
        }
    }
}

/// Test double that remembers every call.
#[cfg(test)]
#[derive(Default, Clone)]
pub struct RecordingPatternOutcomes {
    pub calls: Arc<Mutex<Vec<(String, String, bool)>>>,
}

#[cfg(test)]
#[async_trait]
impl PatternOutcomes for RecordingPatternOutcomes {
    async fn record(&self, pattern_id: &str, project_id: &str, succeeded: bool) -> Result<()> {
        self.calls.lock().expect("outcomes lock").push((
            pattern_id.to_owned(),
            project_id.to_owned(),
            succeeded,
        ));
        Ok(())
    }
}
