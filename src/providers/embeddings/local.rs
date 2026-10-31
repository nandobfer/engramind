//! Provedor `local`: `fastembed` sobre o ONNX Runtime carregado dinamicamente
//! (`ORT_DYLIB_PATH`). O modelo é baixado no primeiro uso para `MODELS_PATH` (cache do
//! Hugging Face, `models--<org>--<nome>`) e fica carregado no processo; trocar de modelo
//! troca a instância. `TextEmbedding::embed` exige `&mut self`: as chamadas são serializadas.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use super::{EmbedError, EmbedFuture, Embedder, EmbeddingModelEntry};

/// Threads do ONNX Runtime por chamada.
const INTRA_THREADS: usize = 2;
const CACHE_PREFIX: &str = "models--";

/// Um modelo suportado pelo `fastembed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModel {
    /// Nome canônico, em kebab-case (`multilingual-e5-small`).
    pub id: String,
    pub dims: usize,
    pub description: String,
    /// Repositório no Hugging Face (`intfloat/multilingual-e5-small`).
    pub code: String,
    model: EmbeddingModel,
}

impl LocalModel {
    /// Diretório do modelo no cache de `MODELS_PATH`.
    pub fn cache_dir(&self) -> String {
        format!("{CACHE_PREFIX}{}", self.code.replace('/', "--"))
    }

    pub fn entry(&self) -> EmbeddingModelEntry {
        EmbeddingModelEntry {
            id: self.id.clone(),
            dimensions: Some(self.dims),
            description: Some(self.description.clone()),
        }
    }
}

/// `MultilingualE5Small` → `multilingual-e5-small`.
fn kebab(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut previous: Option<char> = None;
    for c in name.chars() {
        if c.is_ascii_uppercase()
            && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
        {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
        previous = Some(c);
    }
    out
}

/// Só letras e dígitos, em minúsculas: `bge-m3`, `BGEM3` e `bgem3` são o mesmo nome.
fn normalized(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Nome do repositório sem o dono nem o sufixo `-onnx` (`BAAI/bge-m3` → `bge-m3`).
fn tail(code: &str) -> String {
    let name = code.rsplit('/').next().unwrap_or(code).to_ascii_lowercase();
    name.strip_suffix("-onnx")
        .map(str::to_owned)
        .unwrap_or(name)
}

/// Nome canônico: o do repositório quando só um modelo o usa; senão o do enum em kebab-case
/// (variantes quantizadas compartilham o repositório).
pub fn supported() -> Vec<LocalModel> {
    let infos = TextEmbedding::list_supported_models();
    let tails: Vec<String> = infos.iter().map(|info| tail(&info.model_code)).collect();
    infos
        .into_iter()
        .zip(&tails)
        .map(|(info, name)| LocalModel {
            id: if tails.iter().filter(|other| *other == name).count() == 1 {
                name.clone()
            } else {
                kebab(&format!("{:?}", info.model))
            },
            dims: info.dim,
            description: info.description,
            code: info.model_code,
            model: info.model,
        })
        .collect()
}

/// O modelo pelo nome canônico, pelo nome do enum ou pelo repositório.
pub fn find(name: &str) -> Option<LocalModel> {
    let wanted = normalized(name.trim());
    if wanted.is_empty() {
        return None;
    }
    let models = supported();
    models
        .iter()
        .find(|model| normalized(&model.id) == wanted)
        .or_else(|| {
            models
                .iter()
                .find(|model| normalized(&format!("{:?}", model.model)) == wanted)
        })
        .or_else(|| {
            models
                .iter()
                .find(|model| model.code.eq_ignore_ascii_case(name.trim()))
        })
        .cloned()
}

/// Remove de `models_path` os modelos baixados que não são `keep`; devolve os removidos.
pub fn remove_unused(
    models_path: &Path,
    keep: Option<&LocalModel>,
) -> std::io::Result<Vec<String>> {
    let keep = keep.map(LocalModel::cache_dir);
    let mut removed = Vec::new();
    let entries = match std::fs::read_dir(models_path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(removed),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(CACHE_PREFIX) || keep.as_deref() == Some(name.as_str()) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
            removed.push(name);
        }
    }
    Ok(removed)
}

struct Loaded {
    model: EmbeddingModel,
    embedding: TextEmbedding,
}

/// Instância compartilhada pelo processo.
#[derive(Clone)]
pub struct LocalEmbedder {
    models_path: PathBuf,
    onnx_runtime: Option<PathBuf>,
    loaded: Arc<Mutex<Option<Loaded>>>,
}

impl LocalEmbedder {
    pub fn new(models_path: PathBuf, onnx_runtime: Option<PathBuf>) -> Self {
        Self {
            models_path,
            onnx_runtime,
            loaded: Arc::new(Mutex::new(None)),
        }
    }

    pub fn models_path(&self) -> &Path {
        &self.models_path
    }

    fn run(&self, name: &str, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, EmbedError> {
        // Sem a biblioteca, o `ort` entra em pânico ao carregar: confere antes.
        match &self.onnx_runtime {
            Some(path) if path.is_file() => {}
            Some(_) => {
                return Err(EmbedError::Runtime(
                    "ORT_DYLIB_PATH nao encontrado".to_owned(),
                ));
            }
            None => return Err(EmbedError::Runtime("ORT_DYLIB_PATH ausente".to_owned())),
        }
        let model = find(name).ok_or_else(|| EmbedError::UnknownModel(name.to_owned()))?;
        let mut guard = self
            .loaded
            .lock()
            .map_err(|_| EmbedError::Runtime("instancia local envenenada".to_owned()))?;
        if guard
            .as_ref()
            .is_none_or(|loaded| loaded.model != model.model)
        {
            *guard = None;
            let started = Instant::now();
            let options = TextInitOptions::new(model.model.clone())
                .with_cache_dir(self.models_path.clone())
                .with_show_download_progress(false)
                .with_intra_threads(INTRA_THREADS);
            let embedding = TextEmbedding::try_new(options).map_err(|error| {
                tracing::warn!(
                    event = "embeddings.load_failed",
                    model = %model.id,
                    error = %error,
                    "falha ao carregar o modelo local de embedding"
                );
                EmbedError::Unavailable
            })?;
            tracing::info!(
                event = "embeddings.ready",
                model = %model.id,
                duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                "modelo local de embedding carregado"
            );
            *guard = Some(Loaded {
                model: model.model.clone(),
                embedding,
            });
        }
        let loaded = guard.as_mut().ok_or(EmbedError::Unavailable)?;
        loaded
            .embedding
            .embed(&inputs, None)
            .map_err(|_| EmbedError::InvalidResponse)
    }
}

impl Embedder for LocalEmbedder {
    fn embed<'a>(&'a self, model: &'a str, inputs: &'a [String]) -> EmbedFuture<'a> {
        let this = self.clone();
        let model = model.to_owned();
        let inputs = inputs.to_vec();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.run(&model, inputs))
                .await
                .map_err(|_| EmbedError::Runtime("falha no ONNX Runtime".to_owned()))?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_canonical_and_matched_loosely() {
        assert_eq!(kebab("MultilingualE5Small"), "multilingual-e5-small");
        let model = find("multilingual-e5-small").expect("e5");
        assert_eq!(model.dims, 384);
        assert_eq!(model.code, "intfloat/multilingual-e5-small");
        assert_eq!(model.cache_dir(), "models--intfloat--multilingual-e5-small");
        assert_eq!(find("MultilingualE5Small"), Some(model.clone()));
        assert_eq!(find("intfloat/multilingual-e5-small"), Some(model));
        assert_eq!(find("bge-m3").expect("bge").id, "bge-m3");
        assert_eq!(find("BGEM3").expect("bge").id, "bge-m3");
        let ids: Vec<String> = supported().into_iter().map(|model| model.id).collect();
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "nomes canonicos unicos");
        assert_eq!(find("gpt-4o"), None);
        assert_eq!(find(""), None);
    }

    #[test]
    fn unused_models_are_removed_and_the_current_one_is_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model = find("multilingual-e5-small").expect("e5");
        for name in [model.cache_dir().as_str(), "models--BAAI--bge-m3", "outro"] {
            std::fs::create_dir(dir.path().join(name)).expect("dir");
        }
        let removed = remove_unused(dir.path(), Some(&model)).expect("remove");
        assert_eq!(removed, vec!["models--BAAI--bge-m3".to_owned()]);
        assert!(dir.path().join(model.cache_dir()).exists());
        assert!(dir.path().join("outro").exists());
        assert_eq!(
            remove_unused(&dir.path().join("nada"), None).expect("missing"),
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn missing_runtime_fails_without_panicking() {
        let embedder = LocalEmbedder::new(PathBuf::from("/nao-existe"), None);
        let inputs = vec!["texto".to_owned()];
        assert!(matches!(
            embedder.embed("multilingual-e5-small", &inputs).await,
            Err(EmbedError::Runtime(_))
        ));
        let embedder = LocalEmbedder::new(
            PathBuf::from("/nao-existe"),
            Some(PathBuf::from("/nao-existe/libonnxruntime.so")),
        );
        assert!(matches!(
            embedder.embed("multilingual-e5-small", &inputs).await,
            Err(EmbedError::Runtime(_))
        ));
    }
}
