use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use super::pending_decision::PendingDecision;
use super::turn_relation::RunSnapshot;
use super::turn_relation::StubClassifier;
use super::turn_relation::TurnRelationClassifier;

pub struct TurnCoordinatorState {
    run_snapshots: RwLock<HashMap<String, RunSnapshot>>,
    pending_decisions: RwLock<HashMap<String, PendingDecision>>,
    classifier: RwLock<Arc<dyn TurnRelationClassifier>>,
}

impl Default for TurnCoordinatorState {
    fn default() -> Self {
        Self {
            run_snapshots: RwLock::new(HashMap::new()),
            pending_decisions: RwLock::new(HashMap::new()),
            classifier: RwLock::new(Arc::new(StubClassifier)),
        }
    }
}

impl TurnCoordinatorState {
    pub fn store_snapshot(&self, session_id: &str, snapshot: RunSnapshot) {
        self.run_snapshots
            .write()
            .insert(session_id.to_string(), snapshot);
    }

    pub fn get_snapshot(&self, session_id: &str) -> Option<RunSnapshot> {
        self.run_snapshots.read().get(session_id).cloned()
    }

    pub fn remove_snapshot(&self, session_id: &str) {
        self.run_snapshots.write().remove(session_id);
    }

    pub fn store_decision(&self, decision: PendingDecision) {
        self.pending_decisions
            .write()
            .insert(decision.session_id.clone(), decision);
    }

    pub fn get_decision(&self, session_id: &str) -> Option<PendingDecision> {
        self.pending_decisions.read().get(session_id).cloned()
    }

    pub fn remove_decision(&self, session_id: &str) {
        self.pending_decisions.write().remove(session_id);
    }

    pub fn classifier(&self) -> Arc<dyn TurnRelationClassifier> {
        self.classifier.read().clone()
    }

    pub fn set_classifier(&self, classifier: Arc<dyn TurnRelationClassifier>) {
        *self.classifier.write() = classifier;
    }
}
