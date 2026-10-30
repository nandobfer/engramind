use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    thread,
};

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

use crate::store::StoreError;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

const QUEUE_CAPACITY: usize = 256;

/// Thread dona da única conexão de escrita (D-005). Toda escrita é serializada aqui.
#[derive(Clone)]
pub(super) struct Writer {
    sender: mpsc::Sender<Job>,
}

impl Writer {
    pub(super) fn spawn(mut connection: Connection) -> Result<Self, StoreError> {
        let (sender, mut receiver) = mpsc::channel::<Job>(QUEUE_CAPACITY);
        thread::Builder::new()
            .name("engramind-writer".to_owned())
            .spawn(move || {
                while let Some(job) = receiver.blocking_recv() {
                    job(&mut connection);
                }
            })
            .map_err(|error| StoreError::Internal(format!("thread escritora: {error}")))?;
        Ok(Self { sender })
    }

    /// Executa `job` na thread escritora. Um pânico no job vira `Internal` e a thread
    /// continua atendendo.
    pub(super) async fn run<R, F>(&self, job: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, StoreError> + Send + 'static,
    {
        let (reply, response) = oneshot::channel();
        let job: Job = Box::new(move |connection| {
            let result = catch_unwind(AssertUnwindSafe(|| job(connection)))
                .unwrap_or_else(|_| Err(StoreError::Internal("panico na escrita".to_owned())));
            let _ = reply.send(result);
        });
        self.sender
            .send(job)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        response.await.map_err(|_| StoreError::Unavailable)?
    }
}
