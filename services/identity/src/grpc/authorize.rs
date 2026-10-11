//! Session + workspace -> principal. Called by the gateway once per request
//! (it caches for 60 s); nothing is cached here.

use sdi_common::{grpc::db_status, principal::Role};
use sdi_proto::identity::v1::{AuthorizeRequest, Principal};
use tonic::Status;
use uuid::Uuid;

use super::{IdentityState, sessions, stored_role};
use crate::{
    convert,
    db::{self, workspaces::KIND_PERSONAL},
    roles,
};

pub async fn authorize(state: &IdentityState, req: AuthorizeRequest) -> Result<Principal, Status> {
    let (_session, user) = sessions::resolve(state, &req.session_token).await?;

    let workspace = if req.workspace_id.trim().is_empty() {
        // No workspace named: the user's first (personal, then oldest org).
        db::workspaces::list_for_user(&state.pool, user.id)
            .await
            .map_err(db_status)?
            .into_iter()
            .next()
            .ok_or_else(|| Status::permission_denied("no workspace"))?
    } else {
        let id = Uuid::parse_str(req.workspace_id.trim())
            .map_err(|_| Status::permission_denied("no such workspace"))?;
        match db::workspaces::get(&state.pool, id).await {
            Ok(row) => row,
            // Existence is not revealed to someone without access.
            Err(sqlx::Error::RowNotFound) => {
                return Err(Status::permission_denied("no such workspace"));
            }
            Err(error) => return Err(db_status(error)),
        }
    };

    let role = match stored_role(state, &workspace, user.id).await? {
        Some(role) => role,
        None if workspace.kind == KIND_PERSONAL => {
            return Err(Status::permission_denied("not your workspace"));
        }
        None => {
            // Not synced yet: ask GitHub for this one user.
            let member = super::members::sync_one(state, &workspace, &user).await?;
            match member {
                Some(row) => {
                    let derived =
                        roles::from_db(&row.membership.derived_role).unwrap_or(Role::Viewer);
                    roles::effective(derived, None)
                }
                None => {
                    return Err(Status::permission_denied(
                        "you are not a member of this organisation",
                    ));
                }
            }
        }
    };

    Ok(Principal {
        user: Some(convert::user_to_proto(user)),
        workspace: Some(convert::workspace_to_proto(workspace, Some(role))),
        role: roles::to_proto(role) as i32,
    })
}
