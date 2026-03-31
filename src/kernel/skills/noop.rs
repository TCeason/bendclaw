//! NoopSkillExecutor — returns error for all skill calls. For bendclaw-local.

use async_trait::async_trait;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::skills::executor::SkillExecutor;
use crate::kernel::skills::executor::SkillOutput;

pub struct NoopSkillExecutor;

#[async_trait]
impl SkillExecutor for NoopSkillExecutor {
    async fn execute(&self, skill_name: &str, _args: &[String]) -> Result<SkillOutput> {
        Err(ErrorCode::internal(format!(
            "skill '{skill_name}' is not available in local mode"
        )))
    }
}
