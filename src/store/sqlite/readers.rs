use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::Connection;
use tokio::sync::Semaphore;

use crate::store::StoreError;

const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Pool próprio de conexões somente leitura (D-005).
#[derive(Clone)]
pub(super) struct Readers {
    inner: Arc<Inner>,
}

struct Inner {
    connections: Mutex<Vec<Connection>>,
    permits: Arc<Semaphore>,
}

impl Readers {
    pub(super) fn new(connections: Vec<Connection>) -> Self {
        let permits = Arc::new(Semaphore::new(connections.len()));
        Self {
            inner: Arc::new(Inner {
                connections: Mutex::new(connections),
                permits,
            }),
        }
    }

    pub(super) async fn run<R, F>(&self, job: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&Connection) -> Result<R, StoreError> + Send + 'static,
    {
        let permit =
            tokio::time::timeout(ACQUIRE_TIMEOUT, self.inner.permits.clone().acquire_owned())
                .await
                .map_err(|_| StoreError::Unavailable)?
                .map_err(|_| StoreError::Unavailable)?;
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let connection = inner
                .connections
                .lock()
                .map_err(|_| StoreError::Unavailable)?
                .pop()
                .ok_or(StoreError::Unavailable)?;
            let result = catch_unwind(AssertUnwindSafe(|| job(&connection)))
                .unwrap_or_else(|_| Err(StoreError::Internal("panico na leitura".to_owned())));
            if let Ok(mut connections) = inner.connections.lock() {
                connections.push(connection);
            }
            drop(permit);
            result
        })
        .await
        .map_err(|_| StoreError::Unavailable)?
    }
}
