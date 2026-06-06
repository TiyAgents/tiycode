/// Snapshot tests for the Composer's rendered output across all surfaces.
///
/// These tests use `insta::assert_snapshot!` to capture the full system prompt
/// text produced by `Composer::build` for each PromptSurface. The snapshots
/// provide a safety net against accidental prompt drift and serve as a
/// human-readable audit trail for prompt content changes.
///
/// Ephemeral sections (ActiveGoal, ActivePlan) are excluded because their
/// content depends on thread-specific DB state not available in fixtures.
///
/// **Cross-platform normalisation**: Environment-dependent values (OS, arch,
/// shell, home directory, tmpdir) are replaced with stable placeholders
/// (`[os]`, `[arch]`, `[shell]`, `[HOME]`, `[TMPDIR]`) before snapshot
/// comparison so the same `.snap` files work on macOS, Linux, and any CI
/// runner regardless of the user account name.
#[cfg(test)]
mod tests {
    use super::super::budget::PromptBudget;
    use super::super::build_context::{BuildCx, ModelTarget};
    use super::super::clock::fixed_clock_for_test;
    use super::super::composer::{ComposedPrompt, Composer};
    use super::super::exec_policy::SourceExecPolicy;
    use super::super::redactor::NoopRedactor;
    use super::super::registry::default_registry;
    use super::super::renderer::MarkdownRenderer;
    use super::super::run_mode::RunMode;
    use super::super::signals::SignalCache;
    use super::super::surface::{CompactionKind, PromptSurface};

    use std::sync::Arc;

    use crate::persistence::init_database;

    /// Build a snapshot for a given surface using a fresh temp DB.
    async fn snap_surface(surface: PromptSurface, snapshot_name: &str) {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("snap.db");
        let pool = init_database(&db_path).await.expect("init db");

        // Use a fixed workspace path to keep snapshots deterministic.
        // Avoid /tmp prefix so normalisation doesn't collide with TMPDIR.
        let workspace: &'static str = "/tiycode-snap-workspace";

        let registry = Arc::new(default_registry());
        let composer = Composer::new(
            registry,
            SourceExecPolicy::default(),
            Arc::new(NoopRedactor),
        );

        let cx = BuildCx {
            pool: &pool,
            workspace_path: workspace,
            thread_id: None, // No thread → Ephemeral sections will Skip
            run_id: None,
            raw_plan: None,
            run_mode: RunMode::Default,
            helper_profile: None,
            custom_subagent_slug: None,
            target_model: ModelTarget::AnthropicClaude {
                context_window: 200_000,
                supports_cache_control: true,
            },
            clock: fixed_clock_for_test(),
            signals: Arc::new(SignalCache::new()),
            renderer: Arc::new(MarkdownRenderer),
            response_language: Some("English"),
        };

        let budget = PromptBudget::for_model(&cx.target_model, &surface);

        let mut composed = composer
            .build(&surface, &cx, &budget)
            .await
            .expect("composer build");

        // Normalise platform / user-account-dependent content so the
        // same snapshots pass on every OS and CI runner.
        composed.text = normalize_snapshot_text(&composed.text);

        let snapshot_text = format_audit_snapshot(&composed);
        insta::with_settings!({ snapshot_suffix => snapshot_name }, {
            insta::assert_snapshot!(snapshot_text);
        });
    }

    /// Replace host-dependent values with stable placeholders and strip the
    /// Skills section — the skills listing depends on locally-installed files
    /// that may not exist on CI or other developer machines.
    fn normalize_snapshot_text(text: &str) -> String {
        let mut text = strip_skills_section(text);

        let home = dirs::home_dir()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "/home/runner".to_string());
        let tmpdir = std::env::var("TMPDIR")
            .unwrap_or_else(|_| "/tmp".to_string())
            .trim_end_matches('/')
            .to_string();
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

        text.replace(&home, "[HOME]")
            .replace(&tmpdir, "[TMPDIR]")
            .replace(os, "[os]")
            .replace(arch, "[arch]")
            .replace(&shell, "[shell]")
    }

    /// Remove the `## Skills` section — its content depends on locally-
    /// installed skill files that vary across machines and CI runners.
    fn strip_skills_section(text: &str) -> String {
        let skills_header = "## Skills\n";
        if let Some(skills_start) = text.find(skills_header) {
            let before = &text[..skills_start];
            let after_skills = &text[skills_start + skills_header.len()..];
            // Find the next section header (starts with "\n## " after Skills).
            if let Some(next_section) = after_skills.find("\n## ") {
                let after = &after_skills[next_section..];
                return format!("{}{}", before, after);
            }
        }
        text.to_string()
    }

    /// Format the ComposedPrompt into a human-readable snapshot string.
    /// Audit `bytes` are replaced with `[snap]` because the same
    /// prompt text can have different byte counts on different platforms
    /// (e.g. "aarch64" → 7 bytes on macOS, "x86_64" → 6 bytes on Linux).
    fn format_audit_snapshot(composed: &ComposedPrompt) -> String {
        let mut out = String::new();
        out.push_str("=== COMPOSED PROMPT TEXT ===\n");
        out.push_str(&composed.text);
        out.push_str("\n\n=== AUDIT ===\n");
        out.push_str(&format!("schema_version: {}\n", composed.schema_version));
        for entry in &composed.audit {
            out.push_str(&format!(
                "id={:?} layer={:?} version={} bytes=[snap] tokens={} truncated={} renderer={}\n",
                entry.id,
                entry.layer,
                entry.version,
                entry.estimated_tokens,
                entry.truncated,
                entry.renderer,
            ));
        }
        if !composed.warnings.is_empty() {
            out.push_str("\n=== WARNINGS ===\n");
            for w in &composed.warnings {
                out.push_str(&format!("{w:?}\n"));
            }
        }
        out
    }

    // ── MainAgent ──────────────────────────────────────────────────

    #[tokio::test]
    async fn snapshot_main_agent_default() {
        snap_surface(
            PromptSurface::MainAgent {
                run_mode: RunMode::Default,
            },
            "main_agent_default",
        )
        .await;
    }

    #[tokio::test]
    async fn snapshot_main_agent_plan() {
        snap_surface(
            PromptSurface::MainAgent {
                run_mode: RunMode::Plan,
            },
            "main_agent_plan",
        )
        .await;
    }

    // ── Subagent ───────────────────────────────────────────────────

    #[tokio::test]
    async fn snapshot_subagent_explore() {
        snap_surface(
            PromptSurface::SubagentExplore {
                inherited_run_mode: RunMode::Default,
            },
            "subagent_explore",
        )
        .await;
    }

    #[tokio::test]
    async fn snapshot_subagent_review() {
        snap_surface(
            PromptSurface::SubagentReview {
                inherited_run_mode: RunMode::Default,
            },
            "subagent_review",
        )
        .await;
    }

    #[tokio::test]
    async fn snapshot_subagent_custom() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("snap.db");
        let pool = init_database(&db_path).await.expect("init db");
        // Avoid /tmp prefix so normalisation doesn't collide with TMPDIR.
        let workspace: &'static str = "/tiycode-snap-workspace";

        let registry = Arc::new(default_registry());
        let composer = Composer::new(
            registry,
            SourceExecPolicy::default(),
            Arc::new(NoopRedactor),
        );

        let surface = PromptSurface::SubagentCustom {
            slug: "my-custom-agent".into(),
            inherited_run_mode: RunMode::Default,
            cache_stability: crate::core::prompt::surface::SubagentCacheStability::Volatile,
        };

        let cx = BuildCx {
            pool: &pool,
            workspace_path: workspace,
            thread_id: None,
            run_id: None,
            raw_plan: None,
            run_mode: RunMode::Default,
            helper_profile: None,
            custom_subagent_slug: Some("my-custom-agent"),
            target_model: ModelTarget::AnthropicClaude {
                context_window: 200_000,
                supports_cache_control: true,
            },
            clock: fixed_clock_for_test(),
            signals: Arc::new(SignalCache::new()),
            renderer: Arc::new(MarkdownRenderer),
            response_language: Some("English"),
        };

        let budget = PromptBudget::for_model(&cx.target_model, &surface);
        let mut composed = composer
            .build(&surface, &cx, &budget)
            .await
            .expect("composer build");

        // Normalise before snapshot comparison (same as snap_surface).
        composed.text = normalize_snapshot_text(&composed.text);

        let snapshot_text = format_audit_snapshot(&composed);
        insta::with_settings!({ snapshot_suffix => "subagent_custom" }, {
            insta::assert_snapshot!(snapshot_text);
        });
    }

    // ── Compaction ─────────────────────────────────────────────────

    #[tokio::test]
    async fn snapshot_compaction_compact() {
        snap_surface(
            PromptSurface::Compaction {
                kind: CompactionKind::Compact,
            },
            "compaction_compact",
        )
        .await;
    }

    #[tokio::test]
    async fn snapshot_compaction_merge() {
        snap_surface(
            PromptSurface::Compaction {
                kind: CompactionKind::Merge,
            },
            "compaction_merge",
        )
        .await;
    }

    // ── Title ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn snapshot_title() {
        snap_surface(PromptSurface::Title, "title").await;
    }

    // ── Schema version consistency ─────────────────────────────────

    #[test]
    fn snapshot_schema_version_is_stable() {
        let registry = default_registry();
        assert_eq!(registry.schema_version(), 3);
        insta::assert_snapshot!("schema_version", registry.schema_version());
    }
}
