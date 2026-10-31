//! Embeddings por hash, determinísticos, para os testes (desenho §21): cada palavra cai num
//! balde; palavras de um mesmo "conceito" caem no mesmo balde e por isso ficam próximas no
//! vetor. Nunca é usado em produção: só entra por `Core::set_local_embedder`.

use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::{EmbedError, EmbedFuture, Embedder, local};

const DEFAULT_DIMS: usize = 64;

#[derive(Default)]
pub struct HashEmbedder {
    concepts: BTreeMap<String, String>,
    dims: BTreeMap<String, usize>,
    failure: Mutex<Option<EmbedError>>,
    calls: AtomicUsize,
}

impl HashEmbedder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Palavras (sem acentos, minúsculas) tratadas como a mesma.
    pub fn concept(mut self, words: &[&str]) -> Self {
        if let Some(first) = words.first() {
            for word in words {
                self.concepts.insert(normalize(word), normalize(first));
            }
        }
        self
    }

    /// Dimensão devolvida para `model` (padrão: a do modelo local, senão 64).
    pub fn dims(mut self, model: &str, dims: usize) -> Self {
        self.dims.insert(model.to_owned(), dims);
        self
    }

    /// Faz as próximas chamadas falharem (`None` volta ao normal).
    pub fn fail(&self, error: Option<EmbedError>) {
        if let Ok(mut failure) = self.failure.lock() {
            *failure = error;
        }
    }

    /// Chamadas recebidas.
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn dims_for(&self, model: &str) -> usize {
        self.dims
            .get(model)
            .copied()
            .or_else(|| local::find(model).map(|model| model.dims))
            .unwrap_or(DEFAULT_DIMS)
    }

    fn vector(&self, text: &str, dims: usize) -> Vec<f32> {
        let text = text
            .strip_prefix("query: ")
            .or_else(|| text.strip_prefix("passage: "))
            .unwrap_or(text);
        let mut vector = vec![0.0_f32; dims];
        for word in normalize(text).split(' ').filter(|word| !word.is_empty()) {
            let concept = self.concepts.get(word).map_or(word, String::as_str);
            let mut hasher = DefaultHasher::new();
            concept.hash(&mut hasher);
            let bucket = usize::try_from(hasher.finish() % dims as u64).unwrap_or(0);
            vector[bucket] += 1.0;
        }
        if vector.iter().all(|value| *value == 0.0) {
            vector[0] = 1.0;
        }
        vector
    }
}

fn normalize(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    text.nfd()
        .filter(|c| c.is_ascii())
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect()
}

impl Embedder for HashEmbedder {
    fn embed<'a>(&'a self, model: &'a str, inputs: &'a [String]) -> EmbedFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.failure.lock().ok().and_then(|failure| failure.clone()) {
                return Err(error);
            }
            let dims = self.dims_for(model);
            Ok(inputs.iter().map(|text| self.vector(text, dims)).collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm(a) * norm(b))
    }

    #[tokio::test]
    async fn concepts_bring_texts_together() {
        let embedder = HashEmbedder::new().concept(&["suite", "testes"]);
        let inputs = vec![
            "query: rodar testes".to_owned(),
            "passage: rodar suíte".to_owned(),
            "passage: deploy na main".to_owned(),
        ];
        let vectors = embedder
            .embed("multilingual-e5-small", &inputs)
            .await
            .expect("vectors");
        assert_eq!(vectors[0].len(), 384);
        assert!((cosine(&vectors[0], &vectors[1]) - 1.0).abs() < 1e-6);
        assert!(cosine(&vectors[0], &vectors[2]) < 0.5);
        embedder.fail(Some(EmbedError::Timeout));
        assert_eq!(embedder.embed("x", &inputs).await, Err(EmbedError::Timeout));
        assert_eq!(embedder.calls(), 2);
    }
}
