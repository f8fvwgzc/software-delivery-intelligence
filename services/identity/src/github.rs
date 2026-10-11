//! The GitHub calls identity makes: the OAuth code exchange, the signed-in
//! user's profile, organisation membership and teams, and GitHub App
//! installation tokens. Every failure is typed so the gRPC layer can
//! answer with the right status.

use std::time::Duration;

use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::{StatusCode, header::ACCEPT};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

const USER_AGENT: &str = "sdi-identity";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const PER_PAGE: u32 = 100;
/// Hard stop for paginated listings (10 000 members).
const MAX_PAGES: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    #[error("GitHub rejected the credentials")]
    Unauthorized,
    #[error("GitHub forbade the request: {0}")]
    Forbidden(String),
    #[error("not found on GitHub: {0}")]
    NotFound(String),
    #[error("GitHub answered {status}: {message}")]
    Api { status: u16, message: String },
    #[error("GitHub request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("could not decode GitHub response: {0}")]
    Decode(String),
    #[error("OAuth exchange failed: {0}")]
    OAuth(String),
    #[error("GitHub App key is not a usable RSA PEM: {0}")]
    AppKey(String),
}

pub type Result<T> = std::result::Result<T, GithubError>;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GhUser {
    pub id: i64,
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhEmail {
    pub email: String,
    pub primary: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhAccount {
    pub login: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhInstallation {
    pub account: GhAccount,
    #[serde(default)]
    pub repository_selection: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthToken {
    pub access_token: String,
    pub scope: String,
}

#[derive(Debug, Clone)]
pub struct InstallationToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// One org member as the membership sync sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgMember {
    pub user: GhUser,
    /// "owner" | "member" | "outside_collaborator"
    pub github_role: String,
    pub teams: Vec<String>,
}

#[derive(Clone)]
pub struct GithubClient {
    http: reqwest::Client,
}

impl Default for GithubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GithubClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client builds with static options");
        Self { http }
    }

    // --- OAuth ---------------------------------------------------------------

    /// `POST {web}/login/oauth/access_token` (GitHub accepts the parameters
    /// as JSON and answers JSON when asked).
    pub async fn exchange_code(
        &self,
        web_url: &str,
        client_id: &str,
        client_secret: &str,
        code: &str,
        redirect_uri: &str,
    ) -> Result<OAuthToken> {
        #[derive(Serialize)]
        struct Form<'a> {
            client_id: &'a str,
            client_secret: &'a str,
            code: &'a str,
            redirect_uri: &'a str,
        }
        #[derive(Deserialize)]
        struct Reply {
            #[serde(default)]
            access_token: Option<String>,
            #[serde(default)]
            scope: Option<String>,
            #[serde(default)]
            error: Option<String>,
            #[serde(default)]
            error_description: Option<String>,
        }
        let response = self
            .http
            .post(format!("{web_url}/login/oauth/access_token"))
            .header(ACCEPT, "application/json")
            .json(&Form {
                client_id,
                client_secret,
                code,
                redirect_uri,
            })
            .send()
            .await?;
        let status = response.status();
        let reply: Reply = response
            .json()
            .await
            .map_err(|e| GithubError::Decode(e.to_string()))?;
        match reply.access_token {
            Some(access_token) => Ok(OAuthToken {
                access_token,
                scope: reply.scope.unwrap_or_default(),
            }),
            None => Err(GithubError::OAuth(
                reply
                    .error_description
                    .or(reply.error)
                    .unwrap_or_else(|| format!("status {status}")),
            )),
        }
    }

    // --- Users ----------------------------------------------------------------

    pub async fn user(&self, api: &str, token: &str) -> Result<GhUser> {
        self.get(api, "/user", token).await
    }

    /// The primary verified address, or any verified one. Needs `user:email`;
    /// a token without it yields `None` instead of an error.
    pub async fn primary_email(&self, api: &str, token: &str) -> Option<String> {
        let emails: Vec<GhEmail> = self.get(api, "/user/emails", token).await.ok()?;
        emails
            .iter()
            .find(|e| e.primary && e.verified)
            .or_else(|| emails.iter().find(|e| e.verified))
            .map(|e| e.email.clone())
    }

    // --- Organisations --------------------------------------------------------

    /// Everyone with access to the org, with GitHub role and the SDI teams
    /// they are on. Needs an installation token with `members:read`.
    pub async fn org_members(&self, api: &str, token: &str, org: &str) -> Result<Vec<OrgMember>> {
        let owners: Vec<GhUser> = self
            .get_all(api, &format!("/orgs/{org}/members?role=admin"), token)
            .await?;
        let members: Vec<GhUser> = self
            .get_all(api, &format!("/orgs/{org}/members?role=member"), token)
            .await?;
        let outside: Vec<GhUser> = self
            .get_all(api, &format!("/orgs/{org}/outside_collaborators"), token)
            .await?;
        let mut result: Vec<OrgMember> = Vec::new();
        for (list, role) in [
            (owners, crate::roles::GITHUB_OWNER),
            (members, crate::roles::GITHUB_MEMBER),
            (outside, crate::roles::GITHUB_OUTSIDE_COLLABORATOR),
        ] {
            for user in list {
                if result.iter().any(|m| m.user.id == user.id) {
                    continue;
                }
                result.push(OrgMember {
                    user,
                    github_role: role.to_owned(),
                    teams: Vec::new(),
                });
            }
        }
        for team in [crate::roles::TEAM_ADMINS, crate::roles::TEAM_APPROVERS] {
            let on_team: Vec<GhUser> = match self
                .get_all(api, &format!("/orgs/{org}/teams/{team}/members"), token)
                .await
            {
                Ok(users) => users,
                // The team simply does not exist in this org.
                Err(GithubError::NotFound(_)) => continue,
                Err(e) => return Err(e),
            };
            for user in on_team {
                if let Some(member) = result.iter_mut().find(|m| m.user.id == user.id) {
                    member.teams.push(team.to_owned());
                }
            }
        }
        Ok(result)
    }

    /// One user's standing in the org, for the lazy sync when they open a
    /// workspace before the next full sync. `None` when they have no access.
    pub async fn org_member(
        &self,
        api: &str,
        token: &str,
        org: &str,
        login: &str,
    ) -> Result<Option<OrgMember>> {
        #[derive(Deserialize)]
        struct Membership {
            state: String,
            role: String,
            user: GhUser,
        }
        let membership: Option<Membership> = match self
            .get(api, &format!("/orgs/{org}/memberships/{login}"), token)
            .await
        {
            Ok(m) => Some(m),
            Err(GithubError::NotFound(_)) => None,
            Err(e) => return Err(e),
        };
        let (user, github_role) = match membership {
            Some(m) if m.state == "active" => {
                let role = if m.role == "admin" {
                    crate::roles::GITHUB_OWNER
                } else {
                    crate::roles::GITHUB_MEMBER
                };
                (m.user, role.to_owned())
            }
            _ => {
                let outside: Vec<GhUser> = self
                    .get_all(api, &format!("/orgs/{org}/outside_collaborators"), token)
                    .await?;
                match outside
                    .into_iter()
                    .find(|u| u.login.eq_ignore_ascii_case(login))
                {
                    Some(user) => (user, crate::roles::GITHUB_OUTSIDE_COLLABORATOR.to_owned()),
                    None => return Ok(None),
                }
            }
        };
        let mut teams = Vec::new();
        if github_role == crate::roles::GITHUB_MEMBER {
            #[derive(Deserialize)]
            struct TeamMembership {
                state: String,
            }
            for team in [crate::roles::TEAM_ADMINS, crate::roles::TEAM_APPROVERS] {
                let path = format!("/orgs/{org}/teams/{team}/memberships/{login}");
                match self.get::<TeamMembership>(api, &path, token).await {
                    Ok(m) if m.state == "active" => teams.push(team.to_owned()),
                    Ok(_) | Err(GithubError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(Some(OrgMember {
            user,
            github_role,
            teams,
        }))
    }

    // --- GitHub App -----------------------------------------------------------

    /// `GET /app/installations/{id}` with the app JWT: fills in what a bare
    /// `installation_id` (the post-install redirect) does not carry.
    pub async fn installation(&self, api: &str, jwt: &str, id: i64) -> Result<GhInstallation> {
        self.get(api, &format!("/app/installations/{id}"), jwt)
            .await
    }

    /// `POST /app/installations/{id}/access_tokens` with the app JWT.
    pub async fn installation_token(
        &self,
        api: &str,
        jwt: &str,
        id: i64,
    ) -> Result<InstallationToken> {
        #[derive(Deserialize)]
        struct Reply {
            token: String,
            expires_at: DateTime<Utc>,
        }
        let response = self
            .http
            .post(format!("{api}/app/installations/{id}/access_tokens"))
            .bearer_auth(jwt)
            .header(ACCEPT, "application/vnd.github+json")
            .send()
            .await?;
        let reply: Reply = decode(response).await?;
        Ok(InstallationToken {
            token: reply.token,
            expires_at: reply.expires_at,
        })
    }

    // --- Plumbing -------------------------------------------------------------

    async fn get<T: DeserializeOwned>(&self, api: &str, path: &str, token: &str) -> Result<T> {
        let response = self
            .http
            .get(format!("{api}{path}"))
            .bearer_auth(token)
            .header(ACCEPT, "application/vnd.github+json")
            .send()
            .await?;
        decode(response).await
    }

    /// Follows `page=` until a short page; `path` may already carry a query.
    async fn get_all<T: DeserializeOwned>(
        &self,
        api: &str,
        path: &str,
        token: &str,
    ) -> Result<Vec<T>> {
        let joiner = if path.contains('?') { '&' } else { '?' };
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let url = format!("{path}{joiner}per_page={PER_PAGE}&page={page}");
            let items: Vec<T> = self.get(api, &url, token).await?;
            let last = (items.len() as u32) < PER_PAGE;
            all.extend(items);
            if last {
                break;
            }
        }
        Ok(all)
    }
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    if status.is_success() {
        return response
            .json()
            .await
            .map_err(|e| GithubError::Decode(e.to_string()));
    }
    let message = response
        .text()
        .await
        .ok()
        .and_then(|body| {
            serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v["message"].as_str().map(str::to_owned))
                .or(Some(body))
        })
        .unwrap_or_default();
    Err(match status {
        StatusCode::UNAUTHORIZED => GithubError::Unauthorized,
        StatusCode::FORBIDDEN => GithubError::Forbidden(message),
        StatusCode::NOT_FOUND => GithubError::NotFound(message),
        other => GithubError::Api {
            status: other.as_u16(),
            message,
        },
    })
}

/// A GitHub App JWT (RS256, `iss` = app id), valid for nine minutes and
/// backdated a minute to survive clock skew.
pub fn app_jwt(app_id: &str, private_key_pem: &str) -> Result<String> {
    #[derive(Serialize)]
    struct Claims<'a> {
        iat: i64,
        exp: i64,
        iss: &'a str,
    }
    let now = Utc::now().timestamp();
    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .map_err(|e| GithubError::AppKey(e.to_string()))?;
    jsonwebtoken::encode(
        &Header::new(Algorithm::RS256),
        &Claims {
            iat: now - 60,
            exp: now + 9 * 60,
            iss: app_id,
        },
        &key,
    )
    .map_err(|e| GithubError::AppKey(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_jwt_rejects_a_key_that_is_not_rsa() {
        let err = app_jwt(
            "1",
            "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----",
        )
        .unwrap_err();
        assert!(matches!(err, GithubError::AppKey(_)), "{err}");
    }
}
