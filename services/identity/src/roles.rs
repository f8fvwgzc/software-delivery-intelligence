//! Role derivation from GitHub membership (CONTRACT.md §18) and the
//! narrowing rule for admin overrides. Pure functions; the gRPC layer and
//! the sync job both go through here.

use sdi_common::principal::Role;
use sdi_proto::identity::v1::Role as ProtoRole;

pub const TEAM_ADMINS: &str = "sdi-admins";
pub const TEAM_APPROVERS: &str = "sdi-approvers";

pub const GITHUB_OWNER: &str = "owner";
pub const GITHUB_MEMBER: &str = "member";
pub const GITHUB_OUTSIDE_COLLABORATOR: &str = "outside_collaborator";

/// owner → OWNER; team `sdi-admins` → ADMIN; `sdi-approvers` → APPROVER;
/// other members → MEMBER; outside collaborators (and anything else) → VIEWER.
pub fn derive(github_role: &str, teams: &[String]) -> Role {
    if github_role == GITHUB_OWNER {
        return Role::Owner;
    }
    let in_team = |name: &str| teams.iter().any(|team| team.eq_ignore_ascii_case(name));
    if github_role == GITHUB_MEMBER {
        if in_team(TEAM_ADMINS) {
            Role::Admin
        } else if in_team(TEAM_APPROVERS) {
            Role::Approver
        } else {
            Role::Member
        }
    } else {
        Role::Viewer
    }
}

/// An override may only narrow: `None` clears it, anything at or above the
/// derived role is refused.
pub fn narrow(derived: Role, override_role: Option<Role>) -> Result<Option<Role>, String> {
    match override_role {
        None => Ok(None),
        Some(role) if role < derived => Ok(Some(role)),
        Some(role) => Err(format!(
            "override {} is not lower than the derived role {}",
            role.as_str(),
            derived.as_str()
        )),
    }
}

pub fn effective(derived: Role, override_role: Option<Role>) -> Role {
    override_role.map_or(derived, |o| o.min(derived))
}

/// Column text: the proto name without its prefix ("OWNER").
pub fn to_db(role: Role) -> &'static str {
    match role {
        Role::Viewer => "VIEWER",
        Role::Member => "MEMBER",
        Role::Approver => "APPROVER",
        Role::Admin => "ADMIN",
        Role::Owner => "OWNER",
    }
}

pub fn from_db(text: &str) -> Option<Role> {
    Role::parse(&text.to_ascii_lowercase())
}

pub fn to_proto(role: Role) -> ProtoRole {
    match role {
        Role::Viewer => ProtoRole::Viewer,
        Role::Member => ProtoRole::Member,
        Role::Approver => ProtoRole::Approver,
        Role::Admin => ProtoRole::Admin,
        Role::Owner => ProtoRole::Owner,
    }
}

pub fn opt_to_proto(role: Option<Role>) -> ProtoRole {
    role.map_or(ProtoRole::Unspecified, to_proto)
}

/// `UNSPECIFIED` (and unknown numbers) become `None`.
pub fn from_proto(raw: i32) -> Option<Role> {
    match ProtoRole::try_from(raw).unwrap_or_default() {
        ProtoRole::Unspecified => None,
        ProtoRole::Viewer => Some(Role::Viewer),
        ProtoRole::Member => Some(Role::Member),
        ProtoRole::Approver => Some(Role::Approver),
        ProtoRole::Admin => Some(Role::Admin),
        ProtoRole::Owner => Some(Role::Owner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn teams(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn roles_follow_the_contract_table() {
        assert_eq!(derive(GITHUB_OWNER, &[]), Role::Owner);
        // An owner stays an owner whatever teams they are on.
        assert_eq!(
            derive(GITHUB_OWNER, &teams(&["sdi-approvers"])),
            Role::Owner
        );
        assert_eq!(derive(GITHUB_MEMBER, &teams(&["sdi-admins"])), Role::Admin);
        assert_eq!(derive(GITHUB_MEMBER, &teams(&["SDI-Admins"])), Role::Admin);
        assert_eq!(
            derive(GITHUB_MEMBER, &teams(&["sdi-approvers", "sdi-admins"])),
            Role::Admin
        );
        assert_eq!(
            derive(GITHUB_MEMBER, &teams(&["platform", "sdi-approvers"])),
            Role::Approver
        );
        assert_eq!(derive(GITHUB_MEMBER, &teams(&["platform"])), Role::Member);
        assert_eq!(derive(GITHUB_MEMBER, &[]), Role::Member);
        assert_eq!(derive(GITHUB_OUTSIDE_COLLABORATOR, &[]), Role::Viewer);
        // Teams never lift an outside collaborator.
        assert_eq!(
            derive(GITHUB_OUTSIDE_COLLABORATOR, &teams(&["sdi-admins"])),
            Role::Viewer
        );
        assert_eq!(derive("something-else", &[]), Role::Viewer);
    }

    #[test]
    fn overrides_only_narrow() {
        assert_eq!(narrow(Role::Admin, None).unwrap(), None);
        assert_eq!(
            narrow(Role::Admin, Some(Role::Viewer)).unwrap(),
            Some(Role::Viewer)
        );
        assert_eq!(
            narrow(Role::Admin, Some(Role::Approver)).unwrap(),
            Some(Role::Approver)
        );
        assert!(narrow(Role::Admin, Some(Role::Admin)).is_err());
        assert!(narrow(Role::Member, Some(Role::Owner)).is_err());
        assert!(narrow(Role::Viewer, Some(Role::Viewer)).is_err());

        assert_eq!(effective(Role::Admin, Some(Role::Member)), Role::Member);
        assert_eq!(effective(Role::Admin, None), Role::Admin);
        // A stale override above the (lowered) derived role never applies.
        assert_eq!(effective(Role::Member, Some(Role::Admin)), Role::Member);
    }

    #[test]
    fn db_and_proto_text_round_trip() {
        for role in [
            Role::Viewer,
            Role::Member,
            Role::Approver,
            Role::Admin,
            Role::Owner,
        ] {
            assert_eq!(from_db(to_db(role)), Some(role));
            assert_eq!(from_proto(to_proto(role) as i32), Some(role));
        }
        assert_eq!(from_db("garbage"), None);
        assert_eq!(from_proto(0), None);
        assert_eq!(from_proto(99), None);
        assert_eq!(opt_to_proto(None), ProtoRole::Unspecified);
    }
}
