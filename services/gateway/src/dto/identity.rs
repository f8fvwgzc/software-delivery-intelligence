//! Identity area: sessions, workspaces, members, audit and data controls
//! (CONTRACT.md §18, §19).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sdi_proto::identity::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{opt_string, proto_enum, timestamp};

proto_enum! {
    /// Ordered: each role includes the ones below it (CONTRACT.md §18).
    Role => proto::Role { Viewer, Member, Approver, Admin, Owner }
}

proto_enum! {
    /// A user's own account, or a GitHub organisation with the app installed.
    WorkspaceKind => proto::WorkspaceKind { Personal, Organization }
}

/// A signed-in GitHub account.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// User id.
    pub id: String,
    /// GitHub numeric id.
    pub github_id: i64,
    /// GitHub login.
    pub login: String,
    /// Display name, when set on the profile.
    pub name: String,
    /// Avatar image URL.
    pub avatar_url: String,
    /// Primary verified email, when the OAuth grant allowed reading it.
    pub email: Option<String>,
    /// GitHub host the user signed in to (`github.com` or a GHES host).
    pub github_host: String,
    /// First sign-in.
    pub created_at: DateTime<Utc>,
}

impl From<proto::User> for User {
    fn from(user: proto::User) -> Self {
        Self {
            id: user.id,
            github_id: user.github_id,
            login: user.login,
            name: user.name,
            avatar_url: user.avatar_url,
            email: opt_string(user.email),
            github_host: user.github_host,
            created_at: timestamp(user.created_at.as_ref()),
        }
    }
}

/// One tenant: a personal account or an organisation installation.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    /// Workspace id; send it as `X-Sdi-Workspace` (or `?workspace=`).
    pub id: String,
    /// Personal or organisation.
    pub kind: WorkspaceKind,
    /// GitHub login of the org or user.
    pub slug: String,
    /// Display name.
    pub name: String,
    /// Avatar image URL.
    pub avatar_url: String,
    /// GitHub host.
    pub github_host: String,
    /// GitHub App installation id; null for personal workspaces.
    pub installation_id: Option<i64>,
    /// Repositories the installation grants: `all` or `selected`; null for personal workspaces.
    pub repository_selection: Option<String>,
    /// The caller's effective role.
    pub my_role: Role,
    /// Number of synced members (1 for personal workspaces).
    pub member_count: u32,
    /// Number of connected projects.
    pub project_count: u32,
    /// When the workspace was created.
    pub created_at: DateTime<Utc>,
}

impl From<proto::Workspace> for Workspace {
    fn from(ws: proto::Workspace) -> Self {
        Self {
            id: ws.id,
            kind: ws.kind.into(),
            slug: ws.slug,
            name: ws.name,
            avatar_url: ws.avatar_url,
            github_host: ws.github_host,
            installation_id: (ws.installation_id != 0).then_some(ws.installation_id),
            repository_selection: opt_string(ws.repository_selection),
            my_role: ws.my_role.into(),
            member_count: ws.member_count,
            project_count: ws.project_count,
            created_at: timestamp(ws.created_at.as_ref()),
        }
    }
}

/// Response of `GET /auth/me`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    /// The signed-in user.
    pub user: User,
    /// Every workspace the user can open, personal first.
    pub workspaces: Vec<Workspace>,
    /// The workspace this request was resolved in.
    pub current_workspace_id: String,
}

/// Query of `GET /auth/login`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct LoginQuery {
    /// Where to send the user after sign-in: a path, or a URL on the app origin.
    pub return_to: Option<String>,
}

/// Query of `GET /auth/callback` (what GitHub appends to the redirect).
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct CallbackQuery {
    /// Authorisation code.
    pub code: Option<String>,
    /// Signed state from `/auth/login`.
    pub state: Option<String>,
    /// Dev mode only: where to go afterwards.
    pub return_to: Option<String>,
    /// GitHub's own error code when the user cancelled.
    pub error: Option<String>,
}

/// A workspace member as synced from GitHub.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    /// The member.
    pub user: User,
    /// GitHub's view: `owner`, `member` or `outside_collaborator`.
    pub github_role: String,
    /// SDI teams the member is on (`sdi-admins`, `sdi-approvers`).
    pub teams: Vec<String>,
    /// Role implied by GitHub.
    pub derived_role: Role,
    /// Admin override, never above `derivedRole`; null when none.
    pub override_role: Option<Role>,
    /// What applies.
    pub effective_role: Role,
    /// Last sync from GitHub.
    pub synced_at: DateTime<Utc>,
}

impl From<proto::Member> for Member {
    fn from(member: proto::Member) -> Self {
        let override_role = Role::from(member.override_role);
        Self {
            user: member.user.unwrap_or_default().into(),
            github_role: member.github_role,
            teams: member.teams,
            derived_role: member.derived_role.into(),
            override_role: (override_role != Role::Unspecified).then_some(override_role),
            effective_role: member.effective_role.into(),
            synced_at: timestamp(member.synced_at.as_ref()),
        }
    }
}

/// Response of `GET /workspaces/{id}/members`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MembersResponse {
    /// Members, by login.
    pub members: Vec<Member>,
    /// When GitHub was last read; null before the first sync.
    pub synced_at: Option<DateTime<Utc>>,
}

/// Body of `PATCH /workspaces/{id}/members/{userId}`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemberRoleRequest {
    /// New override, strictly lower than the derived role; null clears it.
    pub override_role: Option<Role>,
}

/// One audited action.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    /// Entry id.
    pub id: String,
    /// Acting user id, or `system`.
    pub user_id: String,
    /// Acting user's login at the time.
    pub user_login: String,
    /// Dotted action key, e.g. `project.connect`, `approval.decide`, `data.export`.
    pub action: String,
    /// Entity type the action touched.
    pub entity: String,
    /// Entity id, when known.
    pub entity_id: String,
    /// Free-text detail.
    pub detail: String,
    /// `x-request-id` of the request.
    pub request_id: String,
    /// Client address as seen by the gateway.
    pub ip: String,
    /// When it happened.
    pub at: DateTime<Utc>,
}

impl From<proto::AuditEntry> for AuditEntry {
    fn from(entry: proto::AuditEntry) -> Self {
        Self {
            id: entry.id,
            user_id: entry.user_id,
            user_login: entry.user_login,
            action: entry.action,
            entity: entry.entity,
            entity_id: entry.entity_id,
            detail: entry.detail,
            request_id: entry.request_id,
            ip: entry.ip,
            at: timestamp(entry.at.as_ref()),
        }
    }
}

/// Query of `GET /people`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct PeopleQuery {
    /// Comma-separated user ids (at most 100), as rows carry them in
    /// `startedBy`, `createdBy`, `decidedBy` and `requestedBy`.
    pub ids: String,
}

/// The public face of a user, for naming who did something. Never carries
/// the email address.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// User id.
    pub id: String,
    /// GitHub login.
    pub login: String,
    /// Display name, when set on the profile.
    pub name: String,
    /// Avatar image URL.
    pub avatar_url: String,
    /// False for someone who acted in this workspace and has since left it.
    pub member: bool,
}

impl From<proto::UserRef> for Person {
    fn from(user: proto::UserRef) -> Self {
        Self {
            id: user.id,
            login: user.login,
            name: user.name,
            avatar_url: user.avatar_url,
            member: user.member,
        }
    }
}

/// Query of `GET /workspaces/{id}/audit`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct AuditQuery {
    /// Only actions starting with this, e.g. `execution.`.
    pub action_prefix: Option<String>,
    /// Cursor: entries strictly before this time.
    pub before: Option<DateTime<Utc>>,
    /// Page size (default 100, max 1000).
    pub limit: Option<u32>,
}

/// Per-workspace data handling (CONTRACT.md §19). Defaults are the most private.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataControls {
    /// `none` (never stored), `session` (per commit, purged after the TTL or on disconnect) or `persistent`.
    pub code_cache: String,
    /// Hours a `session` cache entry lives.
    pub code_cache_ttl_hours: u32,
    /// Semantic chunks keep only short excerpts when true.
    pub minimal_chunks: bool,
    /// `off` or `claude`. Only `claude` with a workspace key sends structured context to the model API.
    pub reasoner: String,
    /// True when a workspace-owned model key is stored.
    pub reasoner_key_configured: bool,
    /// Last characters of the stored key, for recognition.
    pub reasoner_key_hint: Option<String>,
    /// Days audit entries are kept; 0 keeps forever.
    pub audit_retention_days: u32,
    /// Last change.
    pub updated_at: DateTime<Utc>,
    /// Whether source lines may be sent to the model so it can write a fix (CONTRACT.md §19, §23): `off` (default) or `changed_lines` (the lines a finding points at plus a few around them, never a whole file, never a line a secrets rule matched). Only has an effect while `reasoner` is not `off`.
    pub code_to_model: String,
}

impl From<proto::DataControls> for DataControls {
    fn from(c: proto::DataControls) -> Self {
        Self {
            code_cache: c.code_cache,
            code_cache_ttl_hours: c.code_cache_ttl_hours,
            minimal_chunks: c.minimal_chunks,
            reasoner: c.reasoner,
            reasoner_key_configured: c.reasoner_key_configured,
            reasoner_key_hint: opt_string(c.reasoner_key_hint),
            audit_retention_days: c.audit_retention_days,
            updated_at: timestamp(c.updated_at.as_ref()),
            code_to_model: c.code_to_model,
        }
    }
}

/// Body of `PATCH /workspaces/{id}/data-controls`. Omitted fields keep their value.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDataControlsRequest {
    /// `none`, `session` or `persistent`.
    pub code_cache: Option<String>,
    /// 1–8760.
    pub code_cache_ttl_hours: Option<u32>,
    /// Keep only short excerpts in semantic chunks.
    pub minimal_chunks: Option<bool>,
    /// `off` or `claude`.
    pub reasoner: Option<String>,
    /// 0–3650; 0 keeps forever.
    pub audit_retention_days: Option<u32>,
    /// `off` or `changed_lines`.
    pub code_to_model: Option<String>,
}

impl UpdateDataControlsRequest {
    pub fn apply(self, current: proto::DataControls) -> proto::DataControls {
        proto::DataControls {
            code_cache: self.code_cache.unwrap_or(current.code_cache),
            code_cache_ttl_hours: self
                .code_cache_ttl_hours
                .unwrap_or(current.code_cache_ttl_hours),
            minimal_chunks: self.minimal_chunks.unwrap_or(current.minimal_chunks),
            reasoner: self.reasoner.unwrap_or(current.reasoner),
            audit_retention_days: self
                .audit_retention_days
                .unwrap_or(current.audit_retention_days),
            code_to_model: self.code_to_model.unwrap_or(current.code_to_model),
            ..current
        }
    }
}

/// Body of `PUT /workspaces/{id}/data-controls/reasoner-key`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetReasonerKeyRequest {
    /// The workspace's own model API key; empty removes it. Never returned.
    pub api_key: String,
}

/// Response of `GET /github/app/install-url`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstallUrl {
    /// Where to send an organisation owner to install the SDI GitHub App.
    pub url: String,
}

/// Query of `GET /github/app/setup` (GitHub's post-install redirect).
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "snake_case")]
#[into_params(parameter_in = Query)]
pub struct SetupQuery {
    /// The new installation.
    pub installation_id: i64,
    /// `install` or `update`.
    pub setup_action: Option<String>,
}

/// Response of `POST /webhooks/github`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookAck {
    /// The `X-GitHub-Event` that was delivered.
    pub event: String,
    /// What the gateway did with it: `recorded`, `removed`, `synced`, `scan_started`, `ignored`.
    pub outcome: String,
}

/// Response of `POST /workspaces/{id}/data/purge-code-cache`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PurgeResult {
    /// Cached file contents removed.
    pub files_deleted: u64,
    /// Cached repository trees removed.
    pub trees_deleted: u64,
}

/// Response of `DELETE /workspaces/{id}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDeleted {
    /// The deleted workspace.
    pub workspace_id: String,
    /// Rows removed per service and entity.
    pub deleted: BTreeMap<String, BTreeMap<String, u64>>,
}

/// Shape of the `POST /workspaces/{id}/data/export` body (documented; the
/// response is streamed as one JSON object of this form).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataExport {
    /// The workspace.
    pub workspace: Workspace,
    /// When the export was produced.
    pub exported_at: DateTime<Utc>,
    /// Members at export time.
    pub members: Vec<Member>,
    /// Data controls at export time.
    pub data_controls: Option<DataControls>,
    /// Names of the workspace's secrets. Values are never exported.
    pub secrets: Vec<String>,
    /// Knowledge rows by entity (projects, facts, graph, ...).
    pub knowledge: BTreeMap<String, Vec<serde_json::Value>>,
    /// Intelligence rows by entity (proposals, decisions, documents, diagrams, ...).
    pub intelligence: BTreeMap<String, Vec<serde_json::Value>>,
    /// Execution rows by entity (executions, steps, approvals, registries, environments, releases, deployment runs, ...).
    pub execution: BTreeMap<String, Vec<serde_json::Value>>,
    /// Every audit entry.
    pub audit: Vec<AuditEntry>,
    /// Services that could not contribute, by name.
    pub errors: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_serialise_lowercase_and_match_the_metadata_vocabulary() {
        for (role, text) in [
            (Role::Viewer, "viewer"),
            (Role::Member, "member"),
            (Role::Approver, "approver"),
            (Role::Admin, "admin"),
            (Role::Owner, "owner"),
        ] {
            assert_eq!(serde_json::to_string(&role).unwrap(), format!("\"{text}\""));
            assert!(sdi_common::principal::Role::parse(text).is_some());
        }
        assert_eq!(Role::from(proto::Role::Admin as i32), Role::Admin);
    }

    #[test]
    fn workspace_nulls_personal_installation_fields() {
        let ws = Workspace::from(proto::Workspace {
            id: "w1".into(),
            kind: proto::WorkspaceKind::Personal as i32,
            my_role: proto::Role::Owner as i32,
            ..Default::default()
        });
        assert_eq!(ws.installation_id, None);
        assert_eq!(ws.repository_selection, None);
        assert_eq!(ws.kind, WorkspaceKind::Personal);
        let json = serde_json::to_value(&ws).unwrap();
        assert_eq!(json["myRole"], "owner");
        assert_eq!(json["kind"], "personal");
    }

    #[test]
    fn member_override_is_null_when_unspecified() {
        let member = Member::from(proto::Member {
            derived_role: proto::Role::Admin as i32,
            effective_role: proto::Role::Admin as i32,
            ..Default::default()
        });
        assert_eq!(member.override_role, None);
        assert_eq!(member.effective_role, Role::Admin);
    }

    #[test]
    fn data_controls_patch_keeps_unset_fields() {
        let current = proto::DataControls {
            workspace_id: "w1".into(),
            code_cache: "session".into(),
            code_cache_ttl_hours: 24,
            reasoner: "off".into(),
            audit_retention_days: 365,
            ..Default::default()
        };
        let patched = UpdateDataControlsRequest {
            reasoner: Some("claude".into()),
            ..Default::default()
        }
        .apply(current);
        assert_eq!(patched.reasoner, "claude");
        assert_eq!(patched.code_cache, "session");
        assert_eq!(patched.audit_retention_days, 365);
        assert_eq!(patched.workspace_id, "w1");
    }

    #[test]
    fn code_to_model_is_patched_on_its_own_and_kept_by_other_patches() {
        let current = proto::DataControls {
            workspace_id: "w1".into(),
            code_cache: "session".into(),
            reasoner: "claude".into(),
            code_to_model: "off".into(),
            ..Default::default()
        };
        let on = UpdateDataControlsRequest {
            code_to_model: Some("changed_lines".into()),
            ..Default::default()
        }
        .apply(current);
        assert_eq!(on.code_to_model, "changed_lines");
        assert_eq!(on.reasoner, "claude");
        let other = UpdateDataControlsRequest {
            minimal_chunks: Some(true),
            ..Default::default()
        }
        .apply(on);
        assert_eq!(
            other.code_to_model, "changed_lines",
            "an unrelated patch keeps it"
        );
        assert_eq!(
            serde_json::to_value(DataControls::from(other)).unwrap()["codeToModel"],
            "changed_lines"
        );
    }
}
