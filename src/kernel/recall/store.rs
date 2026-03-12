use crate::storage::dal::knowledge::KnowledgeRepo;
use crate::storage::dal::learning::LearningRepo;
use crate::storage::pool::Pool;

#[derive(Clone)]
pub struct RecallStore {
    knowledge: KnowledgeRepo,
    learnings: LearningRepo,
}

impl RecallStore {
    pub fn new(pool: Pool) -> Self {
        Self {
            knowledge: KnowledgeRepo::new(pool.clone()),
            learnings: LearningRepo::new(pool),
        }
    }

    pub fn knowledge(&self) -> &KnowledgeRepo {
        &self.knowledge
    }

    pub fn learnings(&self) -> &LearningRepo {
        &self.learnings
    }
}
