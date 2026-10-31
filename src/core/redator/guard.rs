//! Guarda de sentido (desenho §7): o texto revisado só vale quando a similaridade de embedding
//! com o original fica em `redator.min_similarity` ou acima. Sem guarda disponível, falha
//! fechada: só o título é aplicado (006 Q-001).

use std::{future::Future, pin::Pin};

pub type GuardFuture<'a> = Pin<Box<dyn Future<Output = Option<f64>> + Send + 'a>>;

pub trait MeaningGuard: Send + Sync {
    /// Similaridade de 0 a 1 entre o original e o revisado; `None` quando não há como medir.
    fn similarity<'a>(&'a self, original: &'a str, revised: &'a str) -> GuardFuture<'a>;
}

/// Sem embeddings (até a 007): nunca mede, e o texto revisado nunca é aplicado.
pub struct NoGuard;

impl MeaningGuard for NoGuard {
    fn similarity<'a>(&'a self, _original: &'a str, _revised: &'a str) -> GuardFuture<'a> {
        Box::pin(async { None })
    }
}
