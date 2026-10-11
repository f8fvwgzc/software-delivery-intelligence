//! The caller's identity on every gRPC call (CONTRACT.md §18).
//!
//! The gateway authenticates once and forwards `x-sdi-user`,
//! `x-sdi-workspace` and `x-sdi-role` as metadata. Services never trust a
//! workspace id from a request body: they read it from here and pass it to
//! every repository function.

use tonic::{Request, Status, metadata::MetadataMap};

pub const USER_HEADER: &str = "x-sdi-user";
pub const WORKSPACE_HEADER: &str = "x-sdi-workspace";
pub const ROLE_HEADER: &str = "x-sdi-role";

/// Internal jobs (webhook rescans, startup seeds) run as this user.
pub const SYSTEM_USER: &str = "system";

/// Ordered: each role includes the ones below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Viewer,
    Member,
    Approver,
    Admin,
    Owner,
}

impl Role {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "viewer" => Self::Viewer,
            "member" => Self::Member,
            "approver" => Self::Approver,
            "admin" => Self::Admin,
            "owner" => Self::Owner,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Member => "member",
            Self::Approver => "approver",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub user_id: String,
    pub workspace_id: String,
    pub role: Role,
}

impl Principal {
    /// Reads the three headers; missing or malformed metadata is
    /// `UNAUTHENTICATED`, because a call without a workspace cannot be scoped.
    pub fn from_metadata(meta: &MetadataMap) -> Result<Self, Status> {
        let get = |key: &str| {
            meta.get(key)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| Status::unauthenticated(format!("missing {key}")))
        };
        let role = get(ROLE_HEADER)?;
        Ok(Self {
            user_id: get(USER_HEADER)?,
            workspace_id: get(WORKSPACE_HEADER)?,
            role: Role::parse(&role).ok_or_else(|| Status::unauthenticated("bad role"))?,
        })
    }

    pub fn from_request<T>(req: &Request<T>) -> Result<Self, Status> {
        Self::from_metadata(req.metadata())
    }

    /// `PERMISSION_DENIED` unless the caller holds at least `min`.
    pub fn require(&self, min: Role) -> Result<(), Status> {
        if self.role >= min {
            Ok(())
        } else {
            Err(Status::permission_denied(format!(
                "requires the {} role",
                min.as_str()
            )))
        }
    }

    /// A principal for internal work in one workspace.
    pub fn system(workspace_id: impl Into<String>) -> Self {
        Self {
            user_id: SYSTEM_USER.to_owned(),
            workspace_id: workspace_id.into(),
            role: Role::Owner,
        }
    }

    /// Copies the three headers onto an outgoing service-to-service request.
    pub fn apply<T>(&self, req: &mut Request<T>) {
        let meta = req.metadata_mut();
        for (key, value) in [
            (USER_HEADER, self.user_id.as_str()),
            (WORKSPACE_HEADER, self.workspace_id.as_str()),
            (ROLE_HEADER, self.role.as_str()),
        ] {
            if let Ok(v) = value.parse() {
                meta.insert(key, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_metadata() {
        let p = Principal {
            user_id: "u1".into(),
            workspace_id: "w1".into(),
            role: Role::Approver,
        };
        let mut req = Request::new(());
        p.apply(&mut req);
        assert_eq!(Principal::from_request(&req).unwrap(), p);
        assert!(p.require(Role::Member).is_ok());
        assert!(p.require(Role::Admin).is_err());
    }

    #[test]
    fn missing_workspace_is_unauthenticated() {
        let req = Request::new(());
        assert_eq!(
            Principal::from_request(&req).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );
    }
}
