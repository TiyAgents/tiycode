use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTarget {
    #[default]
    Code,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewScope {
    Local,
    DiffFirstGlobal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalScanMode {
    Off,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Pass,
    Fail,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
    Skipped,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequest {
    pub task: String,
    pub target: ReviewTarget,
    pub review_scope: ReviewScope,
    pub global_scan_mode: GlobalScanMode,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub preferred_checks: Vec<String>,
    #[serde(default)]
    pub risk_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFinding {
    pub title: String,
    pub severity: FindingSeverity,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub command: String,
    pub status: VerificationStatus,
    pub summary: String,
    #[serde(default)]
    pub key_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageNote {
    pub diff_reviewed: bool,
    pub global_scan_performed: bool,
    #[serde(default)]
    pub changed_files_reviewed: Vec<String>,
    #[serde(default)]
    pub scanned_paths: Vec<String>,
    #[serde(default)]
    pub unscanned_paths: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewReport {
    pub verdict: ReviewVerdict,
    #[serde(default)]
    pub direct_findings: Vec<ReviewFinding>,
    #[serde(default)]
    pub global_findings: Vec<ReviewFinding>,
    #[serde(default)]
    pub verification: Vec<VerificationResult>,
    pub coverage: CoverageNote,
    #[serde(default)]
    pub follow_up: Vec<String>,
}

impl ReviewRequest {
    pub fn from_tool_input(tool_input: &serde_json::Value) -> Result<Self, String> {
        let task = tool_input
            .get("task")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        if task.is_empty() {
            return Err("missing helper task".to_string());
        }

        let target = parse_target(tool_input.get("target"))?;
        let review_scope = parse_scope(tool_input.get("reviewScope"), target)?;
        let global_scan_mode = parse_global_scan(tool_input.get("globalScanMode"), review_scope)?;

        Ok(Self {
            task,
            target,
            review_scope,
            global_scan_mode,
            changed_files: read_string_array(tool_input.get("changedFiles")),
            preferred_checks: read_string_array(tool_input.get("preferredChecks")),
            risk_hints: read_string_array(tool_input.get("riskHints")),
        })
    }

    pub fn to_helper_prompt(&self) -> String {
        let changed_files = format_list(&self.changed_files, "discover from git_status + git_diff");
        let preferred_checks = format_list(
            &self.preferred_checks,
            "infer from the repository instructions, scripts, and build files",
        );
        let risk_hints = format_list(&self.risk_hints, "none provided");

        format!(
            "Review request:
- task: {task}
- target: {target}
- review_scope: {review_scope}
- global_scan_mode: {global_scan_mode}
- changed_files: {changed_files}
- preferred_checks: {preferred_checks}
- risk_hints: {risk_hints}

Execution rules:
- Adapt your review to the active repository instead of assuming a specific framework, language, or test runner.
- If target=diff, start from the current workspace changes. Use `git_status` to enumerate changed files and `git_diff` to inspect exact diffs whenever changed_files is empty or incomplete.
- Review direct diff risks first. Focus on correctness, regressions, edge cases, error handling, and consistency with existing patterns.
- If review_scope=diff_first_global and global_scan_mode=auto, perform a limited global impact probe after the direct diff review.
- Keep the global impact probe bounded: inspect at most one dependency hop and at most 8 additional files unless a smaller set is sufficient.
- Use the global probe for shared exports, public interfaces, schemas, persistence boundaries, runtime commands, configuration, cross-platform paths, or tests that may be affected by the diff.
- Run the necessary verification commands for this repository. Prefer preferred_checks when provided; otherwise infer the right type-check and test commands from workspace instructions, package scripts, manifests, and build files.
- If verification or the global scan cannot be completed, record that honestly in coverage.limitations or verification with status=skipped/not_run.

Return exactly one JSON object with this contract:
{{
  \"verdict\": \"pass|fail|needs_attention\",
  \"directFindings\": [
    {{
      \"title\": \"short title\",
      \"severity\": \"critical|high|medium|low\",
      \"path\": \"optional/path\",
      \"line\": null,
      \"summary\": \"what is wrong and why it matters\",
      \"evidence\": [\"specific evidence\"],
      \"suggestion\": \"optional concrete fix\"
    }}
  ],
  \"globalFindings\": [
    {{
      \"title\": \"short title\",
      \"severity\": \"critical|high|medium|low\",
      \"path\": \"optional/path\",
      \"line\": null,
      \"summary\": \"diff-adjacent system risk\",
      \"evidence\": [\"specific evidence\"],
      \"suggestion\": \"optional concrete fix\"
    }}
  ],
  \"verification\": [
    {{
      \"command\": \"command string\",
      \"status\": \"passed|failed|skipped|not_run\",
      \"summary\": \"short outcome summary\",
      \"keyOutput\": \"important output excerpt if helpful\"
    }}
  ],
  \"coverage\": {{
    \"diffReviewed\": true,
    \"globalScanPerformed\": true,
    \"changedFilesReviewed\": [\"path\"],
    \"scannedPaths\": [\"path\"],
    \"unscannedPaths\": [\"path\"],
    \"limitations\": [\"what could not be completed\"]
  }},
  \"followUp\": [\"exact next step for the parent agent or user\"]
}}

Do not wrap the JSON in markdown fences and do not add prose before or after it.",
            task = self.task,
            target = review_target_label(self.target),
            review_scope = review_scope_label(self.review_scope),
            global_scan_mode = global_scan_mode_label(self.global_scan_mode),
            changed_files = changed_files,
            preferred_checks = preferred_checks,
            risk_hints = risk_hints,
        )
    }
}

pub fn extract_review_report(text: &str) -> Option<ReviewReport> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(report) = serde_json::from_str::<ReviewReport>(trimmed) {
        return Some(report);
    }

    let stripped = strip_code_fence(trimmed);
    serde_json::from_str::<ReviewReport>(stripped).ok()
}

pub fn render_parent_summary(report: &ReviewReport) -> String {
    let mut lines = vec![format!("Verdict: {}", verdict_label(report.verdict))];

    lines.push(render_findings_section(
        "Direct Diff Findings",
        &report.direct_findings,
    ));
    lines.push(render_findings_section(
        "Global Impact Findings",
        &report.global_findings,
    ));
    lines.push(render_verification_section(&report.verification));
    lines.push(render_coverage_section(&report.coverage));
    lines.push(render_follow_up_section(&report.follow_up));

    lines.join("\n\n")
}

fn parse_target(value: Option<&serde_json::Value>) -> Result<ReviewTarget, String> {
    match value.and_then(serde_json::Value::as_str).unwrap_or("code") {
        "code" => Ok(ReviewTarget::Code),
        "diff" => Ok(ReviewTarget::Diff),
        other => Err(format!("invalid review target: {other}")),
    }
}

fn parse_scope(
    value: Option<&serde_json::Value>,
    target: ReviewTarget,
) -> Result<ReviewScope, String> {
    match value.and_then(serde_json::Value::as_str) {
        Some("local") => Ok(ReviewScope::Local),
        Some("diff_first_global") => Ok(ReviewScope::DiffFirstGlobal),
        Some(other) => Err(format!("invalid review scope: {other}")),
        None if target == ReviewTarget::Diff => Ok(ReviewScope::DiffFirstGlobal),
        None => Ok(ReviewScope::Local),
    }
}

fn parse_global_scan(
    value: Option<&serde_json::Value>,
    review_scope: ReviewScope,
) -> Result<GlobalScanMode, String> {
    match value.and_then(serde_json::Value::as_str) {
        Some("off") => Ok(GlobalScanMode::Off),
        Some("auto") => Ok(GlobalScanMode::Auto),
        Some(other) => Err(format!("invalid global scan mode: {other}")),
        None if review_scope == ReviewScope::DiffFirstGlobal => Ok(GlobalScanMode::Auto),
        None => Ok(GlobalScanMode::Off),
    }
}

fn read_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .take(100)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
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

fn render_findings_section(title: &str, findings: &[ReviewFinding]) -> String {
    if findings.is_empty() {
        return format!("{title}:\n- none");
    }

    let lines = findings
        .iter()
        .map(|finding| {
            let path = finding.path.as_deref().unwrap_or("unknown path");
            let location = finding
                .line
                .map(|line| format!("{path}:{line}"))
                .unwrap_or_else(|| path.to_string());
            let suggestion = if finding.suggestion.trim().is_empty() {
                String::new()
            } else {
                format!(" Fix: {}", finding.suggestion.trim())
            };
            format!(
                "- [{}] {} at {}. {}{}",
                severity_label(finding.severity),
                finding.title.trim(),
                location,
                finding.summary.trim(),
                suggestion
            )
        })
        .collect::<Vec<_>>();

    format!("{title}:\n{}", lines.join("\n"))
}

fn render_verification_section(verification: &[VerificationResult]) -> String {
    if verification.is_empty() {
        return "Verification Results:\n- none recorded".to_string();
    }

    let lines = verification
        .iter()
        .map(|item| {
            let extra = if item.key_output.trim().is_empty() {
                String::new()
            } else {
                format!(" Output: {}", item.key_output.trim())
            };
            format!(
                "- [{}] `{}`: {}{}",
                verification_status_label(item.status),
                item.command.trim(),
                item.summary.trim(),
                extra
            )
        })
        .collect::<Vec<_>>();

    format!("Verification Results:\n{}", lines.join("\n"))
}

fn render_coverage_section(coverage: &CoverageNote) -> String {
    let mut lines = vec![
        format!("- diff_reviewed: {}", yes_no(coverage.diff_reviewed)),
        format!(
            "- global_scan_performed: {}",
            yes_no(coverage.global_scan_performed)
        ),
        format!(
            "- changed_files_reviewed: {}",
            format_list(&coverage.changed_files_reviewed, "none recorded")
        ),
        format!(
            "- scanned_paths: {}",
            format_list(&coverage.scanned_paths, "none recorded")
        ),
    ];

    if !coverage.unscanned_paths.is_empty() {
        lines.push(format!(
            "- unscanned_paths: {}",
            coverage.unscanned_paths.join(", ")
        ));
    }

    if !coverage.limitations.is_empty() {
        lines.push(format!(
            "- limitations: {}",
            coverage.limitations.join(" | ")
        ));
    }

    format!("Coverage Notes:\n{}", lines.join("\n"))
}

fn render_follow_up_section(follow_up: &[String]) -> String {
    if follow_up.is_empty() {
        return "Parent Follow-up:\n- none".to_string();
    }

    format!("Parent Follow-up:\n- {}", follow_up.join("\n- "))
}

fn format_list(items: &[String], empty_label: &str) -> String {
    if items.is_empty() {
        empty_label.to_string()
    } else {
        items.join(", ")
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn review_target_label(target: ReviewTarget) -> &'static str {
    match target {
        ReviewTarget::Code => "code",
        ReviewTarget::Diff => "diff",
    }
}

fn review_scope_label(scope: ReviewScope) -> &'static str {
    match scope {
        ReviewScope::Local => "local",
        ReviewScope::DiffFirstGlobal => "diff_first_global",
    }
}

fn global_scan_mode_label(mode: GlobalScanMode) -> &'static str {
    match mode {
        GlobalScanMode::Off => "off",
        GlobalScanMode::Auto => "auto",
    }
}

fn verdict_label(verdict: ReviewVerdict) -> &'static str {
    match verdict {
        ReviewVerdict::Pass => "PASS",
        ReviewVerdict::Fail => "FAIL",
        ReviewVerdict::NeedsAttention => "NEEDS ATTENTION",
    }
}

fn severity_label(severity: FindingSeverity) -> &'static str {
    match severity {
        FindingSeverity::Critical => "CRITICAL",
        FindingSeverity::High => "HIGH",
        FindingSeverity::Medium => "MEDIUM",
        FindingSeverity::Low => "LOW",
    }
}

fn verification_status_label(status: VerificationStatus) -> &'static str {
    match status {
        VerificationStatus::Passed => "passed",
        VerificationStatus::Failed => "failed",
        VerificationStatus::Skipped => "skipped",
        VerificationStatus::NotRun => "not_run",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_review_report, render_parent_summary, GlobalScanMode, ReviewRequest, ReviewScope,
        ReviewTarget, ReviewVerdict,
    };

    #[test]
    fn review_request_defaults_to_diff_first_global_for_diff_target() {
        let request = ReviewRequest::from_tool_input(&serde_json::json!({
            "task": "review the current changes",
            "target": "diff"
        }))
        .expect("request should parse");

        assert_eq!(request.target, ReviewTarget::Diff);
        assert_eq!(request.review_scope, ReviewScope::DiffFirstGlobal);
        assert_eq!(request.global_scan_mode, GlobalScanMode::Auto);
    }

    #[test]
    fn review_request_keeps_local_scope_for_code_target() {
        let request = ReviewRequest::from_tool_input(&serde_json::json!({
            "task": "review the implementation",
            "target": "code"
        }))
        .expect("request should parse");

        assert_eq!(request.target, ReviewTarget::Code);
        assert_eq!(request.review_scope, ReviewScope::Local);
        assert_eq!(request.global_scan_mode, GlobalScanMode::Off);
    }

    #[test]
    fn extract_review_report_accepts_json_fence() {
        let report = extract_review_report(
            r#"```json
{"verdict":"pass","directFindings":[],"globalFindings":[],"verification":[],"coverage":{"diffReviewed":true,"globalScanPerformed":false,"changedFilesReviewed":[],"scannedPaths":[],"unscannedPaths":[],"limitations":[]},"followUp":[]}
```"#,
        )
        .expect("report should parse");

        assert_eq!(report.verdict, ReviewVerdict::Pass);
    }

    #[test]
    fn review_request_honors_explicit_scope_scan_and_arrays() {
        let request = ReviewRequest::from_tool_input(&serde_json::json!({
            "task": "review with explicit knobs",
            "target": "diff",
            "reviewScope": "local",
            "globalScanMode": "off",
            "changedFiles": ["src/a.ts", " ", "", "src/b.ts"],
            "preferredChecks": ["npm run typecheck", "   ", "cargo test"],
            "riskHints": ["cross_platform", "", " persistence "]
        }))
        .expect("request should parse");

        assert_eq!(request.target, ReviewTarget::Diff);
        assert_eq!(request.review_scope, ReviewScope::Local);
        assert_eq!(request.global_scan_mode, GlobalScanMode::Off);
        assert_eq!(request.changed_files, vec!["src/a.ts", "src/b.ts"]);
        assert_eq!(
            request.preferred_checks,
            vec!["npm run typecheck", "cargo test"]
        );
        assert_eq!(request.risk_hints, vec!["cross_platform", "persistence"]);
    }

    #[test]
    fn review_request_rejects_missing_task_and_invalid_enums() {
        assert!(ReviewRequest::from_tool_input(&serde_json::json!({})).is_err());
        assert!(ReviewRequest::from_tool_input(&serde_json::json!({
            "task": "review",
            "target": "unknown"
        }))
        .is_err());
        assert!(ReviewRequest::from_tool_input(&serde_json::json!({
            "task": "review",
            "reviewScope": "wide"
        }))
        .is_err());
        assert!(ReviewRequest::from_tool_input(&serde_json::json!({
            "task": "review",
            "globalScanMode": "sometimes"
        }))
        .is_err());
    }

    #[test]
    fn extract_review_report_rejects_malformed_or_incomplete_inputs() {
        assert!(extract_review_report("").is_none());
        assert!(extract_review_report("   ").is_none());
        assert!(extract_review_report("{not json").is_none());
        assert!(extract_review_report(r#"{"directFindings":[]}"#).is_none());
        assert!(extract_review_report("```json\n{\"verdict\":\"pass\"}").is_none());
        assert!(
            extract_review_report("before\n```json\n{\"verdict\":\"pass\"}\n```\nafter").is_none()
        );
    }

    #[test]
    fn render_parent_summary_includes_direct_and_global_sections() {
        let report = extract_review_report(
            r#"{"verdict":"needs_attention","directFindings":[{"title":"Missing guard","severity":"high","path":"src/a.ts","line":9,"summary":"Can panic on empty input.","evidence":["new code assumes at least one item"],"suggestion":"Guard against empty input."}],"globalFindings":[{"title":"Shared type contract changed","severity":"medium","path":"src/shared.ts","line":null,"summary":"Downstream callers may rely on the old shape.","evidence":["exported interface changed"],"suggestion":"Review dependent call sites."}],"verification":[{"command":"npm run typecheck","status":"passed","summary":"Typecheck passed","keyOutput":""}],"coverage":{"diffReviewed":true,"globalScanPerformed":true,"changedFilesReviewed":["src/a.ts"],"scannedPaths":["src/a.ts","src/shared.ts"],"unscannedPaths":[],"limitations":[]},"followUp":["none"]}"#,
        )
        .expect("report should parse");

        let summary = render_parent_summary(&report);
        assert!(summary.contains("Verdict: NEEDS ATTENTION"));
        assert!(summary.contains("Direct Diff Findings"));
        assert!(summary.contains("Global Impact Findings"));
        assert!(summary.contains("npm run typecheck"));
    }
}
