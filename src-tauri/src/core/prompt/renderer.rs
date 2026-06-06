/// Section renderer trait - controls how (title, body) pairs are formatted
/// for a specific LLM provider. Different providers respond better to
/// different markup conventions (Markdown vs XML vs plain text).
pub trait SectionRenderer: Send + Sync {
    /// Render a single section from (title, body) to provider-preferred format.
    fn render_section(&self, title: &str, body: &str) -> String;

    /// Separator between layers (default: "\n\n").
    fn layer_separator(&self) -> &'static str {
        "\n\n"
    }

    /// Human-readable renderer name for audit trails.
    fn name(&self) -> &'static str;
}

/// Default renderer: uses `## title\n{body}` Markdown format.
/// Preserves byte-equal compatibility with the old system.
pub struct MarkdownRenderer;

impl SectionRenderer for MarkdownRenderer {
    fn render_section(&self, title: &str, body: &str) -> String {
        format!("## {}\n{}", title, body)
    }

    fn name(&self) -> &'static str {
        "markdown"
    }
}

/// XML renderer: uses `<section name="title">body</section>`.
/// Anthropic models show improved section recall with XML markup.
pub struct XmlRenderer;

impl SectionRenderer for XmlRenderer {
    fn render_section(&self, title: &str, body: &str) -> String {
        format!("<section name=\"{}\">\n{}\n</section>", title, body)
    }

    fn name(&self) -> &'static str {
        "xml"
    }
}
