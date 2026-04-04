use crate::model::errors::AppError;

use super::context::PromptBuildContext;
use super::providers::{BaseProvider, EnvironmentProvider, ProfileProvider, WorkspaceProvider};
use super::section::{PromptSection, PromptSectionProvider};

pub async fn build_system_prompt(
    pool: &sqlx::SqlitePool,
    raw_plan: &crate::core::agent_session::RuntimeModelPlan,
    workspace_path: &str,
    run_mode: &str,
) -> Result<String, AppError> {
    let ctx = PromptBuildContext::new(pool, raw_plan, workspace_path, run_mode);

    let mut sections: Vec<PromptSection> = Vec::new();
    sections.extend(BaseProvider.collect(&ctx).await?);
    sections.extend(WorkspaceProvider.collect(&ctx).await?);
    sections.extend(EnvironmentProvider.collect(&ctx).await?);
    sections.extend(ProfileProvider.collect(&ctx).await?);

    sections.retain(|section: &PromptSection| !section.is_empty());
    sections.sort_by_key(|section| (section.phase, section.order_in_phase));

    Ok(sections
        .into_iter()
        .map(|section: PromptSection| section.render())
        .collect::<Vec<_>>()
        .join("\n\n"))
}
