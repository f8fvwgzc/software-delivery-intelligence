//! One error type for every handler. gRPC statuses, extractor rejections and
//! gateway-side failures all end up as the same `{code, message}` body so the
//! web app has a single thing to parse.

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, OptionalFromRequest, Path, Query, Request},
    http::{StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use serde::de::DeserializeOwned;
use utoipa::IntoResponses;

use crate::dto::common::ErrorBody;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_argument", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GATEWAY_TIMEOUT, "deadline_exceeded", message)
    }

    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            code: self.code.clone(),
            message: self.message.clone(),
        }
    }
}

/// The mapping table from CONTRACT.md §8. Internal causes are logged, not
/// forwarded, so a backend bug never leaks SQL or stack text to a browser.
impl From<tonic::Status> for ApiError {
    fn from(status: tonic::Status) -> Self {
        use tonic::Code;

        let message = status.message().to_owned();
        let (http, code) = match status.code() {
            Code::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Code::InvalidArgument => (StatusCode::BAD_REQUEST, "invalid_argument"),
            Code::AlreadyExists => (StatusCode::CONFLICT, "already_exists"),
            Code::FailedPrecondition => (StatusCode::PRECONDITION_FAILED, "failed_precondition"),
            Code::PermissionDenied => (StatusCode::FORBIDDEN, "permission_denied"),
            Code::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated"),
            Code::ResourceExhausted => (StatusCode::TOO_MANY_REQUESTS, "resource_exhausted"),
            Code::Unavailable => {
                tracing::warn!(error = %message, "upstream service unavailable");
                return Self::new(
                    StatusCode::BAD_GATEWAY,
                    "unavailable",
                    format!("upstream service unavailable: {message}"),
                );
            }
            Code::DeadlineExceeded => (StatusCode::GATEWAY_TIMEOUT, "deadline_exceeded"),
            other => {
                tracing::error!(code = ?other, error = %message, "upstream service failed");
                return Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal error",
                );
            }
        };
        Self::new(http, code, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body())).into_response()
    }
}

/// Error responses every route can answer with, for the OpenAPI document.
#[allow(dead_code)]
#[derive(IntoResponses)]
pub enum ErrorResponses {
    /// Malformed path, query or body, or a rejected argument.
    #[response(status = 400)]
    BadRequest(ErrorBody),
    /// The resource does not exist.
    #[response(status = 404)]
    NotFound(ErrorBody),
    /// The resource exists already.
    #[response(status = 409)]
    Conflict(ErrorBody),
    /// The resource is not in a state that allows this.
    #[response(status = 412)]
    PreconditionFailed(ErrorBody),
    /// A backing service is unreachable.
    #[response(status = 502)]
    BadGateway(ErrorBody),
    /// A backing service did not answer in time.
    #[response(status = 504)]
    GatewayTimeout(ErrorBody),
}

/// `Json<T>` whose rejection is the API error body with status 400 instead
/// of axum's plain-text 422/415.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match <Json<T> as FromRequest<S>>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_body",
                rejection.body_text(),
            )),
        }
    }
}

/// `Option<ApiJson<T>>`: a request without a body (no content type) is
/// `None`; a body that is present must still parse.
impl<S, T> OptionalFromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        if !request.headers().contains_key(header::CONTENT_TYPE) {
            return Ok(None);
        }
        <Self as FromRequest<S>>::from_request(request, state)
            .await
            .map(Some)
    }
}

/// `Query<T>` with the API error body on rejection.
pub struct ApiQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_query",
                rejection.body_text(),
            )),
        }
    }
}

/// `Path<T>` with the API error body on rejection.
pub struct ApiPath<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_path",
                rejection.body_text(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_codes_follow_the_contract_table() {
        let cases = [
            (tonic::Code::NotFound, StatusCode::NOT_FOUND, "not_found"),
            (
                tonic::Code::InvalidArgument,
                StatusCode::BAD_REQUEST,
                "invalid_argument",
            ),
            (
                tonic::Code::AlreadyExists,
                StatusCode::CONFLICT,
                "already_exists",
            ),
            (
                tonic::Code::FailedPrecondition,
                StatusCode::PRECONDITION_FAILED,
                "failed_precondition",
            ),
            (
                tonic::Code::PermissionDenied,
                StatusCode::FORBIDDEN,
                "permission_denied",
            ),
            (
                tonic::Code::Unavailable,
                StatusCode::BAD_GATEWAY,
                "unavailable",
            ),
            (
                tonic::Code::DeadlineExceeded,
                StatusCode::GATEWAY_TIMEOUT,
                "deadline_exceeded",
            ),
            (
                tonic::Code::Internal,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
            ),
            (
                tonic::Code::Unknown,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
            ),
        ];
        for (grpc, http, code) in cases {
            let error = ApiError::from(tonic::Status::new(grpc, "boom"));
            assert_eq!(error.status, http, "{grpc:?}");
            assert_eq!(error.code, code, "{grpc:?}");
        }
    }

    #[test]
    fn internal_errors_do_not_leak_their_message() {
        let error = ApiError::from(tonic::Status::internal("SELECT * FROM secrets"));
        assert_eq!(error.message, "internal error");
    }
}
