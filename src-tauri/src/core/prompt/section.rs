use crate::model::errors::AppError;

use super::context::PromptBuildContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PromptPhase {
    Core,
    Capability,
    WorkspacePreference,
    RuntimeContext,
}

#[derive(Debug, Clone)]
pub struct PromptSection {
    pub key: &'static str,
    pub title: &'static str,
    pub body: String,
    pub phase: PromptPhase,
    pub order_in_phase: u16,
}

impl PromptSection {
    pub fn render(&self) -> String {
        format!("## {}\n{}", self.title, self.body)
    }

    pub fn is_empty(&self) -> bool {
        self.body.trim().is_empty()
    }
}

pub trait PromptSectionProvider {
    fn collect<'a>(
        &'a self,
        ctx: &'a PromptBuildContext<'a>,
    ) -> impl std::future::Future<Output = Result<Vec<PromptSection>, AppError>> + 'a;
}
