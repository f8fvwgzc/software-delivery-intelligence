//! Rows -> proto messages.

use sdi_common::{principal::Role, time};
use sdi_proto::identity::v1::{
    AuditEntry, DataControls, Member, SecretInfo, User, UserRef, Workspace, WorkspaceKind,
};

use crate::{
    db::{
        audit::AuditRow,
        controls::ControlsRow,
        memberships::MemberRow,
        secrets::SecretRow,
        users::{UserRefRow, UserRow},
        workspaces::WorkspaceRow,
    },
    roles,
};

pub fn user_to_proto(row: UserRow) -> User {
    User {
        id: row.id.to_string(),
        github_id: row.github_id,
        login: row.login,
        name: row.name,
        avatar_url: row.avatar_url,
        email: row.email,
        github_host: row.github_host,
        created_at: Some(time::to_proto(row.created_at)),
    }
}

pub fn user_ref_to_proto(row: UserRefRow) -> UserRef {
    UserRef {
        id: row.id.to_string(),
        login: row.login,
        name: row.name,
        avatar_url: row.avatar_url,
        member: row.member,
    }
}

pub fn kind_from_db(text: &str) -> WorkspaceKind {
    WorkspaceKind::from_str_name(&format!("WORKSPACE_KIND_{text}")).unwrap_or_default()
}

/// `my_role` is the caller's effective role when known; `project_count` is
/// owned by the knowledge service and left for the gateway to fill.
pub fn workspace_to_proto(row: WorkspaceRow, my_role: Option<Role>) -> Workspace {
    let member_count = if row.kind == crate::db::workspaces::KIND_PERSONAL {
        1
    } else {
        u32::try_from(row.member_count.max(0)).unwrap_or(u32::MAX)
    };
    Workspace {
        id: row.id.to_string(),
        kind: kind_from_db(&row.kind) as i32,
        slug: row.slug,
        name: row.name,
        avatar_url: row.avatar_url,
        github_host: row.github_host,
        installation_id: row
            .installation_id
            .filter(|_| row.installation_removed_at.is_none())
            .unwrap_or(0),
        repository_selection: row.repository_selection.unwrap_or_default(),
        my_role: roles::opt_to_proto(my_role) as i32,
        member_count,
        project_count: 0,
        created_at: Some(time::to_proto(row.created_at)),
    }
}

pub fn member_to_proto(row: MemberRow) -> Member {
    let derived = roles::from_db(&row.membership.derived_role).unwrap_or(Role::Viewer);
    let override_role = row
        .membership
        .override_role
        .as_deref()
        .and_then(roles::from_db);
    Member {
        user: Some(user_to_proto(row.user)),
        github_role: row.membership.github_role,
        teams: row.membership.teams,
        derived_role: roles::to_proto(derived) as i32,
        override_role: roles::opt_to_proto(override_role) as i32,
        effective_role: roles::to_proto(roles::effective(derived, override_role)) as i32,
        synced_at: Some(time::to_proto(row.membership.synced_at)),
    }
}

/// The owner of a personal workspace, presented like a synced member.
pub fn owner_to_proto(user: UserRow, synced_at: chrono::DateTime<chrono::Utc>) -> Member {
    Member {
        user: Some(user_to_proto(user)),
        github_role: roles::GITHUB_OWNER.to_owned(),
        teams: Vec::new(),
        derived_role: roles::to_proto(Role::Owner) as i32,
        override_role: 0,
        effective_role: roles::to_proto(Role::Owner) as i32,
        synced_at: Some(time::to_proto(synced_at)),
    }
}

pub fn audit_to_proto(row: AuditRow) -> AuditEntry {
    AuditEntry {
        id: row.id.to_string(),
        workspace_id: row.workspace_id.to_string(),
        user_id: row.user_id,
        user_login: row.user_login,
        action: row.action,
        entity: row.entity,
        entity_id: row.entity_id,
        detail: row.detail,
        request_id: row.request_id,
        ip: row.ip,
        at: Some(time::to_proto(row.at)),
    }
}

pub fn controls_to_proto(row: ControlsRow) -> DataControls {
    let hint = row.key_hint.unwrap_or_default();
    DataControls {
        workspace_id: row.workspace_id.to_string(),
        code_cache: row.code_cache,
        code_cache_ttl_hours: u32::try_from(row.code_cache_ttl_hours.max(0)).unwrap_or(0),
        minimal_chunks: row.minimal_chunks,
        reasoner: row.reasoner,
        reasoner_key_configured: !hint.is_empty(),
        reasoner_key_hint: hint,
        audit_retention_days: u32::try_from(row.audit_retention_days.max(0)).unwrap_or(0),
        updated_at: Some(time::to_proto(row.updated_at)),
    }
}

/// A secret as the API shows it: never the value.
pub fn secret_to_proto(row: SecretRow) -> SecretInfo {
    SecretInfo {
        name: row.name,
        hint: row.hint,
        description: row.description,
        updated_by: row.updated_by,
        updated_at: Some(time::to_proto(row.updated_at)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_text_round_trips() {
        assert_eq!(kind_from_db("PERSONAL"), WorkspaceKind::Personal);
        assert_eq!(kind_from_db("ORGANIZATION"), WorkspaceKind::Organization);
        assert_eq!(kind_from_db("garbage"), WorkspaceKind::Unspecified);
    }
}
