//! Code review (CONTRACT.md §20): pull requests of a project, starting a
//! review, deciding findings, publishing to GitHub, and the activity log.

use std::collections::HashMap;

use axum::{Json, http::StatusCode};
use sdi_proto::{intelligence::v1 as intelligence, knowledge::v1 as knowledge};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::reviews::{
        DecideFindingRequest, Finding, ListActivityQuery, ListPullRequestsQuery, ListReviewsQuery,
        PublishReviewRequest, PullRequest, PullRequestSummary, PullRequestsResponse, Review,
        ReviewActivity,
    },
    error::{ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_pull_requests))
        .routes(routes!(get_pull_request))
        .routes(routes!(start_review))
        .routes(routes!(list_reviews))
        .routes(routes!(get_review))
        .routes(routes!(decide_finding))
        .routes(routes!(publish_review))
        .routes(routes!(list_activity))
}

/// Pull requests of a project from the knowledge cache (synced from GitHub
/// on `refresh` or when empty), each with the id of its latest SDI review.
#[utoipa::path(
    get,
    path = "/projects/{id}/pull-requests",
    tag = "reviews",
    params(("id" = String, Path, description = "Project id"), ListPullRequestsQuery),
    responses((status = 200, body = PullRequestsResponse), ErrorResponses),
)]
async fn list_pull_requests(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ListPullRequestsQuery>,
) -> ApiResult<Json<PullRequestsResponse>> {
    let response = state
        .knowledge()
        .list_pull_requests(knowledge::ListPullRequestsRequest {
            project_id: id.clone(),
            state: query.state.unwrap_or_default(),
            refresh: query.refresh.unwrap_or(false),
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    let mut pull_requests: Vec<PullRequestSummary> = map_all(response.pull_requests);
    // One intelligence call fills `latestReviewId`: reviews come newest
    // first, so the first review seen per PR is its latest.
    if !pull_requests.is_empty() {
        let reviews = state
            .intelligence()
            .list_reviews(intelligence::ListReviewsRequest {
                project_id: id,
                limit: 200,
            })
            .await?
            .into_inner()
            .reviews;
        let latest = latest_review_ids(&reviews);
        for pr in &mut pull_requests {
            pr.latest_review_id = latest.get(&pr.number).cloned();
        }
    }
    Ok(Json(PullRequestsResponse {
        pull_requests,
        synced_at: crate::dto::opt_timestamp(response.synced_at.as_ref()),
    }))
}

/// The newest review id per pull request number.
pub(crate) fn latest_review_ids(reviews: &[intelligence::Review]) -> HashMap<u64, String> {
    let mut latest = HashMap::new();
    for review in reviews {
        latest
            .entry(review.pr_number)
            .or_insert_with(|| review.id.clone());
    }
    latest
}

/// One pull request with its changed files, bounded diff (≤ 300 KB, ≤ 200
/// files), touched facts and impacted graph nodes.
#[utoipa::path(
    get,
    path = "/projects/{id}/pull-requests/{number}",
    tag = "reviews",
    params(
        ("id" = String, Path, description = "Project id"),
        ("number" = u64, Path, description = "Pull request number"),
    ),
    responses((status = 200, body = PullRequest), ErrorResponses),
)]
async fn get_pull_request(
    state: Scoped,
    ApiPath((id, number)): ApiPath<(String, u64)>,
) -> ApiResult<Json<PullRequest>> {
    let pr = state
        .knowledge()
        .get_pull_request(knowledge::GetPullRequestRequest {
            project_id: id,
            number,
        })
        .await?
        .into_inner();
    Ok(Json(pr.into()))
}

/// Starts a review of the pull request at its current head commit and
/// returns at once; progress arrives as `review` events. Starting a review
/// that already exists for the head returns it (a failed one is retried).
#[utoipa::path(
    post,
    path = "/projects/{id}/pull-requests/{number}/reviews",
    tag = "reviews",
    params(
        ("id" = String, Path, description = "Project id"),
        ("number" = u64, Path, description = "Pull request number"),
    ),
    responses((status = 202, body = Review), ErrorResponses),
)]
async fn start_review(
    state: Scoped,
    ApiPath((id, number)): ApiPath<(String, u64)>,
) -> ApiResult<(StatusCode, Json<Review>)> {
    let review = state
        .intelligence()
        .start_review(intelligence::StartReviewRequest {
            project_id: id,
            pr_number: number,
        })
        .await?
        .into_inner();
    Ok((StatusCode::ACCEPTED, Json(review.into())))
}

/// Reviews, newest first, without their summary and findings.
#[utoipa::path(
    get,
    path = "/reviews",
    tag = "reviews",
    params(ListReviewsQuery),
    responses((status = 200, body = Vec<Review>), ErrorResponses),
)]
async fn list_reviews(
    state: Scoped,
    ApiQuery(query): ApiQuery<ListReviewsQuery>,
) -> ApiResult<Json<Vec<Review>>> {
    let response = state
        .intelligence()
        .list_reviews(intelligence::ListReviewsRequest {
            project_id: query.project_id.unwrap_or_default(),
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.reviews)))
}

/// One review with its rail, summary and findings.
#[utoipa::path(
    get,
    path = "/reviews/{id}",
    tag = "reviews",
    params(("id" = String, Path, description = "Review id")),
    responses((status = 200, body = Review), ErrorResponses),
)]
async fn get_review(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<Review>> {
    let review = state
        .intelligence()
        .get_review(intelligence::GetReviewRequest { id })
        .await?
        .into_inner();
    Ok(Json(review.into()))
}

/// Accepts or dismisses a finding. Only accepted findings are published.
#[utoipa::path(
    post,
    path = "/findings/{id}/decide",
    tag = "reviews",
    params(("id" = String, Path, description = "Finding id")),
    request_body = DecideFindingRequest,
    responses((status = 200, body = Finding), ErrorResponses),
)]
async fn decide_finding(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<DecideFindingRequest>,
) -> ApiResult<Json<Finding>> {
    let finding = state
        .intelligence()
        .decide_finding(intelligence::DecideFindingRequest {
            finding_id: id,
            decision: body.decision.into(),
            note: body.note.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(finding.into()))
}

/// Posts the review to GitHub: the summary as the body and one comment per
/// accepted finding (suggestions become GitHub suggestions). Never automatic.
#[utoipa::path(
    post,
    path = "/reviews/{id}/publish",
    tag = "reviews",
    params(("id" = String, Path, description = "Review id")),
    request_body = PublishReviewRequest,
    responses((status = 200, body = Review), ErrorResponses),
)]
async fn publish_review(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<PublishReviewRequest>>,
) -> ApiResult<Json<Review>> {
    let event = body.and_then(|ApiJson(b)| b.event).unwrap_or_default();
    let review = state
        .intelligence()
        .publish_review(intelligence::PublishReviewRequest {
            review_id: id,
            event,
        })
        .await?
        .into_inner();
    Ok(Json(review.into()))
}

/// The review activity log, newest first. Page with `before`.
#[utoipa::path(
    get,
    path = "/activity",
    tag = "reviews",
    params(ListActivityQuery),
    responses((status = 200, body = Vec<ReviewActivity>), ErrorResponses),
)]
async fn list_activity(
    state: Scoped,
    ApiQuery(query): ApiQuery<ListActivityQuery>,
) -> ApiResult<Json<Vec<ReviewActivity>>> {
    let response = state
        .intelligence()
        .list_review_activity(intelligence::ListReviewActivityRequest {
            project_id: query.project_id.unwrap_or_default(),
            limit: query.limit.unwrap_or(0),
            before: query.before.map(sdi_common::time::to_proto),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.activity)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_review_per_pull_request_is_the_first_listed() {
        let reviews = vec![
            intelligence::Review {
                id: "r-new".into(),
                pr_number: 1,
                ..Default::default()
            },
            intelligence::Review {
                id: "r-old".into(),
                pr_number: 1,
                ..Default::default()
            },
            intelligence::Review {
                id: "r-2".into(),
                pr_number: 2,
                ..Default::default()
            },
        ];
        let latest = latest_review_ids(&reviews);
        assert_eq!(latest.get(&1).map(String::as_str), Some("r-new"));
        assert_eq!(latest.get(&2).map(String::as_str), Some("r-2"));
        assert!(!latest.contains_key(&3));
    }
}
