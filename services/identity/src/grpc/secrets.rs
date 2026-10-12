//! Workspace secrets (CONTRACT.md §22): values deployers reference by name.
//!
//! Unlike the rest of this service, these RPCs read the caller from the
//! gRPC metadata themselves: the three management calls need an ADMIN of
//! the workspace named in the request, and `ResolveSecret`, the only call
//! that returns a value, needs the OWNER role that internal callers carry
//! (`Principal::system`). An end user below owner can therefore never read
//! a value, whichever service forwards their metadata. Every resolve is
//! written to the audit trail with the secret's name, never its value.

use chrono::Utc;
use sdi_common::{
    grpc::db_status,
    principal::{Principal, Role},
};
use sdi_proto::identity::v1::{
    DeleteSecretRequest, DeleteSecretResponse, ListSecretsRequest, ListSecretsResponse,
    ResolveSecretRequest, ResolvedSecret, SecretInfo, SetSecretRequest,
};
use tonic::Status;
use uuid::Uuid;

use super::{IdentityState, parse_id};
use crate::{
    convert,
    db::{self, audit::AuditRow},
};

/// Longest secret name: one leading letter plus up to 63 more characters.
pub const MAX_NAME_LEN: usize = 64;
/// Largest value stored (a PEM key with its chain fits; a file does not).
pub const MAX_VALUE_BYTES: usize = 32 * 1024;
pub const MAX_DESCRIPTION_CHARS: usize = 500;
/// Secrets one workspace may hold.
pub const MAX_SECRETS_PER_WORKSPACE: i64 = 200;
/// Values shorter than this get no hint: four characters of a short value
/// would give away too much of it.
const MIN_LEN_FOR_HINT: usize = 12;

pub const ACTION_RESOLVE: &str = "secret.resolve";

/// `^[A-Z][A-Z0-9_]{1,63}$`: upper snake case, like an environment variable.
pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=MAX_NAME_LEN).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

fn check_name(name: &str) -> Result<&str, Status> {
    let name = name.trim();
    if valid_name(name) {
        Ok(name)
    } else {
        Err(Status::invalid_argument(
            "name must be upper snake case: a letter, then 1 to 63 letters, digits or underscores (e.g. JENKINS_TOKEN)",
        ))
    }
}

/// The last four characters, for recognition only.
pub fn hint_of(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < MIN_LEN_FOR_HINT {
        return String::new();
    }
    chars[chars.len() - 4..].iter().collect()
}

/// The workspace a request names, which must be the caller's own: metadata
/// is the authority, the body only says which workspace is meant.
fn workspace_of(principal: &Principal, raw: &str, min: Role) -> Result<Uuid, Status> {
    principal.require(min)?;
    let id = parse_id(raw, "workspace_id")?;
    if principal.workspace_id.trim() != id.to_string() {
        return Err(Status::permission_denied(
            "workspace_id does not match the caller's workspace",
        ));
    }
    Ok(id)
}

async fn ensure_workspace(state: &IdentityState, id: Uuid) -> Result<(), Status> {
    db::workspaces::get(&state.pool, id)
        .await
        .map(|_| ())
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => Status::not_found("no such workspace"),
            other => db_status(other),
        })
}

/// The login of the user behind `user_id` when it is one of ours, else the
/// id as given (`system` for internal callers).
async fn actor(state: &IdentityState, user_id: &str) -> String {
    let Ok(id) = Uuid::parse_str(user_id.trim()) else {
        return user_id.trim().to_owned();
    };
    match db::users::get(&state.pool, id).await {
        Ok(user) => user.login,
        Err(_) => user_id.trim().to_owned(),
    }
}

pub async fn list(
    state: &IdentityState,
    principal: &Principal,
    req: ListSecretsRequest,
) -> Result<ListSecretsResponse, Status> {
    let workspace = workspace_of(principal, &req.workspace_id, Role::Admin)?;
    let rows = db::secrets::list(&state.pool, workspace)
        .await
        .map_err(db_status)?;
    Ok(ListSecretsResponse {
        secrets: rows.into_iter().map(convert::secret_to_proto).collect(),
    })
}

pub async fn set(
    state: &IdentityState,
    principal: &Principal,
    req: SetSecretRequest,
) -> Result<SecretInfo, Status> {
    let workspace = workspace_of(principal, &req.workspace_id, Role::Admin)?;
    let name = check_name(&req.name)?;
    if req.value.is_empty() {
        return Err(Status::invalid_argument(
            "value must not be empty; delete the secret to remove it",
        ));
    }
    if req.value.len() > MAX_VALUE_BYTES {
        return Err(Status::invalid_argument(format!(
            "value must be at most {MAX_VALUE_BYTES} bytes"
        )));
    }
    let description = req.description.trim();
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(Status::invalid_argument(format!(
            "description must be at most {MAX_DESCRIPTION_CHARS} characters"
        )));
    }
    ensure_workspace(state, workspace).await?;
    let replacing = db::secrets::exists(&state.pool, workspace, name)
        .await
        .map_err(db_status)?;
    if !replacing
        && db::secrets::count(&state.pool, workspace)
            .await
            .map_err(db_status)?
            >= MAX_SECRETS_PER_WORKSPACE
    {
        return Err(Status::resource_exhausted(format!(
            "a workspace holds at most {MAX_SECRETS_PER_WORKSPACE} secrets"
        )));
    }
    let updated_by = actor(state, &principal.user_id).await;
    let row = db::secrets::upsert(
        &state.pool,
        workspace,
        name,
        &state.keys.seal(req.value.as_bytes()),
        &hint_of(&req.value),
        description,
        &updated_by,
    )
    .await
    .map_err(db_status)?;
    tracing::info!(%workspace, secret = name, replacing, "workspace secret stored");
    Ok(convert::secret_to_proto(row))
}

pub async fn delete(
    state: &IdentityState,
    principal: &Principal,
    req: DeleteSecretRequest,
) -> Result<DeleteSecretResponse, Status> {
    let workspace = workspace_of(principal, &req.workspace_id, Role::Admin)?;
    let name = check_name(&req.name)?;
    let removed = db::secrets::delete(&state.pool, workspace, name)
        .await
        .map_err(db_status)?;
    if !removed {
        return Err(Status::not_found(format!("no secret named {name}")));
    }
    tracing::info!(%workspace, secret = name, "workspace secret deleted");
    Ok(DeleteSecretResponse {})
}

/// Service-to-service only: the value in clear, for one request. The call
/// is audited before the value is returned, so a resolve that cannot be
/// recorded does not happen.
pub async fn resolve(
    state: &IdentityState,
    principal: &Principal,
    req: ResolveSecretRequest,
) -> Result<ResolvedSecret, Status> {
    let workspace = workspace_of(principal, &req.workspace_id, Role::Owner)?;
    let name = check_name(&req.name)?;
    let sealed = db::secrets::get_sealed(&state.pool, workspace, name)
        .await
        .map_err(db_status)?
        .ok_or_else(|| Status::not_found(format!("no secret named {name}")))?;
    let value = state.keys.open_str(&sealed).map_err(|error| {
        tracing::error!(%error, secret = name, "stored secret does not open");
        Status::internal("stored secret is unreadable")
    })?;
    db::audit::insert(
        &state.pool,
        &AuditRow {
            id: Uuid::now_v7(),
            workspace_id: workspace,
            user_id: principal.user_id.clone(),
            user_login: String::new(),
            action: ACTION_RESOLVE.to_owned(),
            entity: "secret".to_owned(),
            entity_id: name.to_owned(),
            detail: String::new(),
            request_id: String::new(),
            ip: String::new(),
            at: Utc::now(),
        },
    )
    .await
    .map_err(db_status)?;
    Ok(ResolvedSecret { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_upper_snake_case_of_two_to_sixty_four_characters() {
        for good in ["AB", "JENKINS_TOKEN", "A1", "X_", "REGISTRY_PASSWORD_2"] {
            assert!(valid_name(good), "{good}");
        }
        let longest = format!("A{}", "B".repeat(63));
        assert!(valid_name(&longest));
        for bad in [
            "",
            "A",
            "a_b",
            "Jenkins",
            "1TOKEN",
            "_TOKEN",
            "MY-TOKEN",
            "MY TOKEN",
            "TOKEN.",
            "ТОКЕН",
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(!valid_name(&format!("A{}", "B".repeat(64))));
    }

    #[test]
    fn hints_show_four_characters_of_long_values_only() {
        assert_eq!(hint_of("ghp_0123456789abcd"), "abcd");
        assert_eq!(hint_of("twelve_chars"), "hars");
        assert_eq!(hint_of("short"), "");
        assert_eq!(hint_of("elevenchars"), "");
        // Counted in characters, cut on character boundaries.
        assert_eq!(hint_of("пароль-пароль-ключ"), "ключ");
    }

    #[test]
    fn the_body_must_name_the_callers_workspace_and_the_role_must_suffice() {
        let ws = Uuid::now_v7();
        let admin = Principal {
            user_id: "u1".into(),
            workspace_id: ws.to_string(),
            role: Role::Admin,
        };
        assert_eq!(
            workspace_of(&admin, &ws.to_string(), Role::Admin).unwrap(),
            ws
        );
        assert_eq!(
            workspace_of(&admin, &Uuid::now_v7().to_string(), Role::Admin)
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            workspace_of(&admin, &ws.to_string(), Role::Owner)
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            workspace_of(&admin, "nope", Role::Admin)
                .unwrap_err()
                .code(),
            tonic::Code::InvalidArgument
        );
        let system = Principal::system(ws.to_string());
        assert!(workspace_of(&system, &ws.to_string(), Role::Owner).is_ok());
    }
}
