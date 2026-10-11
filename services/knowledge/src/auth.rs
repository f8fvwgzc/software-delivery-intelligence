//! Who is calling (CONTRACT.md §18). The gateway forwards `x-sdi-user`,
//! `x-sdi-workspace` and `x-sdi-role` as gRPC metadata; [`interceptor`]
//! reads them on every RPC into a [`Caller`] extension and answers
//! `UNAUTHENTICATED` when they are missing, so no handler ever runs without
//! a workspace to scope its queries to.

use sdi_common::principal::{Principal, Role};
use tonic::{Request, Status};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub user_id: String,
    pub workspace_id: Uuid,
    pub role: Role,
}

impl Caller {
    /// A principal whose workspace is a uuid. Anything else cannot address a
    /// row and is refused as `UNAUTHENTICATED`, like a missing header.
    pub fn from_principal(principal: Principal) -> Result<Self, Status> {
        let workspace_id = Uuid::parse_str(principal.workspace_id.trim())
            .map_err(|_| Status::unauthenticated("workspace id is not a uuid"))?;
        Ok(Self {
            user_id: principal.user_id,
            workspace_id,
            role: principal.role,
        })
    }

    /// The caller the interceptor attached, or, for a service called without
    /// the interceptor (tests), the metadata read directly.
    pub fn from_request<T>(request: &Request<T>) -> Result<Self, Status> {
        if let Some(caller) = request.extensions().get::<Caller>() {
            return Ok(caller.clone());
        }
        Self::from_principal(Principal::from_request(request)?)
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

    /// Internal work in one workspace (startup jobs, background sweeps).
    pub fn system(workspace_id: Uuid) -> Self {
        Self {
            user_id: sdi_common::principal::SYSTEM_USER.to_owned(),
            workspace_id,
            role: Role::Owner,
        }
    }

    /// The same identity as metadata for an outgoing service call.
    pub fn principal(&self) -> Principal {
        Principal {
            user_id: self.user_id.clone(),
            workspace_id: self.workspace_id.to_string(),
            role: self.role,
        }
    }
}

/// tonic interceptor: every RPC carries a caller or is `UNAUTHENTICATED`.
pub fn interceptor(mut request: Request<()>) -> Result<Request<()>, Status> {
    let caller = Caller::from_principal(Principal::from_request(&request)?)?;
    request.extensions_mut().insert(caller);
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(user: &str, workspace: &str, role: &str) -> Request<()> {
        let mut request = Request::new(());
        let meta = request.metadata_mut();
        meta.insert("x-sdi-user", user.parse().unwrap());
        meta.insert("x-sdi-workspace", workspace.parse().unwrap());
        meta.insert("x-sdi-role", role.parse().unwrap());
        request
    }

    #[test]
    fn interceptor_rejects_missing_metadata() {
        let error = interceptor(Request::new(())).unwrap_err();
        assert_eq!(error.code(), tonic::Code::Unauthenticated);

        let mut partial = Request::new(());
        partial
            .metadata_mut()
            .insert("x-sdi-user", "u1".parse().unwrap());
        assert_eq!(
            interceptor(partial).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );

        let bad_workspace = request("u1", "not-a-uuid", "member");
        assert_eq!(
            interceptor(bad_workspace).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn interceptor_attaches_the_caller_and_roles_are_ordered() {
        let workspace = Uuid::now_v7();
        let intercepted = interceptor(request("u1", &workspace.to_string(), "approver")).unwrap();
        let caller = Caller::from_request(&intercepted).unwrap();
        assert_eq!(caller.user_id, "u1");
        assert_eq!(caller.workspace_id, workspace);
        assert_eq!(caller.role, Role::Approver);
        assert!(caller.require(Role::Viewer).is_ok());
        assert!(caller.require(Role::Member).is_ok());
        assert_eq!(
            caller.require(Role::Admin).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );

        // Without the interceptor the metadata is read directly.
        let direct =
            Caller::from_request(&request("u2", &workspace.to_string(), "viewer")).unwrap();
        assert_eq!(direct.role, Role::Viewer);
        assert_eq!(
            Caller::from_request(&Request::new(())).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );

        let mut outgoing = Request::new(());
        caller.principal().apply(&mut outgoing);
        assert_eq!(Caller::from_request(&outgoing).unwrap(), caller);
    }
}
