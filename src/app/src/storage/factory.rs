use std::sync::Arc;

use crate::conf::StorageBackend;
use crate::conf::StorageConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::storage::fs::FsStorage;
use crate::storage::Storage;

pub fn open_storage(conf: &StorageConfig) -> Result<Arc<dyn Storage>> {
    match conf.backend {
        StorageBackend::Fs => Ok(Arc::new(FsStorage::new(conf.fs.root_dir.clone()))),
        StorageBackend::Cloud => Err(BendclawError::Store("cloud backend not implemented".into())),
    }
}
