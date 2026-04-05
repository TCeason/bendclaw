use std::path::PathBuf;
use std::sync::Arc;

use crate::conf::StoreBackend;
use crate::conf::StoreConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::store::fs::run::FsRunStore;
use crate::store::fs::session::FsSessionStore;
use crate::store::RunStore;
use crate::store::SessionStore;

pub struct Stores {
    pub session: Arc<dyn SessionStore>,
    pub run: Arc<dyn RunStore>,
}

fn fs_dirs(root_dir: PathBuf) -> (PathBuf, PathBuf) {
    (root_dir.join("sessions"), root_dir.join("runs"))
}

pub fn create_stores(conf: &StoreConfig) -> Result<Stores> {
    match conf.backend {
        StoreBackend::Fs => {
            let (session_dir, run_dir) = fs_dirs(conf.fs.root_dir.clone());
            Ok(Stores {
                session: Arc::new(FsSessionStore::new(session_dir)),
                run: Arc::new(FsRunStore::new(run_dir)),
            })
        }
        StoreBackend::Cloud => Err(BendclawError::Store("cloud backend not implemented".into())),
    }
}
