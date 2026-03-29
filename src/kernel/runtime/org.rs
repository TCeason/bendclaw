//! OrgServices — aggregates org-level (evotai_meta) shared services.

use std::sync::Arc;

use crate::kernel::memory::store::SharedMemoryStore;
use crate::kernel::memory::MemoryService;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::skills::projector::SkillProjector;
use crate::kernel::skills::service::SkillService;
use crate::kernel::skills::shared::DatabendSharedSkillStore;
use crate::kernel::subscriptions::SharedSubscriptionStore;
use crate::kernel::subscriptions::SubscriptionStore;
use crate::kernel::variables::service::VariableService;
use crate::kernel::variables::store::SharedVariableStore;
use crate::llm::provider::LLMProvider;
use crate::storage::pool::Pool;

pub struct OrgServices {
    variables: Arc<VariableService>,
    skills: Arc<SkillService>,
    memory: Option<Arc<MemoryService>>,
    subscriptions: Arc<dyn SubscriptionStore>,
}

impl OrgServices {
    pub fn new(
        meta_pool: Pool,
        projector: Arc<SkillProjector>,
        config: &AgentConfig,
        llm: Arc<dyn LLMProvider>,
    ) -> Self {
        let sub_store: Arc<dyn SubscriptionStore> =
            Arc::new(SharedSubscriptionStore::new(meta_pool.clone()));

        let variable_store = Arc::new(SharedVariableStore::new(meta_pool.clone()));
        let variables = Arc::new(VariableService::new(variable_store, sub_store.clone()));

        let skill_store = Arc::new(DatabendSharedSkillStore::new(meta_pool.clone()));
        let skills = Arc::new(SkillService::new(skill_store, sub_store.clone(), projector));

        let memory = if config.memory.enabled {
            let store = Arc::new(SharedMemoryStore::new(meta_pool));
            let model: Arc<str> = llm.default_model().into();
            Some(Arc::new(MemoryService::new(store, llm, model)))
        } else {
            None
        };

        Self {
            variables,
            skills,
            memory,
            subscriptions: sub_store,
        }
    }

    pub fn variables(&self) -> &Arc<VariableService> {
        &self.variables
    }

    pub fn skills(&self) -> &Arc<SkillService> {
        &self.skills
    }

    pub fn memory(&self) -> Option<&Arc<MemoryService>> {
        self.memory.as_ref()
    }

    pub fn subscriptions(&self) -> &Arc<dyn SubscriptionStore> {
        &self.subscriptions
    }
}
