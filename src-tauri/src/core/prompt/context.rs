use sqlx::SqlitePool;

use crate::core::agent_session::RuntimeModelPlan;

#[derive(Debug, Clone)]
pub struct PromptBuildContext<'a> {
    pub pool: &'a SqlitePool,
    pub raw_plan: &'a RuntimeModelPlan,
    pub workspace_path: &'a str,
    pub run_mode: &'a str,
}

impl<'a> PromptBuildContext<'a> {
    pub fn new(
        pool: &'a SqlitePool,
        raw_plan: &'a RuntimeModelPlan,
        workspace_path: &'a str,
        run_mode: &'a str,
    ) -> Self {
        Self {
            pool,
            raw_plan,
            workspace_path,
            run_mode,
        }
    }
}
