use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Template variables for placeholder substitution.
pub struct TemplateVars {
    vars: HashMap<&'static str, String>,
    user_text_keys: HashSet<&'static str>,
}

impl TemplateVars {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
            user_text_keys: HashSet::new(),
        }
    }

    /// Insert a regular variable value.
    pub fn insert(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.vars.insert(key, value.into());
        self
    }

    /// Insert user-provided text. {{...}} inside the value is NOT expanded.
    pub fn insert_user_text(mut self, key: &'static str, value: impl Into<String>) -> Self {
        let value_str = value.into();
        // Escape {{ and }} in user text to prevent template expansion
        let escaped = value_str
            .replace("{{", "\\x7B\\x7B")
            .replace("}}", "\\x7D\\x7D");
        self.vars.insert(key, escaped);
        self.user_text_keys.insert(key);
        self
    }
}

impl Default for TemplateVars {
    fn default() -> Self {
        Self::new()
    }
}

/// Error during template rendering.
#[derive(Debug)]
pub enum TemplateError {
    /// A declared key is missing from the variables
    MissingKey { key: &'static str },
    /// A variable is declared but not used in the template
    UnusedKey { key: &'static str },
    /// Front-matter parsing failed
    InvalidFrontMatter { message: String },
    /// Version mismatch between template front-matter and SectionSpec
    VersionMismatch {
        section_id: String,
        template_version: u32,
        spec_version: u32,
    },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::MissingKey { key } => write!(f, "missing template key: {}", key),
            TemplateError::UnusedKey { key } => write!(f, "unused template key: {}", key),
            TemplateError::InvalidFrontMatter { message } => {
                write!(f, "invalid front-matter: {}", message)
            }
            TemplateError::VersionMismatch {
                section_id,
                template_version,
                spec_version,
            } => write!(
                f,
                "version mismatch for {}: template={}, spec={}",
                section_id, template_version, spec_version
            ),
        }
    }
}

impl std::error::Error for TemplateError {}

/// Parsed template with front-matter metadata.
#[derive(Debug)]
pub struct Template {
    /// Section ID declared in front-matter
    pub section_id: String,
    /// Version declared in front-matter
    pub version: u32,
    /// Declared placeholder keys (must be superset of code-declared keys)
    pub declared_keys: Vec<String>,
    /// Template body (after front-matter stripped)
    pub body: String,
}

/// Load a template file. In debug builds, reads from disk for hot-reload;
/// otherwise uses the compile-time embedded string.
pub fn load_template(rel_path: &str, embedded: &'static str) -> Cow<'static, str> {
    #[cfg(debug_assertions)]
    {
        let template_root = template_root();
        let path = template_root.join(rel_path);
        if let Ok(s) = std::fs::read_to_string(&path) {
            return Cow::Owned(s);
        }
    }
    Cow::Borrowed(embedded)
}

/// Render a template with strict key checking.
/// Returns error if a declared key is missing.
pub fn render_template_strict(
    tpl: &str,
    declared_keys: &[&'static str],
    vars: &TemplateVars,
) -> Result<String, TemplateError> {
    for key in declared_keys {
        if !vars.vars.contains_key(key) {
            return Err(TemplateError::MissingKey { key });
        }
    }

    let mut result = tpl.to_string();
    for (key, value) in &vars.vars {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }

    // Restore escaped user text
    result = result
        .replace("\\x7B\\x7B", "{{")
        .replace("\\x7D\\x7D", "}}");

    Ok(result)
}

/// Parse YAML front-matter from a template string.
/// Returns (Template, body_without_front_matter).
pub fn parse_front_matter(raw: &str) -> Result<(Template, String), TemplateError> {
    let raw = raw.trim_start();
    if !raw.starts_with("---") {
        // No front-matter; return defaults
        return Ok((
            Template {
                section_id: String::new(),
                version: 1,
                declared_keys: Vec::new(),
                body: String::new(),
            },
            raw.to_string(),
        ));
    }

    // Find the closing ---
    let after_first = &raw[3..];
    let end = after_first.find("\n---").unwrap_or(0);
    if end == 0 {
        // No closing ---; treat all as body
        return Ok((
            Template {
                section_id: String::new(),
                version: 1,
                declared_keys: Vec::new(),
                body: String::new(),
            },
            raw.to_string(),
        ));
    }

    let front = &after_first[..end];
    let body = after_first[end + 4..].trim_start().to_string();

    // Simple YAML parsing (avoid full serde_yaml dependency for now)
    let mut section_id = String::new();
    let mut version = 1u32;
    let mut declared_keys = Vec::new();

    for line in front.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "section_id" => section_id = value.to_string(),
                "version" => {
                    version = value
                        .parse()
                        .map_err(|_| TemplateError::InvalidFrontMatter {
                            message: format!("invalid version: {}", value),
                        })?
                }
                "declared_keys" => {
                    // Parse YAML list: [key1, key2]
                    let list_str = value.trim_start_matches('[').trim_end_matches(']');
                    for item in list_str.split(',') {
                        let item = item.trim().trim_matches('"').trim_matches('\'');
                        if !item.is_empty() {
                            declared_keys.push(item.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok((
        Template {
            section_id,
            version,
            declared_keys,
            body: String::new(),
        },
        body,
    ))
}

fn template_root() -> PathBuf {
    // In dev, templates are relative to the prompt module directory
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(manifest_dir)
        .join("src")
        .join("core")
        .join("prompt")
        .join("templates")
}

/// Tokenizer trait for estimating token counts.
pub trait Tokenizer: Send + Sync {
    fn estimate(&self, text: &str) -> usize;
    fn name(&self) -> &'static str;
}

/// Default heuristic tokenizer: chars / 4.
pub struct HeuristicTokenizer;

impl Tokenizer for HeuristicTokenizer {
    fn estimate(&self, text: &str) -> usize {
        text.chars().count() / 4
    }

    fn name(&self) -> &'static str {
        "heuristic"
    }
}

// ── TemplateSource ──────────────────────────────────────────────────

use async_trait::async_trait;

use super::build_context::BuildCx;
use super::section_source::{FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource};

/// A SectionSource backed by a markdown template file with `{{key}}` placeholders.
pub struct TemplateSource<F>
where
    F: Fn(&BuildCx<'_>) -> Result<TemplateVars, FatalError> + Send + Sync,
{
    rel_path: &'static str,
    embedded: &'static str,
    declared_keys: &'static [&'static str],
    resolve_fn: F,
    spec_version: u32,
}

impl<F> TemplateSource<F>
where
    F: Fn(&BuildCx<'_>) -> Result<TemplateVars, FatalError> + Send + Sync,
{
    pub fn new(
        rel_path: &'static str,
        embedded: &'static str,
        declared_keys: &'static [&'static str],
        resolve_fn: F,
        spec_version: u32,
    ) -> Self {
        Self {
            rel_path,
            embedded,
            declared_keys,
            resolve_fn,
            spec_version,
        }
    }
}

#[async_trait]
impl<F> SectionSource for TemplateSource<F>
where
    F: Fn(&BuildCx<'_>) -> Result<TemplateVars, FatalError> + Send + Sync,
{
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let raw = load_template(self.rel_path, self.embedded);
        let (tmpl, body) = parse_front_matter(&raw)
            .map_err(|e| FatalError::new("template.parse", format!("{}: {}", self.rel_path, e)))?;

        // § 3.20: template front-matter version must match SectionSpec.version
        if tmpl.version != self.spec_version {
            return Err(FatalError::new(
                "template.version_mismatch",
                format!(
                    "{}: template front-matter version {} != spec version {}",
                    self.rel_path, tmpl.version, self.spec_version
                ),
            ));
        }

        if body.trim().is_empty() {
            return Ok(SectionOutcome::Skip);
        }

        let vars = (self.resolve_fn)(cx)?;

        let rendered = if self.declared_keys.is_empty() {
            body
        } else {
            render_template_strict(&body, self.declared_keys, &vars).map_err(|e| {
                FatalError::new("template.render", format!("{}: {}", self.rel_path, e))
            })?
        };

        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered,
            meta: SectionMeta {
                template_path: Some(self.rel_path),
                ..Default::default()
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_template_strict_missing_key() {
        let tpl = "Hello {{name}}!";
        let vars = TemplateVars::new();
        let result = render_template_strict(tpl, &["name"], &vars);
        assert!(result.is_err());
    }

    #[test]
    fn test_render_template_strict_success() {
        let tpl = "Hello {{name}}!";
        let vars = TemplateVars::new().insert("name", "World");
        let result = render_template_strict(tpl, &["name"], &vars).unwrap();
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_user_text_not_expanded() {
        let tpl = "Value: {{content}}";
        let vars = TemplateVars::new().insert_user_text("content", "{{secret}}");
        let result = render_template_strict(tpl, &["content"], &vars).unwrap();
        assert_eq!(result, "Value: {{secret}}");
    }

    #[test]
    fn test_parse_front_matter() {
        let raw = "---\nsection_id: Role\nversion: 5\n---\nYou are an AI agent.";
        let (template, body) = parse_front_matter(raw).unwrap();
        assert_eq!(template.section_id, "Role");
        assert_eq!(template.version, 5);
        assert_eq!(body, "You are an AI agent.");
    }

    #[test]
    fn template_version_sync() {
        let samples: &[(&str, u32, &str)] = &[
            ("---\nsection_id: Role\nversion: 1\n---\nBody", 1, "Role"),
            (
                "---\nsection_id: ProjectContext\nversion: 2\n---\nBody",
                2,
                "ProjectContext",
            ),
        ];

        for (raw, expected_version, expected_id) in samples {
            let (tmpl, _body) = parse_front_matter(raw)
                .unwrap_or_else(|e| panic!("failed to parse front-matter: {}", e));
            assert_eq!(
                tmpl.version, *expected_version,
                "version mismatch for section_id={}",
                expected_id
            );
            assert_eq!(tmpl.section_id, *expected_id, "section_id mismatch");
        }
    }

    /// Lint: every `{{key}}` placeholder in a template body must be declared
    /// in the front-matter `declared_keys`, and every declared key must appear
    /// in the body at least once. This prevents silently-ignored typos and
    /// stale declarations after template edits.
    #[test]
    fn templates_have_no_undeclared_keys() {
        let template_dir = super::template_root();
        assert!(
            template_dir.exists(),
            "template directory not found: {}",
            template_dir.display()
        );

        let mut failures = Vec::new();
        visit_templates(&template_dir, &mut failures);

        if !failures.is_empty() {
            panic!(
                "{} template key declaration mismatch(es):\n{}",
                failures.len(),
                failures.join("\n")
            );
        }
    }

    /// Recursively walk the template directory and check each .md file.
    fn visit_templates(dir: &std::path::Path, failures: &mut Vec<String>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
            panic!("failed to read template dir {}: {}", dir.display(), e);
        });

        for entry in entries {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                visit_templates(&path, failures);
            } else if path.extension().map_or(false, |ext| ext == "md") {
                check_template(&path, failures);
            }
        }
    }

    /// Validate a single template file: declared_keys must match actual {{key}} usage.
    fn check_template(path: &std::path::Path, failures: &mut Vec<String>) {
        let raw = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("failed to read template {}: {}", path.display(), e);
        });

        let (template, body) = match super::parse_front_matter(&raw) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("  {} — parse error: {}", path.display(), e));
                return;
            }
        };

        let actual_keys = extract_placeholders(&body);
        let declared_keys: Vec<String> = template.declared_keys;

        let rel_path = path
            .strip_prefix(super::template_root())
            .unwrap_or(path)
            .display();

        // Forward: every declared key must appear in the body
        for declared in &declared_keys {
            if !actual_keys.contains(declared) {
                failures.push(format!(
                    "  {} — declared key '{}' not found in template body",
                    rel_path, declared
                ));
            }
        }

        // Reverse: every {{key}} in body must be declared in front-matter
        for actual in &actual_keys {
            if !declared_keys.contains(actual) {
                failures.push(format!(
                    "  {} — undeclared key '{}' used in body (add to front-matter declared_keys)",
                    rel_path, actual
                ));
            }
        }
    }

    /// Extract all `{{key}}` placeholder names from a template body.
    fn extract_placeholders(body: &str) -> Vec<String> {
        let mut keys = Vec::new();
        let mut i = 0;
        let bytes = body.as_bytes();

        while i < bytes.len() {
            if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                // Found "{{"
                let start = i + 2;
                // Look for "}}"
                if let Some(end) = body[start..].find("}}") {
                    let key = body[start..start + end].trim();
                    if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        keys.push(key.to_string());
                    }
                    i = start + end + 2;
                    continue;
                }
            }
            i += 1;
        }

        keys.sort();
        keys.dedup();
        keys
    }
}
