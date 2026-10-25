//! The `gitops` adapter (CONTRACT.md §24.7): the four `gitops.*` steps,
//! run against SDI's own GitOps engine (`services/gitops`, gRPC at
//! `GITOPS_GRPC_URL`) as the person the run acts for (`RunContext::actor`).
//!
//! | action type | what it asks the engine |
//! |---|---|
//! | `gitops.preview` | `SyncApplication` with `dry_run`: what would be created, changed and pruned |
//! | `gitops.sync` | `SyncApplication` (prune, revision, image overrides, execution, timeout); a success only when the result phase is `succeeded` |
//! | `gitops.rollback` | `RollbackApplication` to a history entry |
//! | `gitops.verify_health` | `RefreshApplication` (a comparison against the live cluster cache) until the application is healthy |
//!
//! The deploy step of an `sdi_gitops` environment (`deploy.*` with
//! `params.operation = gitops.sync`) runs as `gitops.sync`: the stage's
//! policy gated it, the engine syncs it.
//!
//! Params: `application` (its id), `prune` (`true` | `false`), `revision`,
//! `history_id` (rollback), `kustomize_images` (a JSON array of
//! `name=newName:tag`, `name:tag` or `name@sha256:…`), `helm_parameters`
//! (a JSON object) and `timeout_seconds`. The sync step of a deployment run
//! names the release instead (`image`, `tag`, `digest`): it asks the engine
//! for the application first and sets the image the way its source takes
//! one, as a kustomize image override or as helm's `image.repository` and
//! `image.tag`.
//!
//! Failures: the engine or a cluster out of reach is `CLUSTER_UNREACHABLE`;
//! a source that does not render `RENDER_FAILED`; a sync that ran and
//! failed `SYNC_FAILED`, naming the resources; an application that does
//! not become healthy `HEALTH_CHECK_FAILED`; the engine refusing the caller
//! `PERMISSION_DENIED`. A plan the adapter cannot make sense of (a missing
//! or malformed param) is `UNKNOWN` with a sentence.
//!
//! Every step is safe to run again after a restart: a sync or rollback this
//! execution already completed (the engine's history names the execution)
//! is a success without a second operation, and an operation of this
//! execution that is still running is waited for, never started twice.

use std::{
    collections::{BTreeMap, HashMap},
    sync::OnceLock,
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use sdi_common::grpc::MAX_MESSAGE_BYTES;
use sdi_proto::gitops::v1::{
    Application, GetApplicationRequest, HistoryEntry, ListHistoryRequest,
    RefreshApplicationRequest, ResourceResult, RollbackApplicationRequest, SyncApplicationRequest,
    SyncResult, gitops_service_client::GitopsServiceClient,
};
use tonic::{
    Code, Request, Status,
    transport::{Channel, Endpoint},
};

use super::{
    Adapter, GITOPS, GITOPS_PREVIEW, GITOPS_ROLLBACK, GITOPS_SYNC, GITOPS_VERIFY_HEALTH,
    OP_GITOPS_SYNC, RunContext, StepInput, StepOutcome, is_deploy_action,
};
use crate::domain::diagnosis;

/// Where the engine listens when `GITOPS_GRPC_URL` says nothing.
pub const DEFAULT_URL: &str = "http://127.0.0.1:50055";
/// What the engine gives a sync or a rollback when the step names no limit.
pub const DEFAULT_SYNC_TIMEOUT_SECONDS: u64 = 600;
/// How long `gitops.verify_health` waits when the step names no limit.
pub const DEFAULT_HEALTH_TIMEOUT_SECONDS: u64 = 300;
/// Longest a step may ask for (the delivery limit, six hours).
pub const MAX_TIMEOUT_SECONDS: u64 = 6 * 60 * 60;
/// Room on top of an operation's own limit for the engine to terminate it
/// and still answer with the result.
const ANSWER_GRACE: Duration = Duration::from_secs(60);
/// Reads come from the engine's memory or one row.
const READ_TIMEOUT: Duration = Duration::from_secs(15);
/// A refresh renders (cached per commit) and compares with the cluster.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_POLL: Duration = Duration::from_secs(5);
/// Resources a step output names per category before "and N more".
const LISTED: usize = 10;
/// History entries looked through for an operation of this execution.
const HISTORY_LOOKBACK: u32 = 50;

/// Result phases (`SyncResult.phase`, `Operation.phase`, `HistoryEntry.phase`).
const PHASE_SUCCEEDED: &str = "succeeded";
const PHASE_FAILED: &str = "failed";
const PHASE_ERROR: &str = "error";
const PHASE_RUNNING: &str = "running";
const PHASE_TERMINATING: &str = "terminating";

/// Source types (`ApplicationSource.type`).
const SOURCE_KUSTOMIZE: &str = "kustomize";
const SOURCE_HELM: &str = "helm";
/// The helm parameters a release image is set through.
pub const HELM_IMAGE_REPOSITORY: &str = "image.repository";
pub const HELM_IMAGE_TAG: &str = "image.tag";

type Step<T> = Result<T, StepOutcome>;

fn failed(code: &str, detail: impl Into<String>) -> StepOutcome {
    StepOutcome::failed(code, detail)
}

fn unknown(detail: impl Into<String>) -> StepOutcome {
    failed(diagnosis::UNKNOWN, detail)
}

/// The engine's endpoint: connected lazily, and without a per-request
/// limit (unlike `sdi_common::grpc::lazy_channel`, whose 60 s would cut a
/// sync short: the engine answers a sync when its operation ends, 600 s by
/// default). Every call sets its own deadline instead.
fn endpoint(url: &str) -> anyhow::Result<Endpoint> {
    Ok(Endpoint::from_shared(url.to_owned())
        .with_context(|| format!("invalid GitOps engine endpoint: {url}"))?
        .connect_timeout(Duration::from_secs(5))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http2_keep_alive_interval(Duration::from_secs(30))
        .keep_alive_while_idle(true))
}

pub struct GitopsEngineAdapter {
    endpoint: Endpoint,
    /// Made on first use: a channel needs the runtime it runs on.
    client: OnceLock<GitopsServiceClient<Channel>>,
    url: String,
    poll: Duration,
}

impl GitopsEngineAdapter {
    pub fn new(url: &str) -> anyhow::Result<Self> {
        let url = url.trim().trim_end_matches('/').to_owned();
        Ok(Self {
            endpoint: endpoint(&url)?,
            client: OnceLock::new(),
            url,
            poll: HEALTH_POLL,
        })
    }

    fn client(&self) -> GitopsServiceClient<Channel> {
        self.client
            .get_or_init(|| {
                GitopsServiceClient::new(self.endpoint.connect_lazy())
                    .max_decoding_message_size(MAX_MESSAGE_BYTES)
                    .max_encoding_message_size(MAX_MESSAGE_BYTES)
            })
            .clone()
    }

    /// A shorter pause between health polls (tests).
    #[cfg(test)]
    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    /// A request carrying the run's principal and a deadline.
    fn request<T>(&self, ctx: &RunContext, message: T, timeout: Duration) -> Step<Request<T>> {
        let Some(actor) = &ctx.actor else {
            return Err(unknown(
                "the step has no principal to act as on the GitOps engine",
            ));
        };
        let mut request = Request::new(message);
        actor.apply(&mut request);
        request.set_timeout(timeout);
        Ok(request)
    }

    /// What a refusal, or the engine being out of reach, means for the step.
    fn refused(&self, action: &str, what: &str, status: &Status) -> StepOutcome {
        let message = status.message().trim();
        let said = if message.is_empty() {
            format!("{:?}", status.code())
        } else {
            message.to_owned()
        };
        let writes = matches!(action, GITOPS_PREVIEW | GITOPS_SYNC | GITOPS_ROLLBACK);
        match status.code() {
            Code::Unavailable | Code::Cancelled => failed(
                diagnosis::CLUSTER_UNREACHABLE,
                format!(
                    "{what} could not reach the GitOps engine at {} or its cluster: {said}",
                    self.url
                ),
            ),
            Code::DeadlineExceeded => failed(
                diagnosis::TIMEOUT,
                format!("the GitOps engine did not answer {what} in time: {said}"),
            ),
            Code::PermissionDenied | Code::Unauthenticated => failed(
                diagnosis::PERMISSION_DENIED,
                format!("the GitOps engine refused {what}: {said}"),
            ),
            Code::FailedPrecondition => match classify(message) {
                Said::Busy => failed(
                    diagnosis::SYNC_FAILED,
                    format!(
                        "the GitOps engine refused {what} because another operation is running on the application; retry once it has ended: {said}"
                    ),
                ),
                Said::Unreachable => failed(
                    diagnosis::CLUSTER_UNREACHABLE,
                    format!("{what} could not reach the cluster: {said}"),
                ),
                Said::Render => failed(
                    diagnosis::RENDER_FAILED,
                    format!("the source of {what} does not render: {said}"),
                ),
                Said::Other if writes => failed(
                    diagnosis::SYNC_FAILED,
                    format!("the GitOps engine refused {what}: {said}"),
                ),
                Said::Other => unknown(format!("the GitOps engine refused {what}: {said}")),
            },
            Code::NotFound => unknown(format!(
                "{what}: the GitOps engine has no such application or history entry in this workspace ({said})"
            )),
            Code::InvalidArgument => unknown(format!(
                "the GitOps engine refused {what} as malformed: {said}"
            )),
            _ => unknown(format!(
                "the GitOps engine failed {what} ({:?}): {said}",
                status.code()
            )),
        }
    }

    async fn application(&self, ctx: &RunContext, action: &str, id: &str) -> Step<Application> {
        let request = self.request(
            ctx,
            GetApplicationRequest { id: id.to_owned() },
            READ_TIMEOUT,
        )?;
        self.get(request)
            .await
            .map_err(|status| self.refused(action, &format!("application {id}"), &status))
    }

    async fn get(&self, request: Request<GetApplicationRequest>) -> Result<Application, Status> {
        let mut client = self.client();
        match tokio::time::timeout(READ_TIMEOUT, client.get_application(request)).await {
            Ok(answer) => answer.map(tonic::Response::into_inner),
            Err(_) => Err(Status::deadline_exceeded(format!(
                "no answer within {}s",
                READ_TIMEOUT.as_secs()
            ))),
        }
    }

    /// The application as the engine sees it now: `RefreshApplication`
    /// compares it against the live cluster cache, where `GetApplication`
    /// alone may answer the last reconcile's snapshot (up to
    /// `GITOPS_RECONCILE_SECONDS` old). A refresh that fails for a passing
    /// reason falls back to that snapshot for this poll; one the engine
    /// refuses for good is the answer.
    async fn live(
        &self,
        ctx: &RunContext,
        id: &str,
        remaining: Duration,
        fell_back: &mut bool,
    ) -> Step<Result<Application, Status>> {
        let timeout = REFRESH_TIMEOUT.min(remaining.max(Duration::from_secs(1)));
        let request = self.request(
            ctx,
            RefreshApplicationRequest {
                id: id.to_owned(),
                hard: false,
            },
            timeout,
        )?;
        let mut client = self.client();
        let refreshed =
            match tokio::time::timeout(timeout, client.refresh_application(request)).await {
                Ok(answer) => answer.map(tonic::Response::into_inner),
                Err(_) => Err(Status::deadline_exceeded(format!(
                    "no answer within {}s",
                    timeout.as_secs()
                ))),
            };
        let status = match refreshed {
            Ok(app) => {
                *fell_back = false;
                return Ok(Ok(app));
            }
            Err(status) if lasting(&status) => return Ok(Err(status)),
            Err(status) => status,
        };
        if !*fell_back {
            ctx.logs.line(format!(
                "refreshing application {id} failed ({:?}: {}); reading its last known state instead",
                status.code(),
                status.message()
            ));
            *fell_back = true;
        }
        let request = self.request(
            ctx,
            GetApplicationRequest { id: id.to_owned() },
            READ_TIMEOUT,
        )?;
        Ok(self.get(request).await)
    }

    async fn history(&self, ctx: &RunContext, action: &str, id: &str) -> Step<Vec<HistoryEntry>> {
        let request = self.request(
            ctx,
            ListHistoryRequest {
                id: id.to_owned(),
                limit: HISTORY_LOOKBACK,
            },
            READ_TIMEOUT,
        )?;
        let mut client = self.client();
        let answer = match tokio::time::timeout(READ_TIMEOUT, client.list_history(request)).await {
            Ok(answer) => answer,
            Err(_) => Err(Status::deadline_exceeded("no answer")),
        };
        answer
            .map(|response| response.into_inner().entries)
            .map_err(|status| {
                self.refused(action, &format!("the history of application {id}"), &status)
            })
    }

    /// The outcome of an operation this execution started before, when the
    /// service restarted while it ran and lost what it answered. One still
    /// running is waited for (up to `limit`); one that succeeded and that
    /// `matches` what the step asks for now is the step's success.
    async fn earlier_operation(
        &self,
        ctx: &RunContext,
        action: &str,
        app: &Application,
        limit: Duration,
        matches: impl Fn(&HistoryEntry) -> bool,
    ) -> Step<Option<StepOutcome>> {
        let execution = ctx.execution_id.to_string();
        let running = |app: &Application| {
            app.operation.as_ref().is_some_and(|operation| {
                operation.execution_id == execution
                    && matches!(operation.phase.as_str(), PHASE_RUNNING | PHASE_TERMINATING)
            })
        };
        let deadline = tokio::time::Instant::now() + limit;
        let mut current = app.clone();
        let mut waited = false;
        while running(&current) {
            if !waited {
                ctx.logs.line(format!(
                    "an operation this execution started earlier is still running on {}; waiting for it instead of starting another",
                    label(&current)
                ));
                waited = true;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(failed(
                    diagnosis::TIMEOUT,
                    format!(
                        "the operation this execution started on {} was still running after {}s",
                        label(&current),
                        limit.as_secs()
                    ),
                ));
            }
            tokio::time::sleep(self.poll).await;
            current = self.application(ctx, action, &app.id).await?;
        }
        let history = self.history(ctx, action, &app.id).await?;
        let Some(entry) = history
            .into_iter()
            .find(|e| e.execution_id == execution && e.phase == PHASE_SUCCEEDED && matches(e))
        else {
            if waited {
                ctx.logs
                    .line("the earlier operation did not succeed; running the step again");
            }
            return Ok(None);
        };
        let what = if action == GITOPS_ROLLBACK {
            "rolled back"
        } else {
            "synced"
        };
        let line = format!(
            "{} was already {what} to {} by this execution (history entry {}); nothing was {what} again",
            label(&current),
            short(&entry.revision),
            entry.id
        );
        ctx.logs.line(line.clone());
        Ok(Some(StepOutcome::Succeeded {
            output: format!("{line}; health {}", or_unknown(&current.health_status)),
            outputs: BTreeMap::from([
                ("revision".to_owned(), entry.revision.clone()),
                ("history_id".to_owned(), entry.id.clone()),
                (
                    "health".to_owned(),
                    or_unknown(&current.health_status).to_owned(),
                ),
            ]),
        }))
    }

    async fn call_sync(
        &self,
        ctx: &RunContext,
        action: &str,
        message: SyncApplicationRequest,
        limit: Duration,
    ) -> Step<SyncResult> {
        let id = message.id.clone();
        let what = if message.dry_run {
            format!("the preview of application {id}")
        } else {
            format!("the sync of application {id}")
        };
        let deadline = limit + ANSWER_GRACE;
        let request = self.request(ctx, message, deadline)?;
        let mut client = self.client();
        match tokio::time::timeout(
            deadline + Duration::from_secs(5),
            client.sync_application(request),
        )
        .await
        {
            Ok(Ok(response)) => Ok(response.into_inner()),
            Ok(Err(status)) => Err(self.refused(action, &what, &status)),
            Err(_) => Err(failed(
                diagnosis::TIMEOUT,
                format!(
                    "the GitOps engine did not answer {what} within {}s",
                    deadline.as_secs()
                ),
            )),
        }
    }

    // --- the four steps ---------------------------------------------------

    async fn preview(&self, step: &StepInput, ctx: &RunContext) -> Step<StepOutcome> {
        let id = application_id(step)?;
        let prune = flag(step, "prune")?;
        let timeout = seconds(step, "timeout_seconds", DEFAULT_SYNC_TIMEOUT_SECONDS)?;
        let mut images = kustomize_images(step)?;
        let mut helm = helm_parameters(step)?;
        let app = self.application(ctx, GITOPS_PREVIEW, id).await?;
        release_override(step, ctx, &app, &mut images, &mut helm)?;
        let result = self
            .call_sync(
                ctx,
                GITOPS_PREVIEW,
                SyncApplicationRequest {
                    id: id.to_owned(),
                    revision: param(step, "revision").to_owned(),
                    prune,
                    dry_run: true,
                    resources: Vec::new(),
                    kustomize_images: images,
                    helm_parameters: helm.into_iter().collect::<HashMap<_, _>>(),
                    execution_id: ctx.execution_id.to_string(),
                    timeout_seconds: u32::try_from(timeout).unwrap_or(u32::MAX),
                },
                Duration::from_secs(timeout),
            )
            .await?;
        if result.phase != PHASE_SUCCEEDED {
            return Err(operation_failure(GITOPS_PREVIEW, &app, &result));
        }
        Ok(preview_outcome(ctx, &app, &result, prune))
    }

    async fn sync(&self, step: &StepInput, ctx: &RunContext) -> Step<StepOutcome> {
        let id = application_id(step)?;
        let prune = flag(step, "prune")?;
        let timeout = seconds(step, "timeout_seconds", DEFAULT_SYNC_TIMEOUT_SECONDS)?;
        let revision = param(step, "revision");
        let mut images = kustomize_images(step)?;
        let mut helm = helm_parameters(step)?;
        let app = self.application(ctx, GITOPS_SYNC, id).await?;
        let image = release_override(step, ctx, &app, &mut images, &mut helm)?;

        let earlier = self
            .earlier_operation(
                ctx,
                GITOPS_SYNC,
                &app,
                Duration::from_secs(timeout),
                |entry| same_revision(entry, revision) && declares(entry, &images, &helm),
            )
            .await?;
        if let Some(done) = earlier {
            return Ok(done);
        }
        ctx.logs.line(format!(
            "syncing {} to {}{}{}",
            label(&app),
            if revision.is_empty() {
                "the source's revision"
            } else {
                revision
            },
            image
                .as_deref()
                .map(|image| format!(" with image {image}"))
                .unwrap_or_default(),
            if prune { ", pruning" } else { "" }
        ));
        let result = self
            .call_sync(
                ctx,
                GITOPS_SYNC,
                SyncApplicationRequest {
                    id: id.to_owned(),
                    revision: revision.to_owned(),
                    prune,
                    dry_run: false,
                    resources: Vec::new(),
                    kustomize_images: images,
                    helm_parameters: helm.into_iter().collect::<HashMap<_, _>>(),
                    execution_id: ctx.execution_id.to_string(),
                    timeout_seconds: u32::try_from(timeout).unwrap_or(u32::MAX),
                },
                Duration::from_secs(timeout),
            )
            .await?;
        operation_outcome(GITOPS_SYNC, ctx, &app, result)
    }

    async fn rollback(&self, step: &StepInput, ctx: &RunContext) -> Step<StepOutcome> {
        let id = application_id(step)?;
        let history_id = param(step, "history_id");
        if history_id.is_empty() {
            return Err(unknown(
                "the rollback names no history entry to go back to (params.history_id)",
            ));
        }
        let prune = flag(step, "prune")?;
        // RollbackApplication carries no limit: the engine's own applies.
        let timeout = seconds(step, "timeout_seconds", DEFAULT_SYNC_TIMEOUT_SECONDS)?
            .max(DEFAULT_SYNC_TIMEOUT_SECONDS);
        let app = self.application(ctx, GITOPS_ROLLBACK, id).await?;
        let target_revision = self
            .history(ctx, GITOPS_ROLLBACK, id)
            .await?
            .into_iter()
            .find(|entry| entry.id == history_id)
            .map(|entry| entry.revision);
        let earlier = self
            .earlier_operation(
                ctx,
                GITOPS_ROLLBACK,
                &app,
                Duration::from_secs(timeout),
                |entry| {
                    entry.id != history_id
                        && target_revision
                            .as_deref()
                            .is_none_or(|revision| entry.revision == revision)
                },
            )
            .await?;
        if let Some(done) = earlier {
            return Ok(done);
        }
        ctx.logs.line(format!(
            "rolling {} back to history entry {history_id}{}{}",
            label(&app),
            target_revision
                .as_deref()
                .map(|revision| format!(" ({})", short(revision)))
                .unwrap_or_default(),
            if prune { ", pruning" } else { "" }
        ));
        let deadline = Duration::from_secs(timeout) + ANSWER_GRACE;
        let what = format!("the rollback of application {id}");
        let request = self.request(
            ctx,
            RollbackApplicationRequest {
                id: id.to_owned(),
                history_id: history_id.to_owned(),
                prune,
                execution_id: ctx.execution_id.to_string(),
            },
            deadline,
        )?;
        let mut client = self.client();
        let result = match tokio::time::timeout(
            deadline + Duration::from_secs(5),
            client.rollback_application(request),
        )
        .await
        {
            Ok(Ok(response)) => response.into_inner(),
            Ok(Err(status)) => return Err(self.refused(GITOPS_ROLLBACK, &what, &status)),
            Err(_) => {
                return Err(failed(
                    diagnosis::TIMEOUT,
                    format!(
                        "the GitOps engine did not answer {what} within {}s",
                        deadline.as_secs()
                    ),
                ));
            }
        };
        operation_outcome(GITOPS_ROLLBACK, ctx, &app, result)
    }

    async fn verify_health(&self, step: &StepInput, ctx: &RunContext) -> Step<StepOutcome> {
        let id = application_id(step)?;
        let limit = seconds(step, "timeout_seconds", DEFAULT_HEALTH_TIMEOUT_SECONDS)?;
        let started = tokio::time::Instant::now();
        let deadline = started + Duration::from_secs(limit);
        let what = format!("application {id}");
        let mut polls = 0u32;
        // Degraded or missing twice in a row fails the step: once may be a
        // cluster cache that has not seen the sync's objects yet.
        let mut bad = 0u32;
        let mut last_app: Option<Application> = None;
        let mut last_error: Option<Status> = None;
        let mut last_health = String::new();
        let mut fell_back = false;
        loop {
            polls += 1;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match self.live(ctx, id, remaining, &mut fell_back).await? {
                Ok(app) => {
                    last_error = None;
                    let health = or_unknown(&app.health_status).to_owned();
                    if health != last_health {
                        ctx.logs.line(format!(
                            "{} is {health}{} (poll {polls})",
                            label(&app),
                            message_suffix(&app.health_message)
                        ));
                        last_health = health.clone();
                    }
                    match health.as_str() {
                        "healthy" => {
                            return Ok(StepOutcome::Succeeded {
                                output: format!(
                                    "{} is healthy at {} ({} resources, {}) after {polls} polls in {:.1}s",
                                    label(&app),
                                    short(if app.synced_revision.is_empty() {
                                        &app.revision
                                    } else {
                                        &app.synced_revision
                                    }),
                                    app.resource_count.max(app.resources.len() as u32),
                                    or_unknown(&app.sync_status).replace('_', " "),
                                    started.elapsed().as_secs_f64()
                                ),
                                outputs: BTreeMap::from([(
                                    "health".to_owned(),
                                    "healthy".to_owned(),
                                )]),
                            });
                        }
                        "degraded" | "missing" => {
                            bad += 1;
                            if bad >= 2 {
                                return Err(failed(
                                    diagnosis::HEALTH_CHECK_FAILED,
                                    format!(
                                        "{} is {health}{}{}",
                                        label(&app),
                                        message_suffix(&app.health_message),
                                        unhealthy(&app)
                                    ),
                                ));
                            }
                        }
                        _ => bad = 0,
                    }
                    last_app = Some(app);
                }
                Err(status) if lasting(&status) => {
                    return Err(self.refused(GITOPS_VERIFY_HEALTH, &what, &status));
                }
                // The engine may be restarting: keep asking until the limit.
                Err(status) => {
                    if last_error.is_none() {
                        ctx.logs.line(format!(
                            "the GitOps engine did not answer for {what}: {} (poll {polls})",
                            status.message()
                        ));
                    }
                    last_error = Some(status);
                }
            }
            if tokio::time::Instant::now() + self.poll >= deadline {
                break;
            }
            tokio::time::sleep(self.poll).await;
        }
        if let Some(status) = last_error.filter(|_| last_app.is_none()) {
            return Err(self.refused(GITOPS_VERIFY_HEALTH, &what, &status));
        }
        let Some(app) = last_app else {
            return Err(unknown(format!("{what} could not be read")));
        };
        // The engine could not compare the application because its cluster
        // is out of reach: the health it reports is the last one it knew.
        if let Some(condition) = app
            .conditions
            .iter()
            .find(|c| classify(c) == Said::Unreachable)
        {
            return Err(failed(
                diagnosis::CLUSTER_UNREACHABLE,
                format!("the health of {} cannot be read: {condition}", label(&app)),
            ));
        }
        Err(failed(
            diagnosis::HEALTH_CHECK_FAILED,
            format!(
                "{} was still {} after {limit}s ({polls} polls){}{}",
                label(&app),
                or_unknown(&app.health_status),
                message_suffix(&app.health_message),
                unhealthy(&app)
            ),
        ))
    }
}

#[async_trait]
impl Adapter for GitopsEngineAdapter {
    fn name(&self) -> &'static str {
        GITOPS
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let outcome = match step.action_type.as_str() {
            GITOPS_PREVIEW => self.preview(step, ctx).await,
            GITOPS_SYNC => self.sync(step, ctx).await,
            // An sdi_gitops environment's deploy step: its stage's policy
            // gated it; the engine syncs the release image.
            action if is_deploy_action(action) && param(step, "operation") == OP_GITOPS_SYNC => {
                self.sync(step, ctx).await
            }
            GITOPS_ROLLBACK => self.rollback(step, ctx).await,
            GITOPS_VERIFY_HEALTH => self.verify_health(step, ctx).await,
            other => Err(unknown(format!("the gitops adapter cannot run {other}"))),
        };
        match outcome {
            Ok(outcome) | Err(outcome) => outcome,
        }
    }
}

// --- params ------------------------------------------------------------------

fn param<'a>(step: &'a StepInput, key: &str) -> &'a str {
    step.params.get(key).map(|v| v.trim()).unwrap_or_default()
}

fn application_id(step: &StepInput) -> Step<&str> {
    match param(step, "application") {
        "" => Err(unknown(
            "the step names no GitOps application (params.application)",
        )),
        id => Ok(id),
    }
}

fn flag(step: &StepInput, key: &str) -> Step<bool> {
    match param(step, key) {
        "" | "false" => Ok(false),
        "true" => Ok(true),
        other => Err(unknown(format!(
            "params.{key} must be true or false (got {other:?})"
        ))),
    }
}

fn seconds(step: &StepInput, key: &str, default: u64) -> Step<u64> {
    match param(step, key) {
        "" => Ok(default),
        raw => match raw.parse::<u64>() {
            Ok(seconds) if (1..=MAX_TIMEOUT_SECONDS).contains(&seconds) => Ok(seconds),
            _ => Err(unknown(format!(
                "params.{key} must be a number of seconds between 1 and {MAX_TIMEOUT_SECONDS} (got {raw:?})"
            ))),
        },
    }
}

/// `params.kustomize_images`: a JSON array of image overrides.
fn kustomize_images(step: &StepInput) -> Step<Vec<String>> {
    let raw = param(step, "kustomize_images");
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let malformed = || {
        unknown(
            "params.kustomize_images must be a JSON array of image overrides such as [\"ghcr.io/acme/api:1.2.3\"]",
        )
    };
    let images: Vec<String> = serde_json::from_str(raw).map_err(|_| malformed())?;
    if images.iter().any(|image| {
        image.trim().is_empty() || image.chars().any(|c| c.is_whitespace() || c.is_control())
    }) {
        return Err(malformed());
    }
    Ok(images
        .into_iter()
        .map(|image| image.trim().to_owned())
        .collect())
}

/// `params.helm_parameters`: a JSON object of `--set` parameters (strings,
/// numbers and booleans, passed on as text).
fn helm_parameters(step: &StepInput) -> Step<BTreeMap<String, String>> {
    let raw = param(step, "helm_parameters");
    if raw.is_empty() {
        return Ok(BTreeMap::new());
    }
    let malformed = || {
        unknown(
            "params.helm_parameters must be a JSON object of helm parameters such as {\"image.tag\": \"1.2.3\"}",
        )
    };
    let values: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(raw).map_err(|_| malformed())?;
    values
        .into_iter()
        .map(|(key, value)| {
            let key = key.trim().to_owned();
            if key.is_empty() {
                return Err(malformed());
            }
            let text = match value {
                serde_json::Value::String(text) => text,
                serde_json::Value::Number(number) => number.to_string(),
                serde_json::Value::Bool(flag) => flag.to_string(),
                _ => return Err(malformed()),
            };
            Ok((key, text))
        })
        .collect()
}

/// The release a deployment run deploys (`params.image` with its tag or
/// digest), added to the overrides the way the application's source takes
/// an image. `None` when the step names no release image.
fn release_override(
    step: &StepInput,
    ctx: &RunContext,
    app: &Application,
    images: &mut Vec<String>,
    helm: &mut BTreeMap<String, String>,
) -> Step<Option<String>> {
    let image = param(step, "image");
    if image.is_empty() {
        return Ok(None);
    }
    let tag = param(step, "tag");
    // The digest the registry step verified, when this run has one.
    let digest = ctx
        .outputs
        .get("digest")
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .unwrap_or(param(step, "digest"));
    let reference = match (tag, digest) {
        ("", "") => {
            return Err(unknown(format!(
                "the release image {image} has neither a tag nor a digest to deploy"
            )));
        }
        ("", digest) => format!("{image}@{digest}"),
        (tag, _) => format!("{image}:{tag}"),
    };
    let kind = app
        .source
        .as_ref()
        .map(|source| source.r#type.trim())
        .unwrap_or_default();
    let helm_values = |helm: &mut BTreeMap<String, String>| -> Step<()> {
        if tag.is_empty() {
            return Err(unknown(format!(
                "{} is a helm source, which takes the image as {HELM_IMAGE_REPOSITORY} and {HELM_IMAGE_TAG}, and the release {image} has no tag",
                label(app)
            )));
        }
        helm.insert(HELM_IMAGE_REPOSITORY.to_owned(), image.to_owned());
        helm.insert(HELM_IMAGE_TAG.to_owned(), tag.to_owned());
        Ok(())
    };
    match kind {
        SOURCE_KUSTOMIZE => images.push(reference.clone()),
        SOURCE_HELM => helm_values(helm)?,
        // Detected when it renders: the engine stores both and the
        // renderer reads the one its source type takes.
        "" => {
            ctx.logs.line(format!(
                "{} detects its source type when it renders, so {reference} is set both as a kustomize image override and as helm's {HELM_IMAGE_REPOSITORY}/{HELM_IMAGE_TAG}",
                label(app)
            ));
            images.push(reference.clone());
            if !tag.is_empty() {
                helm_values(helm)?;
            }
        }
        other => {
            return Err(unknown(format!(
                "{} renders a {other} source, which takes no image override: make its source kustomize or helm to deploy releases through it",
                label(app)
            )));
        }
    }
    Ok(Some(reference))
}

/// The history entry's revision is the one the step asks for (any, when it
/// names none; a branch or tag matches the source as it was synced).
fn same_revision(entry: &HistoryEntry, revision: &str) -> bool {
    revision.is_empty()
        || entry.revision == revision
        || (revision.len() >= 7 && entry.revision.starts_with(revision))
        || entry
            .source
            .as_ref()
            .is_some_and(|source| source.revision == revision)
}

/// The entry's source declares every override the step asks for.
fn declares(entry: &HistoryEntry, images: &[String], helm: &BTreeMap<String, String>) -> bool {
    if images.is_empty() && helm.is_empty() {
        return true;
    }
    let Some(source) = &entry.source else {
        return false;
    };
    images
        .iter()
        .all(|image| source.kustomize_images.iter().any(|i| i == image))
        && helm
            .iter()
            .all(|(key, value)| source.helm_parameters.get(key) == Some(value))
}

// --- results -----------------------------------------------------------------

/// What a `FAILED_PRECONDITION` or an operation's message is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Said {
    /// Another operation holds the application.
    Busy,
    /// The cluster's API server cannot be reached.
    Unreachable,
    /// The source does not render.
    Render,
    Other,
}

fn classify(message: &str) -> Said {
    let text = message.to_ascii_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| text.contains(w));
    let unreachable = any(&[
        "unreachable",
        "could not reach",
        "cannot reach",
        "can't reach",
        "connection refused",
        "no route to host",
        "no such host",
        "i/o timeout",
        "dial tcp",
        "tls handshake",
        "unable to connect",
    ]);
    if text.contains("operation")
        && any(&["already running", "is running", "in progress", "already in"])
    {
        Said::Busy
    } else if unreachable && text.contains("cluster") {
        Said::Unreachable
    } else if any(&["render", "kustomiz", "helm", "chart", "template"]) {
        Said::Render
    } else if unreachable {
        Said::Unreachable
    } else {
        Said::Other
    }
}

/// A refusal that asking again will not change.
fn lasting(status: &Status) -> bool {
    matches!(
        status.code(),
        Code::NotFound | Code::PermissionDenied | Code::Unauthenticated | Code::InvalidArgument
    )
}

fn or_unknown(text: &str) -> &str {
    match text.trim() {
        "" => "unknown",
        text => text,
    }
}

fn short(revision: &str) -> &str {
    let revision = revision.trim();
    if revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        &revision[..7]
    } else if revision.is_empty() {
        "its revision"
    } else {
        revision
    }
}

fn message_suffix(message: &str) -> String {
    match message.trim() {
        "" => String::new(),
        message => format!(" ({})", message.trim_end_matches('.')),
    }
}

/// `application api (0192…)`.
fn label(app: &Application) -> String {
    if app.name.trim().is_empty() {
        format!("application {}", app.id)
    } else {
        format!("application {} ({})", app.name.trim(), app.id)
    }
}

/// `Deployment shop/api`, `ClusterRole reader`.
fn resource(kind: &str, namespace: &str, name: &str) -> String {
    if namespace.trim().is_empty() {
        format!("{kind} {name}")
    } else {
        format!("{kind} {namespace}/{name}")
    }
}

fn result_name(result: &ResourceResult) -> String {
    let mut text = resource(&result.kind, &result.namespace, &result.name);
    if !result.hook_phase.is_empty() {
        text.push_str(&format!(" ({} hook)", result.hook_phase.replace('_', " ")));
    }
    text
}

/// `a, b, c and 4 more`.
fn listing(items: &[String]) -> String {
    let shown = items
        .iter()
        .take(LISTED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > LISTED {
        format!("{shown} and {} more", items.len() - LISTED)
    } else {
        shown
    }
}

/// The resources that are not healthy, for a failed health check.
fn unhealthy(app: &Application) -> String {
    let bad: Vec<String> = app
        .resources
        .iter()
        .filter(|r| !r.hook && !matches!(r.health_status.as_str(), "" | "healthy"))
        .map(|r| {
            format!(
                "{}: {}{}",
                resource(&r.kind, &r.namespace, &r.name),
                r.health_status,
                message_suffix(&r.health_message)
            )
        })
        .collect();
    if bad.is_empty() {
        String::new()
    } else {
        format!("; not healthy: {}", listing(&bad))
    }
}

/// Whether any resource was acted on (a sync that ran, not one that could
/// not start).
fn any_ran(result: &SyncResult) -> bool {
    result
        .resources
        .iter()
        .any(|r| matches!(r.status.as_str(), "synced" | "pruned" | "failed"))
}

/// The failure a result that is not `succeeded` stands for.
fn operation_failure(action: &str, app: &Application, result: &SyncResult) -> StepOutcome {
    let what = match action {
        GITOPS_PREVIEW => "the preview of",
        GITOPS_ROLLBACK => "the rollback of",
        _ => "the sync of",
    };
    let message = result.message.trim();
    let said = if message.is_empty() {
        String::new()
    } else {
        format!(": {}", message.trim_end_matches('.'))
    };
    let at = if result.revision.is_empty() {
        String::new()
    } else {
        format!(" at {}", short(&result.revision))
    };
    match result.phase.as_str() {
        PHASE_ERROR => match (classify(message), any_ran(result)) {
            (Said::Unreachable, _) => failed(
                diagnosis::CLUSTER_UNREACHABLE,
                format!(
                    "{what} {}{at} could not start: the cluster is out of reach{said}",
                    label(app)
                ),
            ),
            (Said::Busy, _) | (_, true) => failed(
                diagnosis::SYNC_FAILED,
                format!("{what} {}{at} ended in error{said}", label(app)),
            ),
            (_, false) => failed(
                diagnosis::RENDER_FAILED,
                format!(
                    "{what} {}{at} could not start: its source did not render{said}",
                    label(app)
                ),
            ),
        },
        PHASE_FAILED => {
            let broken: Vec<String> = result
                .resources
                .iter()
                .filter(|r| r.status == "failed")
                .map(|r| format!("{}{}", result_name(r), message_suffix(&r.message)))
                .collect();
            let which = if broken.is_empty() {
                String::new()
            } else {
                format!(
                    "; {} failed: {}",
                    if broken.len() == 1 {
                        "1 resource".to_owned()
                    } else {
                        format!("{} resources", broken.len())
                    },
                    listing(&broken)
                )
            };
            failed(
                diagnosis::SYNC_FAILED,
                format!("{what} {}{at} failed{said}{which}", label(app)),
            )
        }
        other => failed(
            diagnosis::SYNC_FAILED,
            format!(
                "{what} {}{at} ended with phase {:?}{said}",
                label(app),
                or_unknown(other)
            ),
        ),
    }
}

/// A sync or rollback that answered: a success only when it `succeeded`.
fn operation_outcome(
    action: &str,
    ctx: &RunContext,
    app: &Application,
    result: SyncResult,
) -> Step<StepOutcome> {
    for r in &result.resources {
        ctx.logs.line(format!(
            "{}: {}{}",
            result_name(r),
            or_unknown(&r.status).replace('_', " "),
            message_suffix(&r.message)
        ));
    }
    if result.phase != PHASE_SUCCEEDED {
        return Err(operation_failure(action, app, &result));
    }
    let count = |status: &str| {
        result
            .resources
            .iter()
            .filter(|r| r.status == status)
            .count()
    };
    let health = or_unknown(&result.health_status).to_owned();
    let output = format!(
        "{} {} to {}: {} synced, {} pruned, {} unchanged{}; history entry {}; health {health}",
        if action == GITOPS_ROLLBACK {
            "rolled back"
        } else {
            "synced"
        },
        label(app),
        short(&result.revision),
        count("synced"),
        count("pruned"),
        count("unchanged"),
        match count("prune_skipped") {
            0 => String::new(),
            n => format!(", {n} left in place (prune is off)"),
        },
        if result.history_id.is_empty() {
            "(none)"
        } else {
            &result.history_id
        }
    );
    Ok(StepOutcome::Succeeded {
        output,
        outputs: BTreeMap::from([
            ("revision".to_owned(), result.revision.clone()),
            ("history_id".to_owned(), result.history_id.clone()),
            ("health".to_owned(), health),
        ]),
    })
}

/// What a dry run says a sync would do, by kind and name.
fn preview_outcome(
    ctx: &RunContext,
    app: &Application,
    result: &SyncResult,
    prune: bool,
) -> StepOutcome {
    let live_status = |r: &ResourceResult| {
        app.resources
            .iter()
            .find(|s| {
                s.group == r.group
                    && s.kind == r.kind
                    && s.namespace == r.namespace
                    && s.name == r.name
            })
            .map(|s| s.sync_status.as_str())
    };
    let (mut create, mut change, mut prune_list, mut kept, mut broken) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut unchanged = 0usize;
    for r in &result.resources {
        let name = result_name(r);
        match r.status.as_str() {
            "synced" => match live_status(r) {
                None | Some("missing") => create.push(name),
                Some(_) => change.push(name),
            },
            "pruned" => prune_list.push(name),
            "prune_skipped" => kept.push(name),
            "failed" => broken.push(format!("{name}{}", message_suffix(&r.message))),
            _ => unchanged += 1,
        }
    }
    let mut lines = vec![format!(
        "preview of {} at {}: {} to create, {} to change, {} to prune, {unchanged} unchanged",
        label(app),
        short(&result.revision),
        create.len(),
        change.len(),
        prune_list.len()
    )];
    for (heading, items) in [
        ("would create", &create),
        ("would change", &change),
        ("would prune", &prune_list),
        ("would fail", &broken),
    ] {
        if !items.is_empty() {
            lines.push(format!("{heading}: {}", listing(items)));
        }
    }
    if !kept.is_empty() {
        lines.push(format!(
            "no longer desired but kept{}: {}",
            if prune { "" } else { " (prune is off)" },
            listing(&kept)
        ));
    }
    lines.push("nothing was applied (dry run)".to_owned());
    for line in &lines {
        ctx.logs.line(line.clone());
    }
    StepOutcome::Succeeded {
        output: lines.join("\n"),
        outputs: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::Ordering};

    use sdi_common::principal::{Principal, Role};
    use sdi_proto::gitops::v1::{ApplicationSource, Operation, ResourceResult, SyncResult};
    use uuid::Uuid;

    use super::*;
    use crate::tests::fakes::gitops::{self as fake, FakeGitops, Scripted};

    const WS: &str = "00000000-0000-7000-8000-0000000000e1";

    fn actor() -> Principal {
        Principal {
            user_id: "user-approver".into(),
            workspace_id: WS.into(),
            role: Role::Approver,
        }
    }

    fn step(action_type: &str, params: &[(&str, &str)]) -> StepInput {
        StepInput {
            key: action_type.trim_start_matches("gitops.").into(),
            title: action_type.into(),
            action_type: action_type.into(),
            adapter: "gitops".into(),
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            attempt: 1,
        }
    }

    fn context(execution_id: Uuid) -> RunContext {
        RunContext {
            execution_id,
            actor: Some(actor()),
            ..Default::default()
        }
    }

    async fn serve(fake: FakeGitops) -> (Arc<FakeGitops>, GitopsEngineAdapter) {
        let (fake, url) = fake.serve().await;
        let adapter = GitopsEngineAdapter::new(&url)
            .unwrap()
            .with_poll(Duration::from_millis(10));
        (fake, adapter)
    }

    fn succeeded(outcome: &StepOutcome) -> (String, BTreeMap<String, String>) {
        match outcome {
            StepOutcome::Succeeded { output, outputs } => (output.clone(), outputs.clone()),
            other => panic!("expected a success, got {other:?}"),
        }
    }

    fn failure(outcome: &StepOutcome) -> (String, String) {
        match outcome {
            StepOutcome::Failed {
                code, raw_output, ..
            } => (code.clone(), raw_output.clone()),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// The dry run lists what would be created, changed and pruned, by
    /// kind and name, and changes nothing.
    #[tokio::test]
    async fn preview_lists_what_a_sync_would_create_change_and_prune() {
        let (fake, adapter) = serve(FakeGitops::default().with_app(fake::kustomize_app())).await;
        let outcome = adapter
            .run(
                &step(
                    GITOPS_PREVIEW,
                    &[("application", fake::APP), ("prune", "true")],
                ),
                &context(Uuid::now_v7()),
            )
            .await;
        let (output, outputs) = succeeded(&outcome);
        assert!(
            output.starts_with(
                "preview of application api (0192f6a0-0000-7000-8000-00000000a991) at 9f8e7d6: 1 to create, 1 to change, 1 to prune, 1 unchanged"
            ),
            "{output}"
        );
        assert!(
            output.contains("would create: Service shop/api"),
            "{output}"
        );
        assert!(
            output.contains("would change: Deployment shop/api"),
            "{output}"
        );
        assert!(
            output.contains("would prune: ConfigMap shop/legacy"),
            "{output}"
        );
        assert!(output.ends_with("nothing was applied (dry run)"));
        assert!(outputs.is_empty());
        let syncs = fake.syncs();
        assert_eq!(syncs.len(), 1);
        assert!(syncs[0].1.dry_run && syncs[0].1.prune);
        assert!(fake.history(fake::APP).is_empty(), "a dry run is no sync");
        // Without prune the extraneous resource is only reported as kept.
        let outcome = adapter
            .run(
                &step(GITOPS_PREVIEW, &[("application", fake::APP)]),
                &context(Uuid::now_v7()),
            )
            .await;
        let (output, _) = succeeded(&outcome);
        assert!(output.contains("0 to prune"), "{output}");
        assert!(
            output.contains("no longer desired but kept (prune is off): ConfigMap shop/legacy"),
            "{output}"
        );
    }

    /// A sync forwards the run's principal and everything the step names,
    /// and reports revision, history entry and health.
    #[tokio::test]
    async fn sync_forwards_the_principal_and_the_params_and_reports_the_result() {
        let (fake, adapter) = serve(FakeGitops::default().with_app(fake::kustomize_app())).await;
        let execution = Uuid::now_v7();
        let outcome = adapter
            .run(
                &step(
                    GITOPS_SYNC,
                    &[
                        ("application", fake::APP),
                        ("prune", "true"),
                        ("revision", "main"),
                        ("kustomize_images", r#"["ghcr.io/acme/api:1.4.0"]"#),
                        ("helm_parameters", r#"{"replicas": 3}"#),
                        ("timeout_seconds", "90"),
                    ],
                ),
                &context(execution),
            )
            .await;
        let (output, outputs) = succeeded(&outcome);
        assert!(output.starts_with("synced application api"), "{output}");
        assert!(output.contains("health healthy"), "{output}");
        assert_eq!(outputs["revision"], fake::REVISION);
        assert_eq!(outputs["health"], "healthy");
        let history = fake.history(fake::APP);
        assert_eq!(outputs["history_id"], history[0].id);
        let syncs = fake.syncs();
        assert_eq!(syncs.len(), 1);
        let (principal, request) = &syncs[0];
        assert_eq!(principal.user_id, "user-approver");
        assert_eq!(principal.role, Role::Approver);
        assert_eq!(principal.workspace_id, WS);
        assert_eq!(request.id, fake::APP);
        assert_eq!(request.revision, "main");
        assert!(request.prune && !request.dry_run);
        assert_eq!(request.kustomize_images, ["ghcr.io/acme/api:1.4.0"]);
        assert_eq!(request.helm_parameters["replicas"], "3");
        assert_eq!(request.execution_id, execution.to_string());
        assert_eq!(request.timeout_seconds, 90);
    }

    /// A deployment run names the release: a kustomize source gets it as
    /// an image override, a helm source as image.repository/image.tag, a
    /// plain directory cannot take it.
    #[tokio::test]
    async fn the_release_image_is_set_the_way_the_source_takes_it() {
        let mut helm = fake::kustomize_app();
        helm.id = fake::HELM_APP.into();
        helm.name = "web".into();
        helm.source = Some(ApplicationSource {
            r#type: "helm".into(),
            path: "charts/web".into(),
            ..Default::default()
        });
        let mut directory = fake::kustomize_app();
        directory.id = "0192f6a0-0000-7000-8000-00000000a993".into();
        directory.source = Some(ApplicationSource {
            r#type: "directory".into(),
            ..Default::default()
        });
        let (fake, adapter) = serve(
            FakeGitops::default()
                .with_app(fake::kustomize_app())
                .with_app(helm)
                .with_app(directory.clone()),
        )
        .await;
        let release = |app: &str| {
            step(
                GITOPS_SYNC,
                &[
                    ("application", app),
                    ("image", "ghcr.io/acme/api"),
                    ("tag", "a1b2c3d"),
                    ("digest", ""),
                ],
            )
        };
        let outcome = adapter
            .run(&release(fake::APP), &context(Uuid::now_v7()))
            .await;
        succeeded(&outcome);
        let outcome = adapter
            .run(&release(fake::HELM_APP), &context(Uuid::now_v7()))
            .await;
        succeeded(&outcome);
        let syncs = fake.syncs();
        assert_eq!(syncs[0].1.kustomize_images, ["ghcr.io/acme/api:a1b2c3d"]);
        assert!(syncs[0].1.helm_parameters.is_empty());
        assert!(syncs[1].1.kustomize_images.is_empty());
        assert_eq!(
            syncs[1].1.helm_parameters,
            HashMap::from([
                ("image.repository".to_owned(), "ghcr.io/acme/api".to_owned()),
                ("image.tag".to_owned(), "a1b2c3d".to_owned()),
            ])
        );
        // The engine keeps the override in the source.
        let stored = fake.app(fake::APP).source.unwrap();
        assert_eq!(stored.kustomize_images, ["ghcr.io/acme/api:a1b2c3d"]);

        let outcome = adapter
            .run(&release(&directory.id), &context(Uuid::now_v7()))
            .await;
        let (code, detail) = failure(&outcome);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(detail.contains("takes no image override"), "{detail}");
        assert_eq!(fake.syncs().len(), 2, "nothing was synced");

        // The deploy step of an sdi_gitops environment keeps its stage's
        // action type and runs as a sync when its operation says so.
        let deploy = |operation: &str| StepInput {
            action_type: "deploy.production".into(),
            ..step(
                GITOPS_SYNC,
                &[
                    ("application", fake::APP),
                    ("operation", operation),
                    ("image", "ghcr.io/acme/api"),
                    ("tag", "b2c3d4e"),
                ],
            )
        };
        let (output, outputs) = succeeded(
            &adapter
                .run(&deploy("gitops.sync"), &context(Uuid::now_v7()))
                .await,
        );
        assert!(output.starts_with("synced application api"), "{output}");
        assert_eq!(outputs["revision"], fake::REVISION);
        assert_eq!(
            fake.syncs().last().unwrap().1.kustomize_images,
            ["ghcr.io/acme/api:b2c3d4e"]
        );
        let (code, _) = failure(
            &adapter
                .run(&deploy("workflow.dispatch"), &context(Uuid::now_v7()))
                .await,
        );
        assert_eq!(code, diagnosis::UNKNOWN);
        assert_eq!(fake.syncs().len(), 3);
    }

    /// Run twice (a restart lost the first answer): the second run finds
    /// the history entry this execution wrote and syncs nothing again. One
    /// that is still running is waited for, not started a second time.
    #[tokio::test]
    async fn a_sync_this_execution_already_ran_is_not_run_again() {
        let (fake, adapter) = serve(FakeGitops::default().with_app(fake::kustomize_app())).await;
        let execution = Uuid::now_v7();
        let sync = step(
            GITOPS_SYNC,
            &[
                ("application", fake::APP),
                ("kustomize_images", r#"["ghcr.io/acme/api:1.4.0"]"#),
            ],
        );
        let first = adapter.run(&sync, &context(execution)).await;
        let (_, first_outputs) = succeeded(&first);
        let again = adapter.run(&sync, &context(execution)).await;
        let (output, outputs) = succeeded(&again);
        assert!(output.contains("already synced"), "{output}");
        assert!(output.contains("nothing was synced again"), "{output}");
        assert_eq!(outputs["history_id"], first_outputs["history_id"]);
        assert_eq!(outputs["revision"], fake::REVISION);
        assert_eq!(fake.syncs().len(), 1, "the engine was asked once");
        // Another image is another sync, even in the same execution.
        let other = step(
            GITOPS_SYNC,
            &[
                ("application", fake::APP),
                ("kustomize_images", r#"["ghcr.io/acme/api:1.5.0"]"#),
            ],
        );
        succeeded(&adapter.run(&other, &context(execution)).await);
        assert_eq!(fake.syncs().len(), 2);
        // Another execution syncs for itself.
        succeeded(&adapter.run(&sync, &context(Uuid::now_v7())).await);
        assert_eq!(fake.syncs().len(), 3);

        // An operation of this execution is still running: wait for it,
        // then take its result instead of syncing a second time.
        let waiting = Uuid::now_v7();
        fake.update(fake::APP, |app| {
            app.operation = Some(Operation {
                kind: "sync".into(),
                phase: "running".into(),
                execution_id: waiting.to_string(),
                ..Default::default()
            });
        });
        fake.finish_operation_after(3, waiting, fake::REVISION);
        let outcome = adapter
            .run(
                &step(GITOPS_SYNC, &[("application", fake::APP)]),
                &context(waiting),
            )
            .await;
        let (output, _) = succeeded(&outcome);
        assert!(output.contains("already synced"), "{output}");
        assert_eq!(fake.syncs().len(), 3, "nothing was started twice");
        assert!(fake.gets.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test]
    async fn a_rollback_goes_to_the_history_entry_and_is_safe_to_repeat() {
        let (fake, adapter) = serve(FakeGitops::default().with_app(fake::kustomize_app())).await;
        // Two syncs make two entries; roll back to the first.
        succeeded(
            &adapter
                .run(
                    &step(
                        GITOPS_SYNC,
                        &[
                            ("application", fake::APP),
                            ("kustomize_images", r#"["ghcr.io/acme/api:1.0.0"]"#),
                        ],
                    ),
                    &context(Uuid::now_v7()),
                )
                .await,
        );
        succeeded(
            &adapter
                .run(
                    &step(
                        GITOPS_SYNC,
                        &[
                            ("application", fake::APP),
                            ("kustomize_images", r#"["ghcr.io/acme/api:2.0.0"]"#),
                        ],
                    ),
                    &context(Uuid::now_v7()),
                )
                .await,
        );
        let target = fake.history(fake::APP)[1].id.clone();
        let execution = Uuid::now_v7();
        let rollback = step(
            GITOPS_ROLLBACK,
            &[
                ("application", fake::APP),
                ("history_id", &target),
                ("prune", "true"),
            ],
        );
        let outcome = adapter.run(&rollback, &context(execution)).await;
        let (output, outputs) = succeeded(&outcome);
        assert!(
            output.starts_with("rolled back application api"),
            "{output}"
        );
        let rollbacks = fake.rollbacks();
        assert_eq!(rollbacks.len(), 1);
        assert_eq!(rollbacks[0].1.history_id, target);
        assert!(rollbacks[0].1.prune);
        assert_eq!(rollbacks[0].1.execution_id, execution.to_string());
        assert_eq!(rollbacks[0].0.user_id, "user-approver");
        assert_eq!(
            fake.app(fake::APP).source.unwrap().kustomize_images,
            ["ghcr.io/acme/api:1.0.0"]
        );
        let entry = outputs["history_id"].clone();
        // Again: nothing is rolled back twice.
        let again = adapter.run(&rollback, &context(execution)).await;
        let (output, outputs) = succeeded(&again);
        assert!(output.contains("already rolled back"), "{output}");
        assert_eq!(outputs["history_id"], entry);
        assert_eq!(fake.rollbacks().len(), 1);

        let missing = adapter
            .run(
                &step(GITOPS_ROLLBACK, &[("application", fake::APP)]),
                &context(execution),
            )
            .await;
        let (code, detail) = failure(&missing);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(detail.contains("params.history_id"), "{detail}");
    }

    #[tokio::test]
    async fn verify_health_waits_for_healthy_and_fails_on_degraded_or_a_timeout() {
        let (fake, adapter) = serve(FakeGitops::default().with_app(fake::kustomize_app())).await;
        fake.health_sequence(&["progressing", "progressing", "healthy"]);
        let verify = step(GITOPS_VERIFY_HEALTH, &[("application", fake::APP)]);
        let outcome = adapter.run(&verify, &context(Uuid::now_v7())).await;
        let (output, outputs) = succeeded(&outcome);
        assert!(output.contains("is healthy"), "{output}");
        assert!(output.contains("after 3 polls"), "{output}");
        assert_eq!(outputs["health"], "healthy");
        // Every poll asked for a live comparison, never the snapshot.
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 3);
        assert_eq!(fake.gets.load(Ordering::SeqCst), 0);

        // A refresh that fails for a passing reason reads the snapshot for
        // that poll, and the next poll refreshes again.
        fake.health_sequence(&["progressing", "healthy"]);
        fake.fail_refresh(Status::unavailable("cluster cache is warming up"));
        let outcome = adapter.run(&verify, &context(Uuid::now_v7())).await;
        let (output, _) = succeeded(&outcome);
        assert!(output.contains("after 2 polls"), "{output}");
        assert_eq!(fake.gets.load(Ordering::SeqCst), 1, "one fallback read");
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 5);
        // One the engine refuses for good is the answer.
        let (code, detail) = failure(
            &adapter
                .run(
                    &step(
                        GITOPS_VERIFY_HEALTH,
                        &[("application", "0192f6a0-0000-7000-8000-00000000ffff")],
                    ),
                    &context(Uuid::now_v7()),
                )
                .await,
        );
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(detail.contains("no such application"), "{detail}");
        fake.fail_refresh(Status::permission_denied("requires the member role"));
        let (code, _) = failure(&adapter.run(&verify, &context(Uuid::now_v7())).await);
        assert_eq!(code, diagnosis::PERMISSION_DENIED);

        // Degraded twice in a row fails, naming what is not healthy.
        fake.health_sequence(&["progressing", "degraded", "degraded"]);
        fake.update(fake::APP, |app| {
            app.health_message = "1 resource is degraded".into();
            app.resources[0].health_status = "degraded".into();
            app.resources[0].health_message = "Deployment exceeded its progress deadline".into();
        });
        let outcome = adapter.run(&verify, &context(Uuid::now_v7())).await;
        let (code, detail) = failure(&outcome);
        assert_eq!(code, diagnosis::HEALTH_CHECK_FAILED);
        assert!(detail.contains("is degraded"), "{detail}");
        assert!(
            detail.contains(
                "not healthy: Deployment shop/api: degraded (Deployment exceeded its progress deadline)"
            ),
            "{detail}"
        );
        // Missing fails the same way.
        fake.health_sequence(&["missing", "missing"]);
        let (code, _) = failure(&adapter.run(&verify, &context(Uuid::now_v7())).await);
        assert_eq!(code, diagnosis::HEALTH_CHECK_FAILED);

        // Still progressing when the limit is up.
        fake.health_sequence(&["progressing"]);
        let slow = step(
            GITOPS_VERIFY_HEALTH,
            &[("application", fake::APP), ("timeout_seconds", "1")],
        );
        let (code, detail) = failure(&adapter.run(&slow, &context(Uuid::now_v7())).await);
        assert_eq!(code, diagnosis::HEALTH_CHECK_FAILED);
        assert!(
            detail.contains("was still progressing after 1s"),
            "{detail}"
        );

        // A cluster the engine cannot compare against is not a health verdict.
        fake.update(fake::APP, |app| {
            app.conditions = vec![
                "comparison failed: cluster prod is unreachable: dial tcp 10.0.0.5:6443: i/o timeout"
                    .into(),
            ];
        });
        let (code, detail) = failure(&adapter.run(&slow, &context(Uuid::now_v7())).await);
        assert_eq!(code, diagnosis::CLUSTER_UNREACHABLE);
        assert!(detail.contains("cluster prod is unreachable"), "{detail}");
    }

    /// Every failure code of §24.7, from what the engine answers.
    #[tokio::test]
    async fn engine_answers_map_to_the_failure_codes() {
        let failed_sync = SyncResult {
            phase: "failed".into(),
            message: "one or more objects failed to apply".into(),
            revision: fake::REVISION.into(),
            resources: vec![
                ResourceResult {
                    kind: "Deployment".into(),
                    group: "apps".into(),
                    namespace: "shop".into(),
                    name: "api".into(),
                    status: "failed".into(),
                    message: "admission webhook denied the request".into(),
                    ..Default::default()
                },
                ResourceResult {
                    kind: "Job".into(),
                    group: "batch".into(),
                    namespace: "shop".into(),
                    name: "migrate".into(),
                    status: "failed".into(),
                    hook_phase: "pre_sync".into(),
                    message: "BackoffLimitExceeded".into(),
                    ..Default::default()
                },
                ResourceResult {
                    kind: "Service".into(),
                    namespace: "shop".into(),
                    name: "api".into(),
                    status: "synced".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let cases: Vec<(Scripted, &str, &str)> = vec![
            (
                Scripted::Result(failed_sync),
                diagnosis::SYNC_FAILED,
                "2 resources failed: Deployment shop/api (admission webhook denied the request), Job shop/migrate (pre sync hook) (BackoffLimitExceeded)",
            ),
            (
                Scripted::Result(SyncResult {
                    phase: "error".into(),
                    message: "kustomize build apps/api: accumulating resources: missing file".into(),
                    ..Default::default()
                }),
                diagnosis::RENDER_FAILED,
                "its source did not render: kustomize build apps/api",
            ),
            (
                Scripted::Result(SyncResult {
                    phase: "error".into(),
                    message: "cluster prod is unreachable: dial tcp 10.0.0.5:6443: connect: connection refused".into(),
                    ..Default::default()
                }),
                diagnosis::CLUSTER_UNREACHABLE,
                "the cluster is out of reach: cluster prod is unreachable",
            ),
            (
                Scripted::Status(Status::failed_precondition(
                    "rendering the source failed: helm template: values.yaml: parse error",
                )),
                diagnosis::RENDER_FAILED,
                "does not render: rendering the source failed",
            ),
            (
                Scripted::Status(Status::failed_precondition(
                    "an operation is already running on application api",
                )),
                diagnosis::SYNC_FAILED,
                "another operation is running",
            ),
            (
                Scripted::Status(Status::unavailable("cluster prod cannot be reached")),
                diagnosis::CLUSTER_UNREACHABLE,
                "could not reach the GitOps engine at http://127.0.0.1:",
            ),
            (
                Scripted::Status(Status::permission_denied("requires the approver role")),
                diagnosis::PERMISSION_DENIED,
                "refused the sync of application",
            ),
        ];
        for (scripted, code, says) in cases {
            let (fake, adapter) =
                serve(FakeGitops::default().with_app(fake::kustomize_app())).await;
            fake.script(scripted);
            let outcome = adapter
                .run(
                    &step(GITOPS_SYNC, &[("application", fake::APP)]),
                    &context(Uuid::now_v7()),
                )
                .await;
            let (got, detail) = failure(&outcome);
            assert_eq!(got, code, "{detail}");
            assert!(detail.contains(says), "{code}: {detail}");
        }

        // The engine itself is not there.
        let adapter = GitopsEngineAdapter::new("http://127.0.0.1:1").unwrap();
        for action in [GITOPS_PREVIEW, GITOPS_SYNC, GITOPS_VERIFY_HEALTH] {
            let outcome = adapter
                .run(
                    &step(
                        action,
                        &[("application", fake::APP), ("timeout_seconds", "1")],
                    ),
                    &context(Uuid::now_v7()),
                )
                .await;
            let (code, detail) = failure(&outcome);
            assert_eq!(code, diagnosis::CLUSTER_UNREACHABLE, "{action}: {detail}");
            assert!(detail.contains("127.0.0.1:1"), "{detail}");
        }

        // A plan the adapter cannot make sense of.
        let (fake, adapter) = serve(FakeGitops::default().with_app(fake::kustomize_app())).await;
        for (params, says) in [
            (vec![], "params.application"),
            (
                vec![("application", fake::APP), ("prune", "yes")],
                "params.prune must be true or false",
            ),
            (
                vec![("application", fake::APP), ("kustomize_images", "api:1")],
                "params.kustomize_images must be a JSON array",
            ),
            (
                vec![("application", fake::APP), ("helm_parameters", "[1]")],
                "params.helm_parameters must be a JSON object",
            ),
            (
                vec![("application", fake::APP), ("timeout_seconds", "0")],
                "params.timeout_seconds",
            ),
            (
                vec![("application", "0192f6a0-0000-7000-8000-00000000ffff")],
                "no such application",
            ),
        ] {
            let (code, detail) = failure(
                &adapter
                    .run(&step(GITOPS_SYNC, &params), &context(Uuid::now_v7()))
                    .await,
            );
            assert_eq!(code, diagnosis::UNKNOWN, "{detail}");
            assert!(detail.contains(says), "{says}: {detail}");
        }
        assert!(fake.syncs().is_empty());
        // Without a principal there is nobody to act as.
        let (code, detail) = failure(
            &adapter
                .run(
                    &step(GITOPS_SYNC, &[("application", fake::APP)]),
                    &RunContext::default(),
                )
                .await,
        );
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(detail.contains("no principal"), "{detail}");
        let (code, _) = failure(
            &adapter
                .run(&step("deploy.staging", &[]), &context(Uuid::now_v7()))
                .await,
        );
        assert_eq!(code, diagnosis::UNKNOWN);
    }

    #[test]
    fn messages_are_classified_by_what_they_are_about() {
        assert_eq!(classify("an operation is already running"), Said::Busy);
        assert_eq!(
            classify("cluster prod is unreachable: dial tcp: i/o timeout"),
            Said::Unreachable
        );
        assert_eq!(
            classify("render: reading acme/fleet@main: dial tcp: i/o timeout"),
            Said::Render
        );
        assert_eq!(classify("helm template: parse error"), Said::Render);
        assert_eq!(classify("connection refused"), Said::Unreachable);
        assert_eq!(classify("quota"), Said::Other);
    }
}
