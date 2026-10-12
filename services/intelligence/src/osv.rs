//! OSV lookups for the dependencies a pull request adds (CONTRACT.md §20).
//! Nothing is bundled: the answers come from `api.osv.dev` (`querybatch`
//! for the ids, `vulns/{id}` for the summary, severity and fixed version),
//! are cached per package coordinates for 24 h in `osv_cache`, and any
//! failure, including no network, simply means no finding from this check.

use std::{collections::BTreeMap, time::Duration};

use chrono::Utc;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::{
    db::osv::{self as cache, Vulnerability},
    domain::review::rules::{
        dependencies::AddedDependency,
        osv::{Coordinates, coordinates},
    },
};

pub const DEFAULT_BASE_URL: &str = "https://api.osv.dev";
/// `OSV_API_URL=off` disables the check.
pub const OFF: &str = "off";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Advisories whose details are fetched per review, at most.
const MAX_DETAILS: usize = 25;
/// Packages sent to OSV per review, at most.
const MAX_PACKAGES: usize = 100;

pub struct OsvClient {
    http: reqwest::Client,
    base_url: String,
    pool: PgPool,
}

impl OsvClient {
    /// `None` when the base URL is `off` or empty.
    pub fn new(base_url: &str, pool: PgPool) -> Option<Self> {
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.is_empty() || base_url.eq_ignore_ascii_case(OFF) {
            return None;
        }
        Some(Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client"),
            base_url: base_url.to_owned(),
            pool,
        })
    }

    /// Known vulnerabilities per added dependency: from the cache when
    /// fresh, from OSV otherwise. A dependency OSV could not be asked about
    /// is simply absent from the map.
    pub async fn check(
        &self,
        deps: &[AddedDependency],
    ) -> BTreeMap<Coordinates, Vec<Vulnerability>> {
        let mut answers: BTreeMap<Coordinates, Vec<Vulnerability>> = BTreeMap::new();
        let mut to_query: Vec<Coordinates> = Vec::new();
        let now = Utc::now();
        for dep in deps.iter().take(MAX_PACKAGES) {
            let coords = coordinates(dep);
            if answers.contains_key(&coords) || to_query.contains(&coords) {
                continue;
            }
            match cache::get(&self.pool, &coords.0, &coords.1, &coords.2).await {
                Ok(Some(row)) if row.fresh(now) => {
                    answers.insert(coords, row.vulns.0);
                }
                Ok(_) => to_query.push(coords),
                Err(error) => {
                    tracing::warn!(%error, "osv cache unavailable; asking OSV directly");
                    to_query.push(coords);
                }
            }
        }
        if to_query.is_empty() {
            return answers;
        }
        let ids = match self.query_batch(&to_query).await {
            Ok(ids) => ids,
            Err(error) => {
                tracing::warn!(%error, "osv query failed; skipping the vulnerability check");
                return answers;
            }
        };
        let mut details: BTreeMap<String, Value> = BTreeMap::new();
        let distinct: Vec<&String> = {
            let mut seen = Vec::new();
            for id in ids.iter().flatten() {
                if !seen.contains(&id) {
                    seen.push(id);
                }
            }
            seen
        };
        for id in distinct.into_iter().take(MAX_DETAILS) {
            match self.vulnerability(id).await {
                Ok(value) => {
                    details.insert(id.clone(), value);
                }
                Err(error) => tracing::warn!(%error, id, "osv advisory details unavailable"),
            }
        }
        for (coords, ids) in to_query.into_iter().zip(ids) {
            let vulns: Vec<Vulnerability> = ids
                .iter()
                .map(|id| match details.get(id) {
                    Some(value) => parse_vulnerability(id, value, &coords.1),
                    None => Vulnerability {
                        id: id.clone(),
                        ..Default::default()
                    },
                })
                .collect();
            if let Err(error) =
                cache::put(&self.pool, &coords.0, &coords.1, &coords.2, &vulns).await
            {
                tracing::warn!(%error, "could not cache the osv answer");
            }
            answers.insert(coords, vulns);
        }
        answers
    }

    /// `POST /v1/querybatch`: the advisory ids per query, in order.
    async fn query_batch(&self, coords: &[Coordinates]) -> anyhow::Result<Vec<Vec<String>>> {
        let queries: Vec<Value> = coords
            .iter()
            .map(|(ecosystem, name, version)| {
                json!({"package": {"name": name, "ecosystem": ecosystem}, "version": version})
            })
            .collect();
        let response = self
            .http
            .post(format!("{}/v1/querybatch", self.base_url))
            .json(&json!({"queries": queries}))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("osv returned {status}");
        }
        let body: Value = response.json().await?;
        let results = body["results"].as_array().cloned().unwrap_or_default();
        if results.len() != coords.len() {
            anyhow::bail!(
                "osv answered {} results for {} queries",
                results.len(),
                coords.len()
            );
        }
        Ok(results
            .iter()
            .map(|r| {
                r["vulns"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .filter_map(|x| x["id"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect())
    }

    async fn vulnerability(&self, id: &str) -> anyhow::Result<Value> {
        let response = self
            .http
            .get(format!("{}/v1/vulns/{id}", self.base_url))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("osv returned {status} for {id}");
        }
        Ok(response.json().await?)
    }
}

/// The parts of an OSV advisory the review cites. `severity` prefers the
/// database's own word (GHSA: CRITICAL/HIGH/MODERATE/LOW); `fixed` is the
/// first fixed version of a range that affects `package`.
pub fn parse_vulnerability(id: &str, value: &Value, package: &str) -> Vulnerability {
    let summary = value["summary"]
        .as_str()
        .or_else(|| value["details"].as_str())
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(160)
        .collect::<String>();
    let severity = value["database_specific"]["severity"]
        .as_str()
        .map(str::to_ascii_uppercase)
        .or_else(|| {
            value["severity"]
                .as_array()?
                .iter()
                .find_map(|s| s["score"].as_str())
                .and_then(cvss_word)
        })
        .unwrap_or_default();
    let fixed = value["affected"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| {
            a["package"]["name"]
                .as_str()
                .is_none_or(|n| n.eq_ignore_ascii_case(package))
        })
        .flat_map(|a| a["ranges"].as_array().cloned().unwrap_or_default())
        .flat_map(|r| r["events"].as_array().cloned().unwrap_or_default())
        .find_map(|e| e["fixed"].as_str().map(str::to_owned))
        .unwrap_or_default();
    Vulnerability {
        id: id.to_owned(),
        summary,
        severity,
        fixed,
    }
}

/// A CVSS vector string to the usual word, by its base metrics (coarse:
/// network + no privileges + high impact is HIGH or worse).
fn cvss_word(vector: &str) -> Option<String> {
    let v = vector.to_ascii_uppercase();
    if !v.starts_with("CVSS:") {
        return None;
    }
    let network = v.contains("/AV:N");
    let no_privileges = v.contains("/PR:N");
    let high_impact = v.contains("/C:H") || v.contains("/I:H") || v.contains("/A:H");
    Some(
        match (network, no_privileges, high_impact) {
            (true, true, true) => "CRITICAL",
            (true, _, true) | (_, true, true) => "HIGH",
            (_, _, true) | (true, _, _) => "MODERATE",
            _ => "LOW",
        }
        .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_fields_are_read_and_bounded() {
        let value = json!({
            "id": "GHSA-abcd",
            "summary": "Prototype pollution in left-pad\nsecond line",
            "database_specific": {"severity": "High"},
            "affected": [
                {"package": {"name": "other", "ecosystem": "npm"},
                 "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "9.9.9"}]}]},
                {"package": {"name": "left-pad", "ecosystem": "npm"},
                 "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "1.3.1"}]}]}
            ]
        });
        let vuln = parse_vulnerability("GHSA-abcd", &value, "left-pad");
        assert_eq!(vuln.id, "GHSA-abcd");
        assert_eq!(vuln.summary, "Prototype pollution in left-pad");
        assert_eq!(vuln.severity, "HIGH");
        assert_eq!(vuln.fixed, "1.3.1");

        let cvss = json!({
            "details": "Long details here",
            "severity": [{"type": "CVSS_V3", "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"}]
        });
        let vuln = parse_vulnerability("CVE-1", &cvss, "x");
        assert_eq!(vuln.summary, "Long details here");
        assert_eq!(vuln.severity, "CRITICAL");
        assert!(vuln.fixed.is_empty());
        assert_eq!(
            cvss_word("CVSS:3.1/AV:L/AC:L/PR:L/UI:N/S:U/C:L/I:N/A:N").as_deref(),
            Some("LOW")
        );
        assert!(cvss_word("not a vector").is_none());
    }

    #[tokio::test]
    async fn off_or_empty_disables_the_client() {
        let pool = sqlx::PgPool::connect_lazy("postgres://x:y@127.0.0.1:1/z").unwrap();
        assert!(OsvClient::new("off", pool.clone()).is_none());
        assert!(OsvClient::new("", pool.clone()).is_none());
        assert!(OsvClient::new("http://127.0.0.1:9/", pool).is_some());
    }
}
