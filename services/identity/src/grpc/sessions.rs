//! GitHub OAuth sign-in and the opaque sessions it produces (CONTRACT.md §18).

use chrono::{Duration, Utc};
use sdi_common::{grpc::db_status, time};
use sdi_proto::identity::v1::{
    BeginLoginRequest, BeginLoginResponse, CompleteLoginRequest, GetSessionRequest,
    RevokeSessionRequest, RevokeSessionResponse, Session,
};
use tonic::Status;

use super::{IdentityState, github_status};
use crate::{convert, db};

/// Sessions live 30 days (§18).
const SESSION_TTL: Duration = Duration::days(30);
/// What the personal workspace needs: profile, email, and repository read
/// (GitHub has no read-only `repo` scope for OAuth apps).
const OAUTH_SCOPES: &str = "read:user user:email repo";
/// Dev mode: both the code and the state are this literal.
pub const DEV_MARKER: &str = "dev";

/// Only same-origin targets: a path, or an absolute URL under
/// `SDI_PUBLIC_URL`. Anything else falls back to the root.
pub fn safe_return_to(public_url: &str, raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return "/".to_owned();
    }
    let is_path = raw.starts_with('/') && !raw.starts_with("//") && !raw.starts_with("/\\");
    let is_same_origin = raw
        .strip_prefix(public_url)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/') || rest.starts_with('?'));
    if is_path || is_same_origin {
        raw.to_owned()
    } else {
        "/".to_owned()
    }
}

fn redirect_uri(state: &IdentityState) -> String {
    format!("{}/api/v1/auth/callback", state.settings.public_url)
}

pub fn begin(state: &IdentityState, req: BeginLoginRequest) -> Result<BeginLoginResponse, Status> {
    let return_to = safe_return_to(&state.settings.public_url, &req.return_to);
    if state.settings.dev_login {
        let mut url = format!("/api/v1/auth/callback?code={DEV_MARKER}&state={DEV_MARKER}");
        if return_to != "/" {
            url.push_str("&returnTo=");
            url.push_str(&percent_encode(&return_to));
        }
        return Ok(BeginLoginResponse { authorize_url: url });
    }
    let Some(oauth) = &state.settings.oauth else {
        return Err(Status::failed_precondition(
            "sign-in is not configured: set GITHUB_OAUTH_CLIENT_ID/SECRET or SDI_DEV_LOGIN=1",
        ));
    };
    let signed = state.keys.sign_state(&return_to);
    let authorize_url = format!(
        "{}/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={}",
        state.settings.github_web_url(),
        percent_encode(&oauth.client_id),
        percent_encode(&redirect_uri(state)),
        percent_encode(OAUTH_SCOPES),
        percent_encode(&signed),
    );
    Ok(BeginLoginResponse { authorize_url })
}

pub async fn complete(state: &IdentityState, req: CompleteLoginRequest) -> Result<Session, Status> {
    if req.code.trim().is_empty() {
        return Err(Status::invalid_argument("code is required"));
    }
    if state.settings.dev_login {
        if req.state != DEV_MARKER || req.code != DEV_MARKER {
            return Err(Status::invalid_argument(
                "dev login expects code=dev and state=dev",
            ));
        }
        let Some(token) = state.settings.github_token.clone() else {
            // No GitHub account to sign in as: a local developer whose
            // workspace has no GitHub access, so the app can still be tried
            // (and its e2e suite run) without one.
            return open_session(state, LOCAL_DEVELOPER, "", "", "/").await;
        };
        return establish(state, &token, "", "/").await;
    }
    let Some(oauth) = &state.settings.oauth else {
        return Err(Status::failed_precondition("sign-in is not configured"));
    };
    let login_state = state
        .keys
        .verify_state(&req.state)
        .map_err(|error| Status::invalid_argument(format!("login state rejected: {error}")))?;
    let token = state
        .github
        .exchange_code(
            &state.settings.github_web_url(),
            &oauth.client_id,
            &oauth.client_secret,
            req.code.trim(),
            &redirect_uri(state),
        )
        .await
        .map_err(github_status)?;
    establish(
        state,
        &token.access_token,
        &token.scope,
        &login_state.return_to,
    )
    .await
}

/// Reads the account behind an OAuth token, creates or refreshes the user
/// and their personal workspace, and opens a session.
async fn establish(
    state: &IdentityState,
    oauth_token: &str,
    scopes: &str,
    return_to: &str,
) -> Result<Session, Status> {
    let api = &state.settings.github_api_url;
    let profile = state
        .github
        .user(api, oauth_token)
        .await
        .map_err(github_status)?;
    let email = match profile.email.clone().filter(|e| !e.is_empty()) {
        Some(email) => Some(email),
        None => state.github.primary_email(api, oauth_token).await,
    };
    let account = Account {
        github_id: profile.id,
        login: &profile.login,
        name: profile.name.as_deref().unwrap_or_default(),
        avatar_url: profile.avatar_url.as_deref().unwrap_or_default(),
        email: email.as_deref(),
    };
    open_session(state, account, oauth_token, scopes, return_to).await
}

/// Who a session is opened for.
struct Account<'a> {
    github_id: i64,
    login: &'a str,
    name: &'a str,
    avatar_url: &'a str,
    email: Option<&'a str>,
}

/// Dev login without `GITHUB_TOKEN`. GitHub ids start at 1, so 0 is never
/// a real account; its workspace's GitHub token is refused with a sentence.
const LOCAL_DEVELOPER: Account<'static> = Account {
    github_id: 0,
    login: "local-dev",
    name: "Local developer",
    avatar_url: "",
    email: None,
};

/// Creates or refreshes the user and their personal workspace, and opens a
/// session holding `oauth_token` (sealed; empty for the local developer).
async fn open_session(
    state: &IdentityState,
    account: Account<'_>,
    oauth_token: &str,
    scopes: &str,
    return_to: &str,
) -> Result<Session, Status> {
    let host = state.settings.github_host.clone();

    let mut tx = state.pool.begin().await.map_err(db_status)?;
    let user = db::users::upsert(
        &mut *tx,
        db::users::Profile {
            github_host: &host,
            github_id: account.github_id,
            login: account.login,
            name: account.name,
            avatar_url: account.avatar_url,
            email: account.email,
        },
    )
    .await
    .map_err(db_status)?;
    let workspace_id = db::workspaces::ensure_personal(
        &mut *tx,
        user.id,
        &host,
        &user.login,
        if user.name.is_empty() {
            &user.login
        } else {
            &user.name
        },
        &user.avatar_url,
    )
    .await
    .map_err(db_status)?;
    db::workspaces::ensure_controls(&mut *tx, workspace_id)
        .await
        .map_err(db_status)?;

    let token = crate::crypto::Keys::new_token();
    let session = db::sessions::insert(
        &mut *tx,
        user.id,
        &state.keys.hash_token(&token),
        &state.keys.seal(oauth_token.as_bytes()),
        scopes,
        Utc::now() + SESSION_TTL,
    )
    .await
    .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;

    tracing::info!(login = %user.login, host, "session created");
    Ok(Session {
        token,
        user: Some(convert::user_to_proto(user)),
        expires_at: Some(time::to_proto(session.expires_at)),
        return_to: return_to.to_owned(),
    })
}

/// The live session for a token, or `UNAUTHENTICATED`.
pub(crate) async fn resolve(
    state: &IdentityState,
    token: &str,
) -> Result<(db::sessions::SessionRow, db::users::UserRow), Status> {
    if token.trim().is_empty() {
        return Err(Status::unauthenticated("no session"));
    }
    let session = db::sessions::get_live(&state.pool, &state.keys.hash_token(token.trim()))
        .await
        .map_err(db_status)?
        .ok_or_else(|| Status::unauthenticated("session expired or revoked"))?;
    let user = db::users::get(&state.pool, session.user_id)
        .await
        .map_err(db_status)?;
    Ok((session, user))
}

pub async fn get(state: &IdentityState, req: GetSessionRequest) -> Result<Session, Status> {
    let (session, user) = resolve(state, &req.token).await?;
    Ok(Session {
        token: String::new(),
        user: Some(convert::user_to_proto(user)),
        expires_at: Some(time::to_proto(session.expires_at)),
        return_to: String::new(),
    })
}

pub async fn revoke(
    state: &IdentityState,
    req: RevokeSessionRequest,
) -> Result<RevokeSessionResponse, Status> {
    if !req.token.trim().is_empty() {
        db::sessions::revoke(&state.pool, &state.keys.hash_token(req.token.trim()))
            .await
            .map_err(db_status)?;
    }
    Ok(RevokeSessionResponse {})
}

/// Minimal application/x-www-form-urlencoded escaping for URL query values.
pub(crate) fn percent_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_to_is_restricted_to_the_app_origin() {
        let public = "https://sdi.example.com";
        assert_eq!(safe_return_to(public, ""), "/");
        assert_eq!(safe_return_to(public, "/projects/1"), "/projects/1");
        assert_eq!(
            safe_return_to(public, "https://sdi.example.com/x?y=1"),
            "https://sdi.example.com/x?y=1"
        );
        assert_eq!(
            safe_return_to(public, "https://sdi.example.com"),
            "https://sdi.example.com"
        );
        assert_eq!(
            safe_return_to(public, "https://sdi.example.com.evil.io/"),
            "/"
        );
        assert_eq!(safe_return_to(public, "https://evil.io/"), "/");
        assert_eq!(safe_return_to(public, "//evil.io/"), "/");
        assert_eq!(safe_return_to(public, "/\\evil.io"), "/");
    }

    /// On GitHub Enterprise Server the authorize URL is
    /// `https://<host>/login/oauth/authorize` with the public callback; no
    /// database is touched to build it.
    #[tokio::test]
    async fn enterprise_sign_in_goes_to_the_enterprise_web_host() {
        use crate::config::{OAuthApp, Settings};
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let settings = Settings {
            listen: "127.0.0.1:0".parse().unwrap(),
            database_url: String::new(),
            public_url: "https://sdi.corp.example".into(),
            session_secret: b"test-secret-test-secret-test-secret!".to_vec(),
            oauth: Some(OAuthApp {
                client_id: "Iv1.enterprise".into(),
                client_secret: "s".into(),
            }),
            app: None,
            dev_login: false,
            github_token: None,
            github_api_url: "https://ghe.corp.example/api/v3".into(),
            github_host: crate::config::host_of("https://ghe.corp.example/api/v3"),
        };
        let state = IdentityState::new(pool, settings);
        let begin = begin(
            &state,
            BeginLoginRequest {
                return_to: "/projects".into(),
            },
        )
        .unwrap();
        assert!(
            begin.authorize_url.starts_with(
                "https://ghe.corp.example/login/oauth/authorize?client_id=Iv1.enterprise&redirect_uri=https%3A%2F%2Fsdi.corp.example%2Fapi%2Fv1%2Fauth%2Fcallback&scope="
            ),
            "{}",
            begin.authorize_url
        );
        assert!(!begin.authorize_url.contains("/api/v3/"));
        assert!(!begin.authorize_url.contains("github.com"));
    }

    #[test]
    fn percent_encoding_covers_reserved_characters() {
        assert_eq!(percent_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(percent_encode("safe-_.~"), "safe-_.~");
    }
}
