//! Provedor `local`: `fastembed` sobre o ONNX Runtime carregado dinamicamente
//! (`ORT_DYLIB_PATH`). O modelo é baixado para `MODELS_PATH` (cache do Hugging Face,
//! `models--<org>--<nome>`) numa thread própria — na inicialização (`prepare`) ou no primeiro
//! uso — e fica carregado no processo; trocar de modelo troca a instância. Enquanto carrega,
//! as chamadas recebem `EmbedError::Loading`. `TextEmbedding::embed` exige `&mut self`: as
//! chamadas são serializadas.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use tokio::sync::Notify;

use super::{EmbedError, EmbedFuture, Embedder, EmbeddingModelEntry, LoadInfo, LoadState};

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

/// Uma sessão carregada: o `TextEmbedding` em produção, uma falsa nos testes.
pub trait Session: Send {
    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>, String>;
}

impl Session for TextEmbedding {
    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>, String> {
        TextEmbedding::embed(self, inputs, None).map_err(|error| error.to_string())
    }
}

/// Baixa (se preciso) e carrega um modelo de `MODELS_PATH`. Bloqueante: roda numa thread
/// própria, fora da requisição.
pub type Loader = Arc<dyn Fn(&LocalModel, &Path) -> Result<Box<dyn Session>, String> + Send + Sync>;

fn fastembed_loader() -> Loader {
    Arc::new(|model: &LocalModel, path: &Path| {
        let options = TextInitOptions::new(model.model.clone())
            .with_cache_dir(path.to_path_buf())
            .with_show_download_progress(false)
            .with_intra_threads(INTRA_THREADS);
        TextEmbedding::try_new(options)
            .map(|embedding| Box::new(embedding) as Box<dyn Session>)
            .map_err(|error| error.to_string())
    })
}

/// Quanto uma chamada espera um carregamento em andamento antes de devolver
/// `EmbedError::Loading`.
const LOAD_GRACE: Duration = Duration::from_secs(5);
/// Depois de uma falha de carregamento, a próxima tentativa só começa após este intervalo;
/// até lá as chamadas recebem a falha.
const RETRY_FAILED_AFTER: Duration = Duration::from_secs(30);

type SharedSession = Arc<Mutex<Box<dyn Session>>>;

enum Slot {
    Idle,
    Loading {
        id: u64,
        model: String,
        downloading: bool,
        started: Instant,
        started_wall: SystemTime,
    },
    Ready {
        model: String,
        session: SharedSession,
        started_wall: SystemTime,
        duration: Duration,
    },
    Failed {
        model: String,
        error: String,
        at: Instant,
        started_wall: SystemTime,
        duration: Duration,
    },
}

struct Shared {
    slot: Mutex<Slot>,
    changed: Notify,
    next_id: AtomicU64,
}

/// Instância compartilhada pelo processo. O modelo é baixado e carregado numa thread
/// própria, uma vez por modelo; quem chama espera no máximo `LOAD_GRACE` e depois recebe
/// `EmbedError::Loading` (o carregamento continua).
#[derive(Clone)]
pub struct LocalEmbedder {
    models_path: PathBuf,
    onnx_runtime: Option<PathBuf>,
    require_runtime: bool,
    loader: Loader,
    grace: Duration,
    shared: Arc<Shared>,
}

impl LocalEmbedder {
    pub fn new(models_path: PathBuf, onnx_runtime: Option<PathBuf>) -> Self {
        Self::build(
            models_path,
            onnx_runtime,
            true,
            fastembed_loader(),
            LOAD_GRACE,
        )
    }

    /// Com outro carregador e outra espera, sem exigir o ONNX Runtime (testes).
    pub fn with_loader(models_path: PathBuf, loader: Loader, grace: Duration) -> Self {
        Self::build(models_path, None, false, loader, grace)
    }

    fn build(
        models_path: PathBuf,
        onnx_runtime: Option<PathBuf>,
        require_runtime: bool,
        loader: Loader,
        grace: Duration,
    ) -> Self {
        Self {
            models_path,
            onnx_runtime,
            require_runtime,
            loader,
            grace,
            shared: Arc::new(Shared {
                slot: Mutex::new(Slot::Idle),
                changed: Notify::new(),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    pub fn models_path(&self) -> &Path {
        &self.models_path
    }

    /// Sem a biblioteca, o `ort` entra em pânico ao carregar: confere antes.
    fn check_runtime(&self) -> Result<(), EmbedError> {
        if !self.require_runtime {
            return Ok(());
        }
        match &self.onnx_runtime {
            Some(path) if path.is_file() => Ok(()),
            Some(_) => Err(EmbedError::Runtime(
                "ORT_DYLIB_PATH nao encontrado".to_owned(),
            )),
            None => Err(EmbedError::Runtime("ORT_DYLIB_PATH ausente".to_owned())),
        }
    }

    fn slot(&self) -> Result<MutexGuard<'_, Slot>, EmbedError> {
        self.shared
            .slot
            .lock()
            .map_err(|_| EmbedError::Runtime("instancia local envenenada".to_owned()))
    }

    /// A sessão do modelo, se pronta; senão dispara o carregamento (se ainda não houver um
    /// em andamento) e devolve `Loading` ou a falha recente.
    fn ensure(&self, model: &LocalModel) -> Result<SharedSession, EmbedError> {
        let mut slot = self.slot()?;
        match &*slot {
            Slot::Ready {
                model: loaded,
                session,
                ..
            } if *loaded == model.id => return Ok(session.clone()),
            Slot::Loading {
                model: loading,
                downloading,
                started,
                ..
            } if *loading == model.id => {
                return Err(loading_error(&model.id, *downloading, *started));
            }
            Slot::Failed {
                model: failed,
                error,
                at,
                ..
            } if *failed == model.id && at.elapsed() < RETRY_FAILED_AFTER => {
                return Err(EmbedError::LoadFailed(error.clone()));
            }
            _ => {}
        }
        let downloading = !self.models_path.join(model.cache_dir()).exists();
        let started = Instant::now();
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        *slot = Slot::Loading {
            id,
            model: model.id.clone(),
            downloading,
            started,
            started_wall: SystemTime::now(),
        };
        drop(slot);
        tracing::info!(
            event = "embeddings.loading",
            model = %model.id,
            downloading,
            "carregando o modelo local de embedding em segundo plano"
        );
        let this = self.clone();
        let loading = model.clone();
        std::thread::spawn(move || this.load(id, &loading, downloading, started));
        Err(loading_error(&model.id, downloading, started))
    }

    /// Thread do carregamento: grava `Ready` ou `Failed` se ainda for o carregamento vigente.
    fn load(&self, id: u64, model: &LocalModel, downloading: bool, started: Instant) {
        let result = (self.loader)(model, &self.models_path);
        let duration = started.elapsed();
        let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        let Ok(mut slot) = self.shared.slot.lock() else {
            return;
        };
        let started_wall = match &*slot {
            Slot::Loading {
                id: current,
                started_wall,
                ..
            } if *current == id => *started_wall,
            _ => return,
        };
        *slot = match result {
            Ok(session) => {
                tracing::info!(
                    event = "embeddings.ready",
                    model = %model.id,
                    downloaded = downloading,
                    duration_ms,
                    "modelo local de embedding carregado"
                );
                Slot::Ready {
                    model: model.id.clone(),
                    session: Arc::new(Mutex::new(session)),
                    started_wall,
                    duration,
                }
            }
            Err(error) => {
                tracing::warn!(
                    event = "embeddings.load_failed",
                    model = %model.id,
                    duration_ms,
                    error = %error,
                    "falha ao carregar o modelo local de embedding"
                );
                Slot::Failed {
                    model: model.id.clone(),
                    error: error.chars().take(MAX_LOAD_ERROR_CHARS).collect(),
                    at: Instant::now(),
                    started_wall,
                    duration,
                }
            }
        };
        drop(slot);
        self.shared.changed.notify_waiters();
    }

    async fn run(&self, name: &str, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.check_runtime()?;
        let model = find(name).ok_or_else(|| EmbedError::UnknownModel(name.to_owned()))?;
        let deadline = tokio::time::Instant::now() + self.grace;
        let session = loop {
            // Registra o interesse antes de olhar o estado, para não perder o aviso.
            let notified = self.shared.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.ensure(&model) {
                Ok(session) => break session,
                Err(EmbedError::Loading { .. }) if tokio::time::Instant::now() < deadline => {
                    let _ = tokio::time::timeout_at(deadline, notified).await;
                }
                Err(error) => return Err(error),
            }
        };
        tokio::task::spawn_blocking(move || {
            session
                .lock()
                .map_err(|_| EmbedError::Runtime("instancia local envenenada".to_owned()))?
                .embed(&inputs)
                .map_err(|_| EmbedError::InvalidResponse)
        })
        .await
        .map_err(|_| EmbedError::Runtime("falha no ONNX Runtime".to_owned()))?
    }
}

const MAX_LOAD_ERROR_CHARS: usize = 160;

fn loading_error(model: &str, downloading: bool, started: Instant) -> EmbedError {
    EmbedError::Loading {
        model: model.to_owned(),
        state: if downloading {
            LoadState::Downloading
        } else {
            LoadState::Loading
        },
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

impl Embedder for LocalEmbedder {
    fn embed<'a>(&'a self, model: &'a str, inputs: &'a [String]) -> EmbedFuture<'a> {
        let this = self.clone();
        let model = model.to_owned();
        let inputs = inputs.to_vec();
        Box::pin(async move { this.run(&model, inputs).await })
    }

    fn load_info(&self, name: &str) -> Option<LoadInfo> {
        let error = |error: String| LoadInfo {
            state: LoadState::Failed,
            started: None,
            elapsed: None,
            error: Some(error),
        };
        if let Err(EmbedError::Runtime(reason)) = self.check_runtime() {
            return Some(error(format!("ONNX Runtime indisponivel ({reason})")));
        }
        let Some(model) = find(name) else {
            return Some(error(format!("modelo local desconhecido: {name}")));
        };
        let slot = self.shared.slot.lock().ok()?;
        Some(match &*slot {
            Slot::Loading {
                model: loading,
                downloading,
                started,
                started_wall,
                ..
            } if *loading == model.id => LoadInfo {
                state: if *downloading {
                    LoadState::Downloading
                } else {
                    LoadState::Loading
                },
                started: Some(*started_wall),
                elapsed: Some(started.elapsed()),
                error: None,
            },
            Slot::Ready {
                model: loaded,
                started_wall,
                duration,
                ..
            } if *loaded == model.id => LoadInfo {
                state: LoadState::Ready,
                started: Some(*started_wall),
                elapsed: Some(*duration),
                error: None,
            },
            Slot::Failed {
                model: failed,
                error,
                started_wall,
                duration,
                ..
            } if *failed == model.id => LoadInfo {
                state: LoadState::Failed,
                started: Some(*started_wall),
                elapsed: Some(*duration),
                error: Some(error.clone()),
            },
            _ => LoadInfo {
                state: LoadState::Idle,
                started: None,
                elapsed: None,
                error: None,
            },
        })
    }

    fn prepare(&self, name: &str) {
        if self.check_runtime().is_err() {
            return;
        }
        if let Some(model) = find(name) {
            let _ = self.ensure(&model);
        }
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
