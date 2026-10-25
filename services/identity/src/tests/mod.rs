//! In-process tests against a throwaway schema in the dev Postgres and a
//! fake GitHub API on a random port. They skip (and say so) when
//! DATABASE_URL is unset.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use sdi_common::principal::Role;
use sdi_proto::identity::v1::{
    AuditEntry, AuthorizeRequest, BeginLoginRequest, CompleteLoginRequest, DataControls,
    GetDataControlsRequest, GetInstallationTokenRequest, GetSessionRequest, ListAuditRequest,
    ListMembersRequest, ListWorkspacesRequest, RecordAuditRequest, RecordInstallationRequest,
    RemoveInstallationRequest, ResolveUsersRequest, RevokeSessionRequest, Role as ProtoRole,
    SetMemberRoleRequest, SetReasonerKeyRequest, SyncMembersRequest, UpdateDataControlsRequest,
    WorkspaceKind,
};
use serde_json::{Value, json};
use tonic::Code;
use uuid::Uuid;

use crate::{
    config::{GithubApp, Settings},
    grpc::{IdentityState, audit, authorize, controls, members, sessions, workspaces},
    roles,
};

/// Throwaway RSA key for the App JWT path (never used anywhere else).
const TEST_APP_KEY: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDo9KXwzc+px+6h\n\
qSYc/OCJ5xRsT2DKdcS6K3+1zzAsD0Nxqtf/2ieMQD5l1kOCuTrpaX/6wDLFlzHN\n\
OK4dZ5+MyvjI0TX8qJqHvddQK+X2+FeSES4y4ezuK0zRP5Ags6AY/9ofkhA0nG4l\n\
PL2OoDWRk69UoLjSDUPdTJJESfZqGcayMAwMDnDa556vkc0aq1pbQJV5ZataSUCC\n\
vJhiTc25angILYsZhrLPmNugszddrIhEUboR50Wc8MC7blp8yxSqeVs9slm0F1MZ\n\
Rd4LJQ/8IIO2mugK4eYoMBylmO1HPK6nJg1ux9gz8scF8hLvTxZWyirBGqCUSMqG\n\
k0Tbu/8rAgMBAAECggEADrtd/R7+2XVP1YBdbSrexknLRFg2vkb5VG6oymYaXUbv\n\
1HHh7AevRZLQf18v8MXnAFjQI5rg+xbnKtvBQxHJCL3TQtvcJqjA8QGj6qmEBvzI\n\
iO+2NLiN47fwSTFUW4UQhFQ1xpZVmQYlfwxazni6OoUU6uypE3uMRaPwggoNX8M3\n\
pBvBR3/3P4utmfdmEozlCktHCCOFLNkCbJf5+kYpjQjYeFTukXgyL91WAPz8pdLU\n\
gJu6NPn0xFbOzWe+q3zq4WrWxlWE3WKGUTmzcFsGXArQCqUZR6PcKd8gJMz8r3Rb\n\
LVktzLNKCm5BUgHQQYzOjVCKm5zHLNHV7SS8C5mwQQKBgQD60sPt51UBKh8XqZgm\n\
+VfQpihCkPIBKTCISqloCStAOT4TtMIHVYnDTKeO6Ixq+9QxskOtHOLcOFk1zYhv\n\
LyZHuCQ98nqo33OFXxhDyOfq/cKa8nK/AKvBoRyvfPmp0UsBHCCcQzTE79KEPzCE\n\
sPRFhACxZtd3WU62YeJnpWbvawKBgQDtw3p8YvjTkdd/SxghHVgiU3M6JNu5vBhs\n\
wycyjm7BT/gqj65qkfBq88rbWpqDcffR7R5WirQRC6vi/4e3pn4UbE/aK+nhgp4z\n\
ecCaCgAyTPTtpgk4CO/E/PlrvhgVclvhgjpmRofxKADGNCvAIVHSc+eXTQUgq107\n\
pUh+j+HfQQKBgAsoAsT7LCHAIN/pkDXzkz9ZlKLZVZpNnbYCNCTHe6AAq80ulg7D\n\
dDw2laFaoJxFlTucpTv28/Q9oWIuQSpI25gHDb2fomsOfVHqV+bz8bTXEqfXrtjm\n\
C72yD9eSZHx0LbTf7mdk6dz0mdV+Bgdw43yQEYPSgjTxyPl9wxc/GSP/AoGBAOXj\n\
KZz2/Otjid8UNoJeIwcddL31QVX2Xg9bKZ0iRkg3EBXm0pdMAA8JJUyt79PD7/9T\n\
qw+2EKCijlFvDEQvgJwylYq3Bu36IwhWYjkODl6UNgVJkoRRb7MhffAE3rht2BTU\n\
m66j53DuYsFBVEPyc3BujK+n0pth1UWEa/GjIJuBAoGBALL+17aGO/TWpQ3OTb9Y\n\
aBfMEoe6BN1fQ+ca2APZppcjRwTIGU7Serkh2pjKgIaztP39mWkmdI78TEYWTeW+\n\
DpHFN2q6no1mk6ULq4CZq61sIMYgC0YlYKWqDDhP9/oTMNg4ELpeASHSpuPP3jkO\n\
/PyJ+B8WO0mSYBiwlCtL6xZ8\n\
-----END PRIVATE KEY-----\n\
";

const DEV_TOKEN: &str = "gho_dev_token";

struct Harness {
    state: Arc<IdentityState>,
    schema: String,
}

fn user(id: i64, login: &str) -> Value {
    json!({"id": id, "login": login, "name": format!("{login} name"), "avatar_url": format!("https://avatars/{login}"), "email": null})
}

/// The slice of GitHub the service talks to. `acme` is an organisation
/// with an owner, two team members, a plain member and an outside
/// collaborator; `octo` (the dev-token account) is a member GitHub knows
/// about but the full listing omits, which exercises the lazy sync.
fn fake_github() -> Router {
    async fn me(headers: HeaderMap) -> (StatusCode, Json<Value>) {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if auth == format!("Bearer {DEV_TOKEN}") {
            (StatusCode::OK, Json(user(1, "octo")))
        } else {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"message": "Bad credentials"})),
            )
        }
    }
    async fn emails() -> Json<Value> {
        Json(json!([
            {"email": "old@example.com", "primary": false, "verified": true},
            {"email": "octo@example.com", "primary": true, "verified": true}
        ]))
    }
    async fn org_members(
        Query(q): Query<std::collections::HashMap<String, String>>,
    ) -> Json<Value> {
        if q.get("page").is_some_and(|p| p != "1") {
            return Json(json!([]));
        }
        Json(match q.get("role").map(String::as_str) {
            Some("admin") => json!([user(10, "alice")]),
            _ => json!([user(11, "bob"), user(12, "carol"), user(13, "dave")]),
        })
    }
    async fn outside(Query(q): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
        if q.get("page").is_some_and(|p| p != "1") {
            return Json(json!([]));
        }
        Json(json!([user(14, "erin")]))
    }
    async fn team_members(Path((_, team)): Path<(String, String)>) -> (StatusCode, Json<Value>) {
        match team.as_str() {
            "sdi-admins" => (StatusCode::OK, Json(json!([user(11, "bob")]))),
            "sdi-approvers" => (StatusCode::OK, Json(json!([user(12, "carol")]))),
            _ => (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"}))),
        }
    }
    async fn membership(Path((_, login)): Path<(String, String)>) -> (StatusCode, Json<Value>) {
        match login.as_str() {
            "octo" => (
                StatusCode::OK,
                Json(json!({"state": "active", "role": "member", "user": user(1, "octo")})),
            ),
            _ => (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"}))),
        }
    }
    async fn team_membership(
        Path((_, team, login)): Path<(String, String, String)>,
    ) -> (StatusCode, Json<Value>) {
        if team == "sdi-approvers" && login == "octo" {
            (
                StatusCode::OK,
                Json(json!({"state": "active", "role": "member"})),
            )
        } else {
            (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"})))
        }
    }
    async fn access_token(Path(id): Path<i64>, headers: HeaderMap) -> (StatusCode, Json<Value>) {
        let jwt = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or_default();
        // Three base64url segments: a JWT, not an OAuth token.
        if jwt.split('.').count() != 3 {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"message": "A JSON web token could not be decoded"})),
            );
        }
        (
            StatusCode::CREATED,
            Json(
                json!({"token": format!("ghs_install_{id}"), "expires_at": "2099-01-01T00:00:00Z"}),
            ),
        )
    }
    async fn installation(Path(id): Path<i64>) -> Json<Value> {
        Json(
            json!({"id": id, "account": {"login": "acme", "type": "Organization", "avatar_url": "https://avatars/acme"}, "repository_selection": "all"}),
        )
    }
    Router::new()
        .route("/user", get(me))
        .route("/user/emails", get(emails))
        .route("/orgs/{org}/members", get(org_members))
        .route("/orgs/{org}/outside_collaborators", get(outside))
        .route("/orgs/{org}/teams/{team}/members", get(team_members))
        .route("/orgs/{org}/memberships/{login}", get(membership))
        .route(
            "/orgs/{org}/teams/{team}/memberships/{login}",
            get(team_membership),
        )
        .route("/app/installations/{id}", get(installation))
        .route("/app/installations/{id}/access_tokens", post(access_token))
}

impl Harness {
    async fn new() -> Option<Self> {
        Self::new_at("", "").await
    }

    /// The fake GitHub mounted under `prefix` (`/api/v3` for an enterprise
    /// server) and the deployment signing in to `host` (empty: the fake's
    /// own address, like github.com's `api.` host maps to github.com).
    async fn new_at(prefix: &'static str, host: &str) -> Option<Self> {
        let Some(url) = std::env::var("DATABASE_URL")
            .ok()
            .filter(|u| !u.trim().is_empty())
        else {
            eprintln!("skipping: DATABASE_URL is not set");
            return None;
        };
        let suffix: String = Uuid::now_v7()
            .as_bytes()
            .iter()
            .take(12)
            .map(|b| char::from(b'a' + b % 26))
            .collect();
        let schema = format!("identity_test_{suffix}");
        let pool = sdi_common::db::connect(&url, &schema)
            .await
            .expect("connect to dev postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = if prefix.is_empty() {
            fake_github()
        } else {
            Router::new().nest(prefix, fake_github())
        };
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let github_host = if host.is_empty() {
            format!("{addr}")
        } else {
            host.to_owned()
        };

        let settings = Settings {
            listen: "127.0.0.1:0".parse().unwrap(),
            database_url: url,
            public_url: "http://127.0.0.1:5173".into(),
            session_secret: b"test-secret-test-secret-test-secret!".to_vec(),
            oauth: None,
            app: Some(GithubApp {
                app_id: "4242".into(),
                private_key_pem: TEST_APP_KEY.into(),
            }),
            dev_login: true,
            github_token: Some(DEV_TOKEN.into()),
            github_api_url: format!("http://{addr}{prefix}"),
            github_host,
        };
        Some(Self {
            state: Arc::new(IdentityState::new(pool, settings)),
            schema,
        })
    }

    async fn login(&self) -> (String, String) {
        let begin = sessions::begin(
            &self.state,
            BeginLoginRequest {
                return_to: "/projects".into(),
            },
        )
        .unwrap();
        assert!(
            begin
                .authorize_url
                .starts_with("/api/v1/auth/callback?code=dev&state=dev"),
            "{}",
            begin.authorize_url
        );
        assert!(
            begin.authorize_url.contains("returnTo=%2Fprojects"),
            "{}",
            begin.authorize_url
        );
        let session = sessions::complete(
            &self.state,
            CompleteLoginRequest {
                code: "dev".into(),
                state: "dev".into(),
            },
        )
        .await
        .expect("dev login");
        let user = session.user.clone().unwrap();
        (session.token, user.id)
    }

    async fn drop_schema(&self) {
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.state.pool)
        .await;
    }
}

#[tokio::test]
async fn dev_login_creates_the_user_their_workspace_and_a_session() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let (token, user_id) = h.login().await;
    assert!(token.len() >= 40);

    let session = sessions::get(
        &h.state,
        GetSessionRequest {
            token: token.clone(),
        },
    )
    .await
    .unwrap();
    let user = session.user.unwrap();
    assert_eq!(user.login, "octo");
    assert_eq!(user.github_id, 1);
    assert_eq!(user.email, "octo@example.com", "primary verified address");
    assert_eq!(user.id, user_id);

    let list = workspaces::list(
        &h.state,
        ListWorkspacesRequest {
            user_id: user_id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(list.workspaces.len(), 1);
    let personal = &list.workspaces[0];
    assert_eq!(personal.kind, WorkspaceKind::Personal as i32);
    assert_eq!(personal.slug, "octo");
    assert_eq!(personal.my_role, ProtoRole::Owner as i32);
    assert_eq!(personal.installation_id, 0);

    // No workspace named: the personal one.
    let principal = authorize::authorize(
        &h.state,
        AuthorizeRequest {
            session_token: token.clone(),
            workspace_id: String::new(),
        },
    )
    .await
    .unwrap();
    assert_eq!(principal.role, ProtoRole::Owner as i32);
    assert_eq!(principal.workspace.unwrap().id, personal.id);

    // A personal workspace reads GitHub with the owner's OAuth token.
    let installation = workspaces::installation_token(
        &h.state,
        GetInstallationTokenRequest {
            workspace_id: personal.id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(installation.token, DEV_TOKEN);
    assert_eq!(installation.api_base_url, h.state.settings.github_api_url);

    // Someone else's workspace id, or a made-up one, is a flat denial.
    let denied = authorize::authorize(
        &h.state,
        AuthorizeRequest {
            session_token: token.clone(),
            workspace_id: Uuid::now_v7().to_string(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    let denied = authorize::authorize(
        &h.state,
        AuthorizeRequest {
            session_token: "nope".into(),
            workspace_id: String::new(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code(), Code::Unauthenticated);

    // Logging in again reuses the user and workspace.
    let (token2, user_id2) = h.login().await;
    assert_eq!(user_id2, user_id);
    assert_ne!(token2, token);
    let list = workspaces::list(
        &h.state,
        ListWorkspacesRequest {
            user_id: user_id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(list.workspaces.len(), 1);

    sessions::revoke(
        &h.state,
        RevokeSessionRequest {
            token: token.clone(),
        },
    )
    .await
    .unwrap();
    let gone = sessions::get(&h.state, GetSessionRequest { token })
        .await
        .unwrap_err();
    assert_eq!(gone.code(), Code::Unauthenticated);
    // The other session is untouched.
    sessions::get(&h.state, GetSessionRequest { token: token2 })
        .await
        .unwrap();

    h.drop_schema().await;
}

#[tokio::test]
async fn installations_sync_members_and_derive_roles() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let (token, user_id) = h.login().await;

    // Only the id, as the post-install redirect delivers it: the rest is
    // looked up with the app JWT.
    let ws = workspaces::record_installation(
        &h.state,
        RecordInstallationRequest {
            installation_id: 777,
            installed_by_user_id: user_id.clone(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(ws.kind, WorkspaceKind::Organization as i32);
    assert_eq!(ws.slug, "acme");
    assert_eq!(ws.installation_id, 777);
    assert_eq!(ws.repository_selection, "all");
    assert_eq!(ws.member_count, 5, "synced on record");

    let listed = members::list(
        &h.state,
        ListMembersRequest {
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap();
    let role_of = |login: &str| {
        let m = listed
            .members
            .iter()
            .find(|m| m.user.as_ref().unwrap().login == login)
            .unwrap_or_else(|| panic!("{login} missing"));
        (m.github_role.clone(), m.teams.clone(), m.effective_role)
    };
    assert_eq!(
        role_of("alice"),
        ("owner".into(), vec![], ProtoRole::Owner as i32)
    );
    assert_eq!(
        role_of("bob"),
        (
            "member".into(),
            vec!["sdi-admins".to_owned()],
            ProtoRole::Admin as i32
        )
    );
    assert_eq!(
        role_of("carol"),
        (
            "member".into(),
            vec!["sdi-approvers".to_owned()],
            ProtoRole::Approver as i32
        )
    );
    assert_eq!(
        role_of("dave"),
        ("member".into(), vec![], ProtoRole::Member as i32)
    );
    assert_eq!(
        role_of("erin"),
        (
            "outside_collaborator".into(),
            vec![],
            ProtoRole::Viewer as i32
        )
    );
    assert!(listed.synced_at.is_some());

    // The app token is minted with the JWT, then served from memory.
    let minted = workspaces::installation_token(
        &h.state,
        GetInstallationTokenRequest {
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(minted.token, "ghs_install_777");
    assert_eq!(h.state.tokens.lock().unwrap().len(), 1);

    // octo is not in the listing but GitHub says member + sdi-approvers:
    // opening the workspace syncs them lazily.
    let principal = authorize::authorize(
        &h.state,
        AuthorizeRequest {
            session_token: token.clone(),
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(principal.role, ProtoRole::Approver as i32);
    let mine = workspaces::list(
        &h.state,
        ListWorkspacesRequest {
            user_id: user_id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(mine.workspaces.len(), 2);
    assert_eq!(
        mine.workspaces[0].kind,
        WorkspaceKind::Personal as i32,
        "personal first"
    );
    assert_eq!(mine.workspaces[1].my_role, ProtoRole::Approver as i32);

    // Overrides only narrow.
    let bob = listed
        .members
        .iter()
        .find(|m| m.user.as_ref().unwrap().login == "bob")
        .unwrap()
        .user
        .clone()
        .unwrap();
    let narrowed = members::set_role(
        &h.state,
        SetMemberRoleRequest {
            workspace_id: ws.id.clone(),
            user_id: bob.id.clone(),
            override_role: ProtoRole::Member as i32,
        },
    )
    .await
    .unwrap();
    assert_eq!(narrowed.derived_role, ProtoRole::Admin as i32);
    assert_eq!(narrowed.override_role, ProtoRole::Member as i32);
    assert_eq!(narrowed.effective_role, ProtoRole::Member as i32);
    let refused = members::set_role(
        &h.state,
        SetMemberRoleRequest {
            workspace_id: ws.id.clone(),
            user_id: bob.id.clone(),
            override_role: ProtoRole::Owner as i32,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    let cleared = members::set_role(
        &h.state,
        SetMemberRoleRequest {
            workspace_id: ws.id.clone(),
            user_id: bob.id.clone(),
            override_role: 0,
        },
    )
    .await
    .unwrap();
    assert_eq!(cleared.effective_role, ProtoRole::Admin as i32);

    // A full sync drops octo again (GitHub's listing never had them) and
    // keeps the rest; the override survives the re-sync.
    members::set_role(
        &h.state,
        SetMemberRoleRequest {
            workspace_id: ws.id.clone(),
            user_id: bob.id.clone(),
            override_role: ProtoRole::Viewer as i32,
        },
    )
    .await
    .unwrap();
    let synced = members::sync(
        &h.state,
        SyncMembersRequest {
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(synced.members.len(), 5);
    assert!(
        synced
            .members
            .iter()
            .all(|m| m.user.as_ref().unwrap().login != "octo")
    );
    let bob_after = synced
        .members
        .iter()
        .find(|m| m.user.as_ref().unwrap().login == "bob")
        .unwrap();
    assert_eq!(bob_after.effective_role, ProtoRole::Viewer as i32);

    // Names for stored actor ids. A member resolves; a stranger, a free-text
    // actor and someone who left without a trace do not.
    let stranger = Uuid::now_v7().to_string();
    let named = members::resolve(
        &h.state,
        ResolveUsersRequest {
            workspace_id: ws.id.clone(),
            user_ids: vec![
                bob.id.clone(),
                bob.id.clone(),
                user_id.clone(),
                stranger.clone(),
                "ops@example.test".into(),
            ],
        },
    )
    .await
    .unwrap();
    assert_eq!(named.users.len(), 1, "{:?}", named.users);
    assert_eq!(named.users[0].id, bob.id);
    assert_eq!(named.users[0].login, "bob");
    assert!(named.users[0].member);
    // octo acted here before leaving: the audit trail keeps the name, marked
    // as no longer a member.
    audit::record(
        &h.state,
        RecordAuditRequest {
            entry: Some(AuditEntry {
                workspace_id: ws.id.clone(),
                user_id: user_id.clone(),
                user_login: "octo".into(),
                action: "deployment.promote".into(),
                ..Default::default()
            }),
        },
    )
    .await
    .unwrap();
    let former = members::resolve(
        &h.state,
        ResolveUsersRequest {
            workspace_id: ws.id.clone(),
            user_ids: vec![user_id.clone(), stranger.clone()],
        },
    )
    .await
    .unwrap();
    assert_eq!(former.users.len(), 1);
    assert_eq!(former.users[0].login, "octo");
    assert!(!former.users[0].member);
    // A personal workspace names its owner and nobody from another workspace.
    let own = members::resolve(
        &h.state,
        ResolveUsersRequest {
            workspace_id: mine.workspaces[0].id.clone(),
            user_ids: vec![user_id.clone(), bob.id.clone()],
        },
    )
    .await
    .unwrap();
    assert_eq!(own.users.len(), 1);
    assert_eq!(own.users[0].id, user_id);
    assert!(own.users[0].member);
    let too_many = members::resolve(
        &h.state,
        ResolveUsersRequest {
            workspace_id: ws.id.clone(),
            user_ids: vec![stranger; members::RESOLVE_LIMIT + 1],
        },
    )
    .await
    .unwrap_err();
    assert_eq!(too_many.code(), Code::InvalidArgument);

    // The webhook form: login known, no selection; nothing stored is lost.
    let again = workspaces::record_installation(
        &h.state,
        RecordInstallationRequest {
            installation_id: 777,
            account_login: "acme".into(),
            account_type: "Organization".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(again.id, ws.id);
    assert_eq!(again.repository_selection, "all");

    workspaces::remove_installation(
        &h.state,
        RemoveInstallationRequest {
            installation_id: 777,
        },
    )
    .await
    .unwrap();
    assert!(h.state.tokens.lock().unwrap().is_empty());
    let after = workspaces::get(
        &h.state,
        sdi_proto::identity::v1::GetWorkspaceRequest { id: ws.id.clone() },
    )
    .await
    .unwrap();
    assert_eq!(after.installation_id, 0);
    let no_token = workspaces::installation_token(
        &h.state,
        GetInstallationTokenRequest {
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(no_token.code(), Code::FailedPrecondition);

    // The personal-workspace path refuses overrides.
    let personal = mine.workspaces[0].id.clone();
    let err = members::set_role(
        &h.state,
        SetMemberRoleRequest {
            workspace_id: personal,
            user_id,
            override_role: ProtoRole::Viewer as i32,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);

    h.drop_schema().await;
}

#[tokio::test]
async fn audit_and_data_controls_round_trip() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let (_token, user_id) = h.login().await;
    let ws = workspaces::list(
        &h.state,
        ListWorkspacesRequest {
            user_id: user_id.clone(),
        },
    )
    .await
    .unwrap()
    .workspaces
    .remove(0);

    let defaults = controls::get(
        &h.state,
        GetDataControlsRequest {
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(defaults.code_cache, "session");
    assert_eq!(defaults.code_cache_ttl_hours, 24);
    assert!(!defaults.minimal_chunks);
    assert_eq!(defaults.reasoner, "off");
    assert_eq!(defaults.audit_retention_days, 365);
    assert!(!defaults.reasoner_key_configured);

    let updated = controls::update(
        &h.state,
        UpdateDataControlsRequest {
            controls: Some(DataControls {
                code_cache: "none".into(),
                code_cache_ttl_hours: 1,
                minimal_chunks: true,
                reasoner: "claude".into(),
                audit_retention_days: 0,
                ..defaults.clone()
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.code_cache, "none");
    assert!(updated.minimal_chunks);
    assert_eq!(updated.reasoner, "claude");
    assert_eq!(updated.audit_retention_days, 0);
    let bad = controls::update(
        &h.state,
        UpdateDataControlsRequest {
            controls: Some(DataControls {
                code_cache: "forever".into(),
                ..updated.clone()
            }),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);

    let keyed = controls::set_key(
        &h.state,
        SetReasonerKeyRequest {
            workspace_id: ws.id.clone(),
            api_key: "sk-ant-secret-1234".into(),
        },
    )
    .await
    .unwrap();
    assert!(keyed.reasoner_key_configured);
    assert_eq!(keyed.reasoner_key_hint, "1234");
    let sealed: Vec<u8> =
        sqlx::query_scalar("SELECT sealed_key FROM reasoner_keys WHERE workspace_id = $1")
            .bind(Uuid::parse_str(&ws.id).unwrap())
            .fetch_one(&h.state.pool)
            .await
            .unwrap();
    assert!(
        !sealed.windows(6).any(|w| w == b"secret"),
        "key is not stored in clear"
    );
    assert_eq!(
        h.state.keys.open_str(&sealed).unwrap(),
        "sk-ant-secret-1234"
    );
    let removed = controls::set_key(
        &h.state,
        SetReasonerKeyRequest {
            workspace_id: ws.id.clone(),
            api_key: String::new(),
        },
    )
    .await
    .unwrap();
    assert!(!removed.reasoner_key_configured);
    assert_eq!(removed.reasoner_key_hint, "");

    for (i, action) in ["project.connect", "scan.start", "project.disconnect"]
        .iter()
        .enumerate()
    {
        audit::record(
            &h.state,
            RecordAuditRequest {
                entry: Some(AuditEntry {
                    workspace_id: ws.id.clone(),
                    user_id: user_id.clone(),
                    user_login: "octo".into(),
                    action: (*action).to_owned(),
                    entity: "project".into(),
                    entity_id: format!("p{i}"),
                    at: Some(sdi_common::time::to_proto(
                        chrono::Utc::now() + chrono::Duration::seconds(i as i64),
                    )),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
    }
    let all = audit::list(
        &h.state,
        ListAuditRequest {
            workspace_id: ws.id.clone(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        all.entries
            .iter()
            .map(|e| e.action.as_str())
            .collect::<Vec<_>>(),
        ["project.disconnect", "scan.start", "project.connect"],
        "newest first"
    );
    let projects = audit::list(
        &h.state,
        ListAuditRequest {
            workspace_id: ws.id.clone(),
            action_prefix: "project.".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(projects.entries.len(), 2);
    let older = audit::list(
        &h.state,
        ListAuditRequest {
            workspace_id: ws.id.clone(),
            before: all.entries[0].at,
            limit: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(older.entries.len(), 1);
    assert_eq!(older.entries[0].action, "scan.start");

    // A service records with the user id only: the login is filled in; the
    // system principal is not a user and keeps an empty login.
    for (user, action) in [
        (user_id.as_str(), "application.sync"),
        ("system", "application.auto_sync"),
    ] {
        audit::record(
            &h.state,
            RecordAuditRequest {
                entry: Some(AuditEntry {
                    workspace_id: ws.id.clone(),
                    user_id: user.to_owned(),
                    action: action.to_owned(),
                    entity: "application".into(),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
    }
    let applications = audit::list(
        &h.state,
        ListAuditRequest {
            workspace_id: ws.id.clone(),
            action_prefix: "application.".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let login_of = |action: &str| {
        applications
            .entries
            .iter()
            .find(|e| e.action == action)
            .map(|e| e.user_login.clone())
    };
    assert_eq!(login_of("application.sync").as_deref(), Some("octo"));
    assert_eq!(login_of("application.auto_sync").as_deref(), Some(""));
    let missing = audit::record(
        &h.state,
        RecordAuditRequest {
            entry: Some(AuditEntry {
                workspace_id: ws.id.clone(),
                ..Default::default()
            }),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(missing.code(), Code::InvalidArgument);

    // Retention 0 keeps everything; one day drops nothing younger than that.
    assert_eq!(crate::db::audit::prune(&h.state.pool).await.unwrap(), 0);

    h.drop_schema().await;
}

/// `code_to_model` (CONTRACT.md §19, §23): nothing leaves by default, the
/// two values round trip, anything else is refused with the allowed values,
/// and a caller that does not know the field cannot change it.
#[tokio::test]
async fn code_to_model_defaults_off_round_trips_and_refuses_unknown_values() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let (_token, user_id) = h.login().await;
    let ws = workspaces::list(&h.state, ListWorkspacesRequest { user_id })
        .await
        .unwrap()
        .workspaces
        .remove(0);
    let get = || async {
        controls::get(
            &h.state,
            GetDataControlsRequest {
                workspace_id: ws.id.clone(),
            },
        )
        .await
        .unwrap()
    };
    let update = |controls: DataControls| async {
        controls::update(
            &h.state,
            UpdateDataControlsRequest {
                controls: Some(controls),
            },
        )
        .await
    };

    let defaults = get().await;
    assert_eq!(defaults.code_to_model, "off", "the most private default");
    let stored: String =
        sqlx::query_scalar("SELECT code_to_model FROM data_controls WHERE workspace_id = $1")
            .bind(Uuid::parse_str(&ws.id).unwrap())
            .fetch_one(&h.state.pool)
            .await
            .unwrap();
    assert_eq!(stored, "off", "the column default, not a conversion");

    // Round trip, case and padding normalised like the other vocabularies.
    let on = update(DataControls {
        code_to_model: " Changed_Lines ".into(),
        ..defaults.clone()
    })
    .await
    .unwrap();
    assert_eq!(on.code_to_model, "changed_lines");
    assert_eq!(get().await.code_to_model, "changed_lines");
    assert_eq!(on.reasoner, defaults.reasoner, "nothing else moved");
    assert_eq!(on.code_cache, defaults.code_cache);

    // Anything else is refused, naming what is allowed, and nothing is stored.
    for bad in ["whole_file", "on", "true"] {
        let refused = update(DataControls {
            code_to_model: bad.into(),
            ..on.clone()
        })
        .await
        .unwrap_err();
        assert_eq!(refused.code(), Code::InvalidArgument, "{bad}");
        assert!(
            refused.message().contains("off") && refused.message().contains("changed_lines"),
            "{}",
            refused.message()
        );
    }
    assert_eq!(get().await.code_to_model, "changed_lines");

    // A caller that leaves the field empty changes the rest and keeps it.
    let kept = update(DataControls {
        code_to_model: String::new(),
        minimal_chunks: true,
        ..on.clone()
    })
    .await
    .unwrap();
    assert!(kept.minimal_chunks);
    assert_eq!(
        kept.code_to_model, "changed_lines",
        "empty keeps the stored value"
    );

    let off = update(DataControls {
        code_to_model: "off".into(),
        ..kept
    })
    .await
    .unwrap();
    assert_eq!(off.code_to_model, "off");
    assert_eq!(get().await.code_to_model, "off");

    h.drop_schema().await;
}

#[test]
fn role_text_matches_the_common_crate() {
    assert_eq!(roles::to_db(Role::Approver), "APPROVER");
}

/// GitHub Enterprise Server end to end: the deployment signs in to
/// `ghe.corp.example`, whose API lives under `/api/v3`. The installation
/// is recorded with that base, its token is minted there, and what
/// knowledge and execution receive (`InstallationToken.api_base_url`) is
/// that base, so every repository call of theirs lands on the enterprise
/// host. OAuth itself goes to the web host, never under `/api/v3`.
#[tokio::test]
async fn enterprise_installations_mint_tokens_under_their_own_api_base() {
    let Some(h) = Harness::new_at("/api/v3", "ghe.corp.example").await else {
        return;
    };
    let base = h.state.settings.github_api_url.clone();
    assert!(base.ends_with("/api/v3"), "{base}");
    // The harness serves the fake enterprise API over plain http, so the
    // derived web URL keeps that scheme; a real GHES base is https.
    assert!(
        h.state
            .settings
            .github_web_url()
            .ends_with("://ghe.corp.example"),
        "OAuth endpoints are on the web host"
    );
    assert_eq!(
        crate::config::github_web_url_of("https://ghe.corp.example/api/v3", "ghe.corp.example"),
        "https://ghe.corp.example"
    );
    let (_, user_id) = h.login().await;

    let ws = workspaces::record_installation(
        &h.state,
        RecordInstallationRequest {
            installation_id: 4242,
            installed_by_user_id: user_id,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(ws.github_host, "ghe.corp.example");
    assert_eq!(ws.slug, "acme", "looked up through {base}");

    let token = workspaces::installation_token(
        &h.state,
        GetInstallationTokenRequest {
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(token.token, "ghs_install_4242");
    assert_eq!(token.api_base_url, base);
    assert_eq!(h.state.cached_tokens(), 1);

    // A second organisation on github.com proper keeps the public base,
    // and any other host gets `/api/v3` derived.
    assert_eq!(
        workspaces::api_base_of(&h.state.settings, "github.com"),
        "https://api.github.com"
    );
    assert_eq!(
        workspaces::api_base_of(&h.state.settings, "other.ghe.example"),
        "https://other.ghe.example/api/v3"
    );
    assert_eq!(
        workspaces::api_base_of(&h.state.settings, "ghe.corp.example"),
        base
    );

    h.drop_schema().await;
}

/// The token cache is bounded: minting for more installations than it
/// holds evicts the oldest instead of growing.
#[tokio::test]
async fn installation_token_cache_is_bounded() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let (_, user_id) = h.login().await;
    for id in 1..=3 {
        let ws = workspaces::record_installation(
            &h.state,
            RecordInstallationRequest {
                installation_id: id,
                installed_by_user_id: user_id.clone(),
                account_login: format!("org{id}"),
                account_type: "Organization".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        workspaces::installation_token(
            &h.state,
            GetInstallationTokenRequest {
                workspace_id: ws.id.clone(),
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(h.state.cached_tokens(), 3);
    {
        let mut tokens = h.state.tokens.lock().unwrap();
        for id in 100..(100 + crate::grpc::TOKEN_CACHE_CAPACITY as i64) {
            tokens.insert(
                id,
                crate::grpc::CachedToken {
                    token: "x".into(),
                    expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                },
            );
        }
    }
    assert_eq!(h.state.cached_tokens(), crate::grpc::TOKEN_CACHE_CAPACITY);
    h.drop_schema().await;
}

/// Workspace secrets (CONTRACT.md §22): sealed at rest, managed by admins
/// of their own workspace only, never returned, and resolved in clear only
/// for an internal (owner-role) caller, with an audit entry per resolve.
#[tokio::test]
async fn secrets_are_sealed_admin_managed_and_resolved_only_by_internal_callers() {
    use sdi_common::principal::Principal as Caller;
    use sdi_proto::identity::v1::{
        DeleteSecretRequest, ListSecretsRequest, ResolveSecretRequest, SetSecretRequest,
        identity_service_server::IdentityService,
    };

    use crate::grpc::{IdentityGrpc, secrets};

    let Some(h) = Harness::new().await else {
        return;
    };
    let (_token, user_id) = h.login().await;
    let ws = workspaces::list(
        &h.state,
        ListWorkspacesRequest {
            user_id: user_id.clone(),
        },
    )
    .await
    .unwrap()
    .workspaces
    .remove(0);
    let caller = |role: Role| Caller {
        user_id: user_id.clone(),
        workspace_id: ws.id.clone(),
        role,
    };
    let admin = caller(Role::Admin);
    let value = "jenkins-api-token-0123456789-wxyz";
    let set = |name: &str, value: &str| SetSecretRequest {
        workspace_id: ws.id.clone(),
        name: name.to_owned(),
        value: value.to_owned(),
        description: "CI trigger token".into(),
    };

    // Stored: the answer describes the secret and never carries its value.
    let info = secrets::set(&h.state, &admin, set("JENKINS_TOKEN", value))
        .await
        .unwrap();
    assert_eq!(info.name, "JENKINS_TOKEN");
    assert_eq!(info.hint, "wxyz");
    assert_eq!(info.description, "CI trigger token");
    assert_eq!(info.updated_by, "octo", "the login, not the user id");
    assert!(info.updated_at.is_some());
    assert!(!format!("{info:?}").contains(value));

    // At rest the value is sealed: the column neither equals nor contains it.
    let workspace_id = Uuid::parse_str(&ws.id).unwrap();
    let sealed: Vec<u8> = sqlx::query_scalar(
        "SELECT sealed_value FROM secrets WHERE workspace_id = $1 AND name = $2",
    )
    .bind(workspace_id)
    .bind("JENKINS_TOKEN")
    .fetch_one(&h.state.pool)
    .await
    .unwrap();
    assert!(
        !sealed
            .windows(value.len())
            .any(|window| window == value.as_bytes())
    );
    assert_eq!(h.state.keys.open_str(&sealed).unwrap(), value);

    // Validation: the name pattern, an empty or oversized value.
    for bad in ["jenkins_token", "1TOKEN", "A", "MY-TOKEN", ""] {
        let err = secrets::set(&h.state, &admin, set(bad, value))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument, "{bad}");
    }
    let err = secrets::set(&h.state, &admin, set("EMPTY_ONE", ""))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = secrets::set(
        &h.state,
        &admin,
        set("HUGE_ONE", &"x".repeat(secrets::MAX_VALUE_BYTES + 1)),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);

    // A second one, short enough to get no hint.
    let short = secrets::set(&h.state, &admin, set("WEBHOOK_KEY", "tiny"))
        .await
        .unwrap();
    assert_eq!(short.hint, "");

    // Listing: names, hints, descriptions; sorted; no value anywhere.
    let listed = secrets::list(
        &h.state,
        &admin,
        ListSecretsRequest {
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap()
    .secrets;
    assert_eq!(
        listed.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["JENKINS_TOKEN", "WEBHOOK_KEY"]
    );
    assert!(!format!("{listed:?}").contains(value));

    // Management needs an admin of this very workspace.
    for role in [Role::Viewer, Role::Member, Role::Approver] {
        let low = caller(role);
        let err = secrets::list(
            &h.state,
            &low,
            ListSecretsRequest {
                workspace_id: ws.id.clone(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), Code::PermissionDenied, "{role:?} lists");
        let err = secrets::set(&h.state, &low, set("NEW_ONE", value))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Code::PermissionDenied, "{role:?} sets");
        let err = secrets::delete(
            &h.state,
            &low,
            DeleteSecretRequest {
                workspace_id: ws.id.clone(),
                name: "JENKINS_TOKEN".into(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), Code::PermissionDenied, "{role:?} deletes");
    }
    let elsewhere = Caller {
        workspace_id: Uuid::now_v7().to_string(),
        ..admin.clone()
    };
    let err = secrets::list(
        &h.state,
        &elsewhere,
        ListSecretsRequest {
            workspace_id: ws.id.clone(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied, "another workspace");

    // Resolve: every end-user role below owner is refused ...
    let resolve = |name: &str| ResolveSecretRequest {
        workspace_id: ws.id.clone(),
        name: name.to_owned(),
    };
    for role in [Role::Viewer, Role::Member, Role::Approver, Role::Admin] {
        let err = secrets::resolve(&h.state, &caller(role), resolve("JENKINS_TOKEN"))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Code::PermissionDenied, "{role:?} resolves");
    }
    // ... a call without metadata is not even identified ...
    let grpc = IdentityGrpc::new(Arc::clone(&h.state));
    let err = grpc
        .resolve_secret(tonic::Request::new(resolve("JENKINS_TOKEN")))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::Unauthenticated);
    let err = grpc
        .list_secrets(tonic::Request::new(ListSecretsRequest {
            workspace_id: ws.id.clone(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::Unauthenticated);
    // ... the system principal of another workspace is refused ...
    let err = secrets::resolve(
        &h.state,
        &Caller::system(Uuid::now_v7().to_string()),
        resolve("JENKINS_TOKEN"),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);
    // ... and nothing so far was audited as a resolve.
    let audited = |prefix: &'static str| {
        let state = Arc::clone(&h.state);
        let workspace_id = ws.id.clone();
        async move {
            audit::list(
                &state,
                ListAuditRequest {
                    workspace_id,
                    action_prefix: prefix.into(),
                    limit: 0,
                    before: None,
                },
            )
            .await
            .unwrap()
            .entries
        }
    };
    assert!(audited("secret.").await.is_empty());

    // The internal caller gets the value, through the real gRPC surface.
    let system = Caller::system(ws.id.clone());
    let mut request = tonic::Request::new(resolve("JENKINS_TOKEN"));
    system.apply(&mut request);
    let resolved = grpc.resolve_secret(request).await.unwrap().into_inner();
    assert_eq!(resolved.value, value);
    let err = secrets::resolve(&h.state, &system, resolve("NOT_THERE"))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);

    // Exactly the successful resolve is in the audit trail: who, which
    // secret, and never the value.
    let entries = audited("secret.resolve").await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].action, "secret.resolve");
    assert_eq!(entries[0].entity, "secret");
    assert_eq!(entries[0].entity_id, "JENKINS_TOKEN");
    assert_eq!(entries[0].user_id, "system");
    assert!(!format!("{entries:?}").contains(value));

    // Replacing keeps one row and refreshes the hint.
    let replaced = secrets::set(
        &h.state,
        &admin,
        set("JENKINS_TOKEN", "a-brand-new-token-value-QRST"),
    )
    .await
    .unwrap();
    assert_eq!(replaced.hint, "QRST");
    assert_eq!(
        secrets::resolve(&h.state, &system, resolve("JENKINS_TOKEN"))
            .await
            .unwrap()
            .value,
        "a-brand-new-token-value-QRST"
    );

    // Deleting removes it for good.
    secrets::delete(
        &h.state,
        &admin,
        DeleteSecretRequest {
            workspace_id: ws.id.clone(),
            name: "JENKINS_TOKEN".into(),
        },
    )
    .await
    .unwrap();
    let err = secrets::delete(
        &h.state,
        &admin,
        DeleteSecretRequest {
            workspace_id: ws.id.clone(),
            name: "JENKINS_TOKEN".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let err = secrets::resolve(&h.state, &system, resolve("JENKINS_TOKEN"))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);

    // A secret of a workspace that does not exist cannot be stored ...
    let ghost = Uuid::now_v7().to_string();
    let err = secrets::set(
        &h.state,
        &Caller {
            workspace_id: ghost.clone(),
            ..admin.clone()
        },
        SetSecretRequest {
            workspace_id: ghost,
            name: "ANY_ONE".into(),
            value: value.into(),
            description: String::new(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    // ... and the rows go with their workspace.
    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(workspace_id)
        .execute(&h.state.pool)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM secrets WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(&h.state.pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    h.drop_schema().await;
}
