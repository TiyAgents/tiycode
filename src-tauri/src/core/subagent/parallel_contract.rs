use serde::{Deserialize, Serialize};

use crate::core::subagent::review_contract::{ReviewReport, ReviewRequest};
use crate::core::subagent::SubagentProgressSnapshot;

pub const PARALLEL_SUBAGENT_DEFAULT_CONCURRENCY: usize = 2;
pub const PARALLEL_SUBAGENT_MAX_CONCURRENCY: usize = 3;
pub const PARALLEL_SUBAGENT_MAX_TASKS: usize = 3;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSubagentRequest {
    pub tasks: Vec<ParallelSubagentTask>,
    #[serde(default)]
    pub max_concurrency: Option<usize>,
    #[serde(default)]
    pub fail_fast: bool,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub result_format: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSubagentTask {
    pub agent: String,
    pub task: String,
    #[serde(default)]
    pub target: Option<serde_json::Value>,
    #[serde(default)]
    pub review_scope: Option<serde_json::Value>,
    #[serde(default)]
    pub global_scan_mode: Option<serde_json::Value>,
    #[serde(default)]
    pub changed_files: Option<Vec<String>>,
    #[serde(default)]
    pub preferred_checks: Option<Vec<String>>,
    #[serde(default)]
    pub risk_hints: Option<Vec<String>>,
    #[serde(default)]
    pub plan_file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSubagentTaskResult {
    pub index: usize,
    pub agent: String,
    pub task: String,
    pub status: ParallelSubagentTaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SubagentProgressSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_request: Option<ReviewRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_report: Option<ReviewReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParallelSubagentTaskStatus {
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParallelSubagentBatchStatus {
    Completed,
    PartialFailure,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSubagentSummary {
    pub summary: String,
    pub mode: Option<String>,
    pub result_format: Option<String>,
    pub batch_status: ParallelSubagentBatchStatus,
    pub max_concurrency: usize,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
    pub results: Vec<ParallelSubagentTaskResult>,
}

impl ParallelSubagentRequest {
    pub fn from_tool_input(tool_input: &serde_json::Value) -> Result<Self, String> {
        let request: Self = serde_json::from_value(tool_input.clone())
            .map_err(|error| format!("invalid agent_parallel request: {error}"))?;
        request.validate()?;
        Ok(request)
    }

    pub fn effective_max_concurrency(&self) -> usize {
        self.max_concurrency
            .unwrap_or(PARALLEL_SUBAGENT_DEFAULT_CONCURRENCY)
            .clamp(1, PARALLEL_SUBAGENT_MAX_CONCURRENCY)
            .min(self.tasks.len().max(1))
    }

    fn validate(&self) -> Result<(), String> {
        if self.tasks.is_empty() {
            return Err("agent_parallel requires at least one task".to_string());
        }
        if self.tasks.len() > PARALLEL_SUBAGENT_MAX_TASKS {
            return Err(format!(
                "agent_parallel supports at most {PARALLEL_SUBAGENT_MAX_TASKS} tasks"
            ));
        }
        if let Some(max_concurrency) = self.max_concurrency {
            if max_concurrency == 0 {
                return Err("agent_parallel maxConcurrency must be at least 1".to_string());
            }
            if max_concurrency > PARALLEL_SUBAGENT_MAX_CONCURRENCY {
                return Err(format!(
                    "agent_parallel maxConcurrency must be <= {PARALLEL_SUBAGENT_MAX_CONCURRENCY}"
                ));
            }
        }
        for (index, task) in self.tasks.iter().enumerate() {
            task.validate(index)?;
        }
        Ok(())
    }
}

impl ParallelSubagentTask {
    pub fn to_tool_input(&self) -> serde_json::Value {
        let mut input = serde_json::Map::new();
        input.insert(
            "task".to_string(),
            serde_json::Value::String(self.task.clone()),
        );
        if let Some(value) = &self.target {
            input.insert("target".to_string(), value.clone());
        }
        if let Some(value) = &self.review_scope {
            input.insert("reviewScope".to_string(), value.clone());
        }
        if let Some(value) = &self.global_scan_mode {
            input.insert("globalScanMode".to_string(), value.clone());
        }
        if let Some(value) = &self.changed_files {
            input.insert("changedFiles".to_string(), serde_json::json!(value));
        }
        if let Some(value) = &self.preferred_checks {
            input.insert("preferredChecks".to_string(), serde_json::json!(value));
        }
        if let Some(value) = &self.risk_hints {
            input.insert("riskHints".to_string(), serde_json::json!(value));
        }
        if let Some(value) = &self.plan_file_path {
            input.insert(
                "planFilePath".to_string(),
                serde_json::Value::String(value.clone()),
            );
        }
        serde_json::Value::Object(input)
    }

    fn validate(&self, index: usize) -> Result<(), String> {
        if self.agent.trim().is_empty() {
            return Err(format!("agent_parallel task {index} is missing agent"));
        }
        if self.agent == "agent_parallel" {
            return Err(format!(
                "agent_parallel task {index} cannot delegate to agent_parallel recursively"
            ));
        }
        if self.task.trim().is_empty() {
            return Err(format!("agent_parallel task {index} is missing task"));
        }
        Ok(())
    }
}

pub fn render_parallel_summary(summary: &ParallelSubagentSummary) -> String {
    let mut lines = vec![format!(
        "Parallel subagents completed: {}/{} succeeded, {} failed, {} skipped (max concurrency {}).",
        summary.completed, summary.total, summary.failed, summary.skipped, summary.max_concurrency
    )];

    for result in &summary.results {
        match result.status {
            ParallelSubagentTaskStatus::Completed => {
                lines.push(format!(
                    "{}. {} succeeded: {}",
                    result.index + 1,
                    result.agent,
                    result
                        .summary
                        .as_deref()
                        .unwrap_or("completed without summary")
                ));
            }
            ParallelSubagentTaskStatus::Failed => {
                lines.push(format!(
                    "{}. {} failed: {}",
                    result.index + 1,
                    result.agent,
                    result.error.as_deref().unwrap_or("unknown error")
                ));
            }
            ParallelSubagentTaskStatus::Skipped => {
                lines.push(format!(
                    "{}. {} skipped: {}",
                    result.index + 1,
                    result.agent,
                    result.error.as_deref().unwrap_or("skipped")
                ));
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_parallel_request_bounds() {
        let request = ParallelSubagentRequest::from_tool_input(&serde_json::json!({
            "tasks": [
                { "agent": "agent_explore", "task": "Explore backend" },
                { "agent": "agent_review", "task": "Review diff", "target": "diff" }
            ],
            "maxConcurrency": 2
        }))
        .expect("request should be valid");

        assert_eq!(request.effective_max_concurrency(), 2);
        assert_eq!(request.tasks[1].to_tool_input()["target"], "diff");
    }

    #[test]
    fn rejects_parallel_request_with_too_many_tasks() {
        let error = ParallelSubagentRequest::from_tool_input(&serde_json::json!({
            "tasks": [
                { "agent": "agent_explore", "task": "one" },
                { "agent": "agent_explore", "task": "two" },
                { "agent": "agent_explore", "task": "three" },
                { "agent": "agent_explore", "task": "four" }
            ]
        }))
        .expect_err("request should be rejected");

        assert!(error.contains("at most 3 tasks"));
    }

    #[test]
    fn rejects_recursive_parallel_task() {
        let error = ParallelSubagentRequest::from_tool_input(&serde_json::json!({
            "tasks": [
                { "agent": "agent_parallel", "task": "recurse" }
            ]
        }))
        .expect_err("request should be rejected");

        assert!(error.contains("recursively"));
    }
}
