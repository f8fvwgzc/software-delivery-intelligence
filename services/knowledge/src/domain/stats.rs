//! Delivery and pattern statistics. Rates are over completed items only, so
//! queued or in-progress work never drags a success rate down.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunStats {
    pub total: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub success_rate: f64,
    pub avg_duration_seconds: u32,
}

/// `statuses` are the stored lowercase run statuses; durations line up by index.
pub fn pipeline_stats(statuses: &[&str], durations: &[u32]) -> RunStats {
    let mut stats = RunStats {
        total: statuses.len() as u32,
        ..RunStats::default()
    };
    let mut duration_sum = 0_u64;
    let mut completed = 0_u64;
    for (index, status) in statuses.iter().enumerate() {
        match *status {
            "succeeded" => stats.succeeded += 1,
            "failed" => stats.failed += 1,
            _ => continue,
        }
        completed += 1;
        duration_sum += u64::from(durations.get(index).copied().unwrap_or(0));
    }
    stats.success_rate = rate(stats.succeeded, stats.succeeded + stats.failed);
    stats.avg_duration_seconds = duration_sum.checked_div(completed).unwrap_or(0) as u32;
    stats
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeployStats {
    pub total: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub success_rate: f64,
}

pub fn deployment_stats(statuses: &[&str]) -> DeployStats {
    let succeeded = statuses.iter().filter(|s| **s == "succeeded").count() as u32;
    let failed = statuses.iter().filter(|s| **s == "failed").count() as u32;
    DeployStats {
        total: statuses.len() as u32,
        succeeded,
        failed,
        success_rate: rate(succeeded, succeeded + failed),
    }
}

/// `successes / total`, 0.0 when nothing completed.
pub fn rate(successes: u32, total: u32) -> f64 {
    if total == 0 {
        0.0
    } else {
        f64::from(successes) / f64::from(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_stats_ignore_incomplete_runs() {
        let stats = pipeline_stats(
            &[
                "succeeded",
                "failed",
                "in_progress",
                "succeeded",
                "cancelled",
            ],
            &[100, 50, 0, 200, 0],
        );
        assert_eq!(stats.total, 5);
        assert_eq!(stats.succeeded, 2);
        assert_eq!(stats.failed, 1);
        assert!((stats.success_rate - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(stats.avg_duration_seconds, 116);
    }

    #[test]
    fn empty_input_is_all_zero() {
        assert_eq!(pipeline_stats(&[], &[]), RunStats::default());
        assert_eq!(deployment_stats(&[]), DeployStats::default());
    }

    #[test]
    fn deployment_rate_over_completed_only() {
        let stats = deployment_stats(&["succeeded", "pending", "failed", "inactive"]);
        assert_eq!(stats.total, 4);
        assert!((stats.success_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn pattern_rate_is_successes_over_deployments() {
        assert_eq!(rate(3, 4), 0.75);
        assert_eq!(rate(0, 0), 0.0);
    }
}
