//! SkillService — thin command orchestrator.
//!
//! Write: store + projector.reconcile().
//! Read: projector (single visibility boundary).
//! Usage tracking: store.

use std::sync::Arc;

use crate::base::Result;
use crate::kernel::skills::projector::SkillProjector;
use crate::kernel::skills::shared::SharedSkillStore;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillId;
use crate::kernel::subscriptions::SubscriptionStore;

pub struct SkillService {
    store: Arc<dyn SharedSkillStore>,
    sub_store: Arc<dyn SubscriptionStore>,
    projector: Arc<SkillProjector>,
}

impl SkillService {
    pub fn new(
        store: Arc<dyn SharedSkillStore>,
        sub_store: Arc<dyn SubscriptionStore>,
        projector: Arc<SkillProjector>,
    ) -> Self {
        Self {
            store,
            sub_store,
            projector,
        }
    }

    // ── Write: DB + reconcile ───────────────────────────────────────────

    pub async fn create(&self, user_id: &str, skill: Skill) -> Result<()> {
        skill.validate()?;
        self.store.save(user_id, &skill).await?;
        if let Err(e) = self.projector.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_service", "reconcile_after_create_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }

    pub async fn delete(&self, user_id: &str, name: &str) -> Result<()> {
        self.store.remove(user_id, name).await?;
        if let Err(e) = self.projector.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_service", "reconcile_after_delete_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }

    // ── Read: all via projector (single visibility boundary) ────────────

    pub fn list(&self, user_id: &str) -> Vec<Skill> {
        self.projector.visible_skills(user_id)
    }

    pub fn get(&self, user_id: &str, name: &str) -> Option<Skill> {
        self.projector.resolve(user_id, name)
    }

    pub fn resolve(&self, user_id: &str, tool_key: &str) -> Option<Skill> {
        self.projector.resolve(user_id, tool_key)
    }

    pub fn read_skill(&self, user_id: &str, path: &str) -> Option<String> {
        self.projector.read_skill(user_id, path)
    }

    pub fn host_script_path(&self, user_id: &str, tool_key: &str) -> Option<std::path::PathBuf> {
        self.projector.host_script_path(user_id, tool_key)
    }

    pub fn script_path(&self, user_id: &str, tool_key: &str) -> Option<String> {
        self.projector.script_path(user_id, tool_key)
    }

    pub fn projector(&self) -> &Arc<SkillProjector> {
        &self.projector
    }

    // ── Shared browsing + subscriptions ─────────────────────────────────

    pub async fn list_shared(&self, user_id: &str) -> Result<Vec<Skill>> {
        self.store.list_shared(user_id).await
    }

    pub async fn subscribe(&self, user_id: &str, skill_name: &str, owner_id: &str) -> Result<()> {
        self.sub_store
            .subscribe(user_id, "skill", skill_name, owner_id)
            .await?;
        if let Err(e) = self.projector.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_service", "reconcile_after_subscribe_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }

    pub async fn unsubscribe(&self, user_id: &str, skill_name: &str, owner_id: &str) -> Result<()> {
        self.sub_store
            .unsubscribe(user_id, "skill", skill_name, owner_id)
            .await?;
        if let Err(e) = self.projector.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_service", "reconcile_after_unsubscribe_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }

    // ── Usage tracking ──────────────────────────────────────────────────

    pub fn touch_used(&self, id: SkillId, agent_id: String) {
        let store = self.store.clone();
        crate::base::spawn_fire_and_forget("skill_touch", async move {
            let _ = store.touch_last_used(&id, &agent_id).await;
        });
    }
}
