use serde::{Deserialize, Serialize};

use crate::manifest::ChallengeManifest;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScoreRecord {
    pub reported_tokens_total: u64,
    pub estimated_tokens_total: u64,
    pub tool_calls_total: u64,
    pub shell_commands_total: u64,
    pub test_runs_total: u64,
    pub elapsed_active_time_seconds: u64,
    pub public_tests_passed: u64,
    pub public_tests_failed: u64,
    pub final_quality_score: f64,
    pub efficiency_score: f64,
}

pub fn calculate_score(record: &ScoreRecord, manifest: &ChallengeManifest) -> f64 {
    let scoring = &manifest.scoring;
    let public_total = record.public_tests_passed + record.public_tests_failed;
    let public_ratio = if public_total == 0 {
        0.0
    } else {
        record.public_tests_passed as f64 / public_total as f64
    };

    let quality_ratio = (record.final_quality_score / 100.0).clamp(0.0, 1.0);
    let normalized_efficiency = if record.efficiency_score > 0.0 {
        (record.efficiency_score / 100.0).clamp(0.0, 1.0)
    } else {
        let token_load = (record.reported_tokens_total + record.estimated_tokens_total) as f64 / 10_000.0;
        let tool_load = record.tool_calls_total as f64 / 100.0;
        let shell_load = record.shell_commands_total as f64 / 100.0;
        let time_load = record.elapsed_active_time_seconds as f64 / 3_600.0;
        (1.0 / (1.0 + token_load + tool_load + shell_load + time_load)).clamp(0.0, 1.0)
    };

    public_ratio * scoring.public_tests_weight as f64
        + quality_ratio * scoring.hidden_tests_weight as f64
        + normalized_efficiency * scoring.token_efficiency_weight as f64
}
