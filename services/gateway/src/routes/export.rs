//! The data export archive (CONTRACT.md §19): one JSON object streamed as
//! it is assembled, so a workspace with a hundred thousand rows never sits
//! in gateway memory at once.

use std::collections::BTreeMap;

use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use futures::StreamExt;
use sdi_proto::{
    execution::v1 as execution, gitops::v1 as gitops, identity::v1 as identity,
    intelligence::v1 as intelligence, knowledge::v1 as knowledge,
};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Status};

use crate::{
    dto::identity::{AuditEntry, DataControls, Member},
    state::{Auth, Scoped},
};

/// Writes JSON fragments to the response body in order.
struct Out(mpsc::Sender<Result<String, std::io::Error>>);

impl Out {
    async fn raw(&self, text: impl Into<String>) {
        let _ = self.0.send(Ok(text.into())).await;
    }

    async fn value<T: Serialize>(&self, value: &T) {
        self.raw(serde_json::to_string(value).unwrap_or_else(|_| "null".into()))
            .await;
    }

    /// `"key": ` ready for a value.
    async fn key(&self, key: &str) {
        self.raw(format!(
            "{}:",
            serde_json::to_string(key).unwrap_or_default()
        ))
        .await;
    }
}

/// `{entity: [rows]}` from a stream of (entity, json) records. Records are
/// grouped as they arrive, so a service must send each entity's rows
/// together (they do: one table after another).
async fn section<S>(out: &Out, key: &str, mut records: S) -> Option<String>
where
    S: futures::Stream<Item = Result<(String, String), Status>> + Unpin,
{
    out.key(key).await;
    out.raw("{").await;
    let mut current: Option<String> = None;
    let mut first_in_entity = true;
    let mut error = None;
    while let Some(record) = records.next().await {
        match record {
            Ok((entity, json)) => {
                if current.as_deref() != Some(entity.as_str()) {
                    if current.is_some() {
                        out.raw("],").await;
                    }
                    out.key(&entity).await;
                    out.raw("[").await;
                    current = Some(entity);
                    first_in_entity = true;
                }
                if !first_in_entity {
                    out.raw(",").await;
                }
                first_in_entity = false;
                // Rows arrive as JSON text; anything unparsable is kept as a string.
                let valid = serde_json::from_str::<serde_json::Value>(&json).is_ok();
                if valid {
                    out.raw(json).await;
                } else {
                    out.value(&json).await;
                }
            }
            Err(status) => {
                error = Some(match status.code() {
                    Code::Unimplemented => "export is not implemented by this service".to_owned(),
                    _ => status.message().to_owned(),
                });
                break;
            }
        }
    }
    if current.is_some() {
        out.raw("]").await;
    }
    out.raw("},").await;
    error
}

fn record_stream<T, F>(
    result: Result<tonic::Response<tonic::Streaming<T>>, Status>,
    map: F,
) -> futures::stream::BoxStream<'static, Result<(String, String), Status>>
where
    T: Send + 'static,
    F: Fn(T) -> (String, String) + Send + 'static,
{
    match result {
        Ok(response) => response
            .into_inner()
            .map(move |item| item.map(&map))
            .boxed(),
        Err(status) => futures::stream::once(async move { Err(status) }).boxed(),
    }
}

pub fn stream(scoped: Scoped, auth: Auth) -> Response {
    let (tx, rx) = mpsc::channel::<Result<String, std::io::Error>>(64);
    sdi_common::tasks::spawn("workspace-export", async move {
        let out = Out(tx);
        let workspace_id = scoped.workspace_id().to_owned();
        let mut errors: BTreeMap<String, String> = BTreeMap::new();

        out.raw("{").await;
        out.key("workspace").await;
        out.value(&auth.workspace).await;
        out.raw(",").await;
        out.key("exportedAt").await;
        out.value(&Utc::now()).await;
        out.raw(",").await;

        let mut id = scoped.identity();
        out.key("members").await;
        match id
            .list_members(identity::ListMembersRequest {
                workspace_id: workspace_id.clone(),
            })
            .await
        {
            Ok(r) => {
                out.value(
                    &r.into_inner()
                        .members
                        .into_iter()
                        .map(Member::from)
                        .collect::<Vec<_>>(),
                )
                .await
            }
            Err(status) => {
                errors.insert("members".into(), status.message().to_owned());
                out.raw("[]").await;
            }
        }
        out.raw(",").await;
        out.key("dataControls").await;
        match id
            .get_data_controls(identity::GetDataControlsRequest {
                workspace_id: workspace_id.clone(),
            })
            .await
        {
            Ok(r) => out.value(&DataControls::from(r.into_inner())).await,
            Err(status) => {
                errors.insert("dataControls".into(), status.message().to_owned());
                out.raw("null").await;
            }
        }
        out.raw(",").await;
        // Secrets are exported as names only (CONTRACT.md §22): what an
        // importer has to set again, never what it was set to.
        out.key("secrets").await;
        match scoped
            .identity_as_caller()
            .list_secrets(identity::ListSecretsRequest {
                workspace_id: workspace_id.clone(),
            })
            .await
        {
            Ok(r) => {
                out.value(
                    &r.into_inner()
                        .secrets
                        .into_iter()
                        .map(|secret| secret.name)
                        .collect::<Vec<_>>(),
                )
                .await
            }
            Err(status) => {
                errors.insert("secrets".into(), status.message().to_owned());
                out.raw("[]").await;
            }
        }
        out.raw(",").await;

        let knowledge = record_stream(
            scoped
                .knowledge()
                .export_workspace_data(knowledge::ExportWorkspaceDataRequest {
                    workspace_id: workspace_id.clone(),
                })
                .await,
            |r: knowledge::ExportRecord| (r.entity, r.record),
        );
        if let Some(error) = section(&out, "knowledge", knowledge).await {
            errors.insert("knowledge".into(), error);
        }
        let intelligence = record_stream(
            scoped
                .intelligence()
                .export_workspace_data(intelligence::ExportWorkspaceDataRequest {
                    workspace_id: workspace_id.clone(),
                })
                .await,
            |r: intelligence::WorkspaceDataRecord| (r.entity, r.json),
        );
        if let Some(error) = section(&out, "intelligence", intelligence).await {
            errors.insert("intelligence".into(), error);
        }
        let execution = record_stream(
            scoped
                .execution()
                .export_workspace_data(execution::ExportWorkspaceDataRequest {
                    workspace_id: workspace_id.clone(),
                })
                .await,
            |r: execution::WorkspaceDataRecord| (r.entity, r.json),
        );
        if let Some(error) = section(&out, "execution", execution).await {
            errors.insert("execution".into(), error);
        }
        // Clusters (never their kubeconfig), applications, history (CONTRACT.md §24).
        let gitops = record_stream(
            scoped
                .gitops()
                .export_workspace_data(gitops::ExportWorkspaceDataRequest {
                    workspace_id: workspace_id.clone(),
                })
                .await,
            |r: gitops::WorkspaceDataRecord| (r.entity, r.json),
        );
        if let Some(error) = section(&out, "gitops", gitops).await {
            errors.insert("gitops".into(), error);
        }

        out.key("audit").await;
        out.raw("[").await;
        let mut before = None;
        let mut first = true;
        loop {
            let page = match id
                .list_audit(identity::ListAuditRequest {
                    workspace_id: workspace_id.clone(),
                    action_prefix: String::new(),
                    limit: 1000,
                    before,
                })
                .await
            {
                Ok(page) => page.into_inner().entries,
                Err(status) => {
                    errors.insert("audit".into(), status.message().to_owned());
                    break;
                }
            };
            let Some(last) = page.last() else { break };
            before = last.at;
            for entry in page {
                if !first {
                    out.raw(",").await;
                }
                first = false;
                out.value(&AuditEntry::from(entry)).await;
            }
            if before.is_none() {
                break;
            }
        }
        out.raw("],").await;
        out.key("errors").await;
        out.value(&errors).await;
        out.raw("}\n").await;
    });
    let body = Body::from_stream(ReceiverStream::new(rx));
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"sdi-workspace-export.json\"",
            ),
        ],
        body,
    )
        .into_response()
}
