//! Membership: synced from GitHub, narrowed by admins (CONTRACT.md §18).

use chrono::Utc;
use sdi_common::{grpc::db_status, principal::Role, time};
use sdi_proto::identity::v1::{
    ListMembersRequest, ListMembersResponse, Member, ResolveUsersRequest, ResolveUsersResponse,
    SetMemberRoleRequest, SyncMembersRequest,
};
use tonic::Status;
use uuid::Uuid;

use super::{IdentityState, github_status, parse_id};
use crate::{
    convert,
    db::{
        self,
        memberships::MemberRow,
        workspaces::{KIND_PERSONAL, WorkspaceRow},
    },
    github::OrgMember,
    roles,
};

async fn members_of(
    state: &IdentityState,
    row: &WorkspaceRow,
) -> Result<ListMembersResponse, Status> {
    if row.kind == KIND_PERSONAL {
        let Some(owner) = row.owner_user_id else {
            return Ok(ListMembersResponse::default());
        };
        let user = db::users::get(&state.pool, owner)
            .await
            .map_err(db_status)?;
        return Ok(ListMembersResponse {
            members: vec![convert::owner_to_proto(user, row.created_at)],
            synced_at: Some(time::to_proto(row.created_at)),
        });
    }
    let rows = db::memberships::list(&state.pool, row.id)
        .await
        .map_err(db_status)?;
    let synced_at = db::memberships::last_synced(&state.pool, row.id)
        .await
        .map_err(db_status)?;
    Ok(ListMembersResponse {
        members: rows.into_iter().map(convert::member_to_proto).collect(),
        synced_at: time::opt_to_proto(synced_at),
    })
}

pub async fn list(
    state: &IdentityState,
    req: ListMembersRequest,
) -> Result<ListMembersResponse, Status> {
    let id = parse_id(&req.workspace_id, "workspace_id")?;
    let row = db::workspaces::get(&state.pool, id)
        .await
        .map_err(db_status)?;
    members_of(state, &row).await
}

/// Ids one call names. A page of rows never carries more people than this.
pub const RESOLVE_LIMIT: usize = 100;

/// Names for stored actor ids, scoped to the workspace (§18). Values that
/// are not user ids (a service name, a free-text approver) are skipped, and
/// ids the workspace may not name are simply absent from the answer.
pub async fn resolve(
    state: &IdentityState,
    req: ResolveUsersRequest,
) -> Result<ResolveUsersResponse, Status> {
    let workspace_id = parse_id(&req.workspace_id, "workspace_id")?;
    if req.user_ids.len() > RESOLVE_LIMIT {
        return Err(Status::invalid_argument(format!(
            "at most {RESOLVE_LIMIT} user ids per call (got {})",
            req.user_ids.len()
        )));
    }
    let mut ids: Vec<Uuid> = req
        .user_ids
        .iter()
        .filter_map(|id| Uuid::parse_str(id.trim()).ok())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(ResolveUsersResponse::default());
    }
    let rows = db::users::resolve(&state.pool, workspace_id, &ids)
        .await
        .map_err(db_status)?;
    Ok(ResolveUsersResponse {
        users: rows.into_iter().map(convert::user_ref_to_proto).collect(),
    })
}

pub async fn set_role(state: &IdentityState, req: SetMemberRoleRequest) -> Result<Member, Status> {
    let workspace_id = parse_id(&req.workspace_id, "workspace_id")?;
    let user_id = parse_id(&req.user_id, "user_id")?;
    let workspace = db::workspaces::get(&state.pool, workspace_id)
        .await
        .map_err(db_status)?;
    if workspace.kind == KIND_PERSONAL {
        return Err(Status::failed_precondition(
            "a personal workspace has one owner and no roles to narrow",
        ));
    }
    let member = db::memberships::get(&state.pool, workspace_id, user_id)
        .await
        .map_err(db_status)?
        .ok_or_else(|| Status::not_found("no such member"))?;
    let derived = roles::from_db(&member.membership.derived_role).unwrap_or(Role::Viewer);
    let override_role = roles::narrow(derived, roles::from_proto(req.override_role))
        .map_err(Status::invalid_argument)?;
    db::memberships::set_override(
        &state.pool,
        workspace_id,
        user_id,
        override_role.map(roles::to_db),
    )
    .await
    .map_err(db_status)?;
    let updated = db::memberships::get(&state.pool, workspace_id, user_id)
        .await
        .map_err(db_status)?
        .ok_or_else(|| Status::not_found("no such member"))?;
    Ok(convert::member_to_proto(updated))
}

pub async fn sync(
    state: &IdentityState,
    req: SyncMembersRequest,
) -> Result<ListMembersResponse, Status> {
    let id = parse_id(&req.workspace_id, "workspace_id")?;
    let row = db::workspaces::get(&state.pool, id)
        .await
        .map_err(db_status)?;
    if row.kind != KIND_PERSONAL {
        sync_from_github(state, &row).await?;
    }
    members_of(state, &row).await
}

/// Stores one member as GitHub reports them and returns the row.
async fn store(
    state: &IdentityState,
    workspace: &WorkspaceRow,
    member: &OrgMember,
) -> Result<MemberRow, Status> {
    let user = db::users::upsert(
        &state.pool,
        db::users::Profile {
            github_host: &workspace.github_host,
            github_id: member.user.id,
            login: &member.user.login,
            name: member.user.name.as_deref().unwrap_or_default(),
            avatar_url: member.user.avatar_url.as_deref().unwrap_or_default(),
            email: None,
        },
    )
    .await
    .map_err(db_status)?;
    let derived = roles::derive(&member.github_role, &member.teams);
    db::memberships::upsert(
        &state.pool,
        workspace.id,
        user.id,
        &member.github_role,
        &member.teams,
        roles::to_db(derived),
    )
    .await
    .map_err(db_status)?;
    db::memberships::get(&state.pool, workspace.id, user.id)
        .await
        .map_err(db_status)?
        .ok_or_else(|| Status::internal("membership vanished"))
}

/// Full re-read of the organisation: members, owners, outside
/// collaborators and the two SDI teams. Whoever GitHub no longer lists
/// loses access at the next request.
pub(crate) async fn sync_from_github(
    state: &IdentityState,
    workspace: &WorkspaceRow,
) -> Result<(), Status> {
    let token = super::workspaces::token_for(state, workspace).await?;
    let members = state
        .github
        .org_members(&token.api_base_url, &token.token, &workspace.slug)
        .await
        .map_err(github_status)?;
    let mut keep: Vec<Uuid> = Vec::with_capacity(members.len());
    for member in &members {
        let row = store(state, workspace, member).await?;
        keep.push(row.user.id);
    }
    let dropped = db::memberships::retain(&state.pool, workspace.id, &keep)
        .await
        .map_err(db_status)?;
    tracing::info!(workspace = %workspace.slug, members = members.len(), dropped, "members synced at {}", Utc::now());
    Ok(())
}

/// One user's membership read from GitHub when the stored rows do not know
/// them yet (§18: synced lazily when a user opens a workspace). `None`
/// means GitHub grants them nothing.
pub(crate) async fn sync_one(
    state: &IdentityState,
    workspace: &WorkspaceRow,
    user: &db::users::UserRow,
) -> Result<Option<MemberRow>, Status> {
    let token = super::workspaces::token_for(state, workspace).await?;
    let member = state
        .github
        .org_member(
            &token.api_base_url,
            &token.token,
            &workspace.slug,
            &user.login,
        )
        .await
        .map_err(github_status)?;
    match member {
        Some(member) => store(state, workspace, &member).await.map(Some),
        None => Ok(None),
    }
}
