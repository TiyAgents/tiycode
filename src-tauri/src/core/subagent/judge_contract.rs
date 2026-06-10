use serde::{Deserialize, Serialize};

/// Input for the `agent_judge` tool (provided by the main agent).
#[derive(Debug, Clone)]
pub struct JudgeRequest {
    /// The main agent's note for this verification request. No longer
    /// injected into the Judge prompt — the Judge evaluates independently
    /// against goal + file system + task board. Parsed for backward
    /// compatibility but the value is discarded by execute_judge_tool.
    pub task: String,
}

impl JudgeRequest {
    pub fn from_tool_input(tool_input: &serde_json::Value) -> Result<Self, String> {
        let task = tool_input
            .get("task")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        // Task is now optional; an empty task string is valid.
        // The Judge does not receive the main agent's self-assessment.
        if task.is_empty() {
            return Ok(Self {
                task: "Goal acceptance verification".to_string(),
            });
        }

        Ok(Self { task })
    }
}

/// Structured verdict produced by the Judge subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgeReport {
    /// Whether the project currently satisfies the goal (acceptance passes).
    pub passed: bool,
    /// Completeness percentage 0-100.
    pub completeness_pct: u8,
    /// Specific unmet / non-conforming points. Required when `passed=false`.
    #[serde(default)]
    pub findings: Vec<String>,
    /// Rationale for the verdict. Used as completion evidence when `passed=true`.
    #[serde(default)]
    pub summary: String,
}

impl JudgeReport {
    /// Build a failed report carrying a single finding (used as a safe fallback
    /// when the Judge output cannot be parsed).
    fn failed_with_finding(finding: String) -> Self {
        Self {
            passed: false,
            completeness_pct: 0,
            findings: vec![finding],
            summary: String::new(),
        }
    }

    /// Normalize a parsed report so it can never represent an unverifiable
    /// acceptance:
    /// - `completeness_pct` is clamped to 0-100.
    /// - `passed=true` with an empty `summary` is downgraded to `passed=false`.
    /// - `passed=false` with no findings gets a placeholder finding.
    fn normalized(mut self) -> Self {
        if self.completeness_pct > 100 {
            self.completeness_pct = 100;
        }

        if self.passed && self.summary.trim().is_empty() {
            self.passed = false;
            self.findings
                .push("Judge reported passed=true but provided no summary/evidence; downgraded to not passed.".to_string());
        }

        if !self.passed && self.findings.is_empty() {
            self.findings
                .push("Judge did not provide actionable findings.".to_string());
        }

        self
    }
}

/// Parse the Judge's textual output into a `JudgeReport`. On any parse failure
/// the result is a *failed* report carrying the raw text as a finding, so a
/// malformed Judge response can never be mistaken for acceptance.
pub fn extract_judge_report(text: &str) -> JudgeReport {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return JudgeReport::failed_with_finding("Judge produced no output.".to_string());
    }

    if let Ok(report) = serde_json::from_str::<JudgeReport>(trimmed) {
        return report.normalized();
    }

    let stripped = strip_code_fence(trimmed);
    if let Ok(report) = serde_json::from_str::<JudgeReport>(stripped) {
        return report.normalized();
    }

    if let Some(report) = extract_embedded_json(trimmed) {
        return report.normalized();
    }

    JudgeReport::failed_with_finding(format!(
        "Judge output could not be parsed as a JudgeReport. Raw output: {trimmed}"
    ))
}

/// Render a parent-facing summary of the verdict for the main agent.
pub fn render_parent_summary(report: &JudgeReport) -> String {
    let mut lines = vec![format!(
        "Judge verdict: {} (completeness {}%)",
        if report.passed {
            "PASSED"
        } else {
            "NOT PASSED"
        },
        report.completeness_pct
    )];

    if !report.summary.trim().is_empty() {
        lines.push(format!("Summary: {}", report.summary.trim()));
    }

    if report.findings.is_empty() {
        lines.push("Findings:\n- none".to_string());
    } else {
        let rendered = report
            .findings
            .iter()
            .map(|f| format!("- {}", f.trim()))
            .collect::<Vec<_>>()
            .join("\n");
        lines.push(format!("Findings:\n{rendered}"));
    }

    if report.passed {
        lines.push(
            "✅ The goal has passed acceptance and is now marked complete. Stop making further changes and summarize the result.".to_string(),
        );
    } else {
        lines.push(
            "❌ The goal has NOT passed acceptance. Fix the findings above, then call agent_judge again to re-verify.".to_string(),
        );
    }

    lines.join("\n\n")
}

fn strip_code_fence(text: &str) -> &str {
    text.strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .or_else(|| {
            text.strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
                .map(str::trim)
        })
        .unwrap_or(text)
}

/// Best-effort: pull the first balanced `{...}` JSON object out of mixed prose
/// and try to parse it as a `JudgeReport`.
fn extract_embedded_json(text: &str) -> Option<JudgeReport> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let candidate = &text[start..=idx];
                    return serde_json::from_str::<JudgeReport>(candidate).ok();
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judge_request_empty_task_returns_default() {
        // Empty task is now valid; returns a default task string.
        let req = JudgeRequest::from_tool_input(&serde_json::json!({})).expect("empty task parses");
        assert_eq!(req.task, "Goal acceptance verification");
        let req = JudgeRequest::from_tool_input(&serde_json::json!({ "task": " verify it " }))
            .expect("parses");
        assert_eq!(req.task, "verify it");
    }

    #[test]
    fn extract_parses_plain_json() {
        let report = extract_judge_report(
            r#"{"passed":true,"completenessPct":100,"findings":[],"summary":"All tests pass."}"#,
        );
        assert!(report.passed);
        assert_eq!(report.completeness_pct, 100);
        assert_eq!(report.summary, "All tests pass.");
    }

    #[test]
    fn extract_parses_json_fence() {
        let report = extract_judge_report(
            "```json\n{\"passed\":false,\"completenessPct\":40,\"findings\":[\"missing tests\"],\"summary\":\"\"}\n```",
        );
        assert!(!report.passed);
        assert_eq!(report.completeness_pct, 40);
        assert_eq!(report.findings, vec!["missing tests"]);
    }

    #[test]
    fn extract_parses_embedded_json() {
        let report = extract_judge_report(
            "Here is my verdict:\n{\"passed\":true,\"completenessPct\":90,\"findings\":[],\"summary\":\"Looks good\"}\nThanks!",
        );
        assert!(report.passed);
        assert_eq!(report.summary, "Looks good");
    }

    #[test]
    fn malformed_output_is_not_passed() {
        let report = extract_judge_report("I think it's done, looks fine to me.");
        assert!(!report.passed);
        assert!(!report.findings.is_empty());
    }

    #[test]
    fn empty_output_is_not_passed() {
        let report = extract_judge_report("   ");
        assert!(!report.passed);
        assert!(!report.findings.is_empty());
    }

    #[test]
    fn passed_with_empty_summary_is_downgraded() {
        let report = extract_judge_report(
            r#"{"passed":true,"completenessPct":100,"findings":[],"summary":"   "}"#,
        );
        assert!(!report.passed);
        assert!(!report.findings.is_empty());
    }

    #[test]
    fn completeness_is_clamped() {
        let report = extract_judge_report(
            r#"{"passed":false,"completenessPct":250,"findings":["x"],"summary":""}"#,
        );
        assert_eq!(report.completeness_pct, 100);
    }

    #[test]
    fn failed_with_no_findings_gets_placeholder() {
        let report = extract_judge_report(
            r#"{"passed":false,"completenessPct":10,"findings":[],"summary":"incomplete"}"#,
        );
        assert!(!report.passed);
        assert_eq!(report.findings.len(), 1);
    }

    #[test]
    fn render_summary_includes_verdict_and_findings() {
        let report = extract_judge_report(
            r#"{"passed":false,"completenessPct":30,"findings":["A","B"],"summary":"not yet"}"#,
        );
        let summary = render_parent_summary(&report);
        assert!(summary.contains("NOT PASSED"));
        assert!(summary.contains("- A"));
        assert!(summary.contains("agent_judge again"));
    }
}
