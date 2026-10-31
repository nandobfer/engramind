//! Diagnóstico local do banco (`engramind doctor`).

use std::{io::ErrorKind, path::Path, process::Command};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    config::DatabaseSettings,
    store::{SqliteStore, Store},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Fail,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

impl Check {
    fn new(name: &'static str, status: CheckStatus, detail: impl Into<String>) -> Self {
        Self {
            name,
            status,
            detail: detail.into(),
        }
    }

    fn from_bool(name: &'static str, ok: bool, detail: impl Into<String>) -> Self {
        let status = if ok {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        };
        Self::new(name, status, detail)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorOutput {
    pub version: &'static str,
    pub database_path: String,
    pub healthy: bool,
    pub checks: Vec<Check>,
}

pub async fn run(settings: &DatabaseSettings) -> DoctorOutput {
    let mut checks = database_checks(&settings.path, settings.read_pool_size).await;
    checks.push(backup_check(settings).await);
    checks.push(onnx_runtime_check(std::env::var("ORT_DYLIB_PATH").ok()));
    DoctorOutput {
        version: env!("CARGO_PKG_VERSION"),
        database_path: settings.path.display().to_string(),
        healthy: checks.iter().all(|check| check.status != CheckStatus::Fail),
        checks,
    }
}

async fn database_checks(path: &Path, read_pool_size: usize) -> Vec<Check> {
    if !path.exists() {
        return vec![Check::new(
            "database",
            CheckStatus::Fail,
            "arquivo do banco nao encontrado",
        )];
    }
    let store = match SqliteStore::open(path, read_pool_size) {
        Ok(store) => store,
        Err(error) => return vec![Check::new("database", CheckStatus::Fail, error.to_string())],
    };
    let report = match store.diagnose().await {
        Ok(report) => report,
        Err(error) => return vec![Check::new("database", CheckStatus::Fail, error.to_string())],
    };
    vec![
        Check::new("database", CheckStatus::Ok, "banco acessivel"),
        Check::from_bool("integrity", report.integrity == "ok", report.integrity),
        Check::from_bool(
            "migrations",
            report.migrations.is_current(),
            format!(
                "{} de {} aplicadas",
                report.migrations.applied, report.migrations.expected
            ),
        ),
        Check::from_bool(
            "journal_mode",
            report.journal_mode.eq_ignore_ascii_case("wal"),
            report.journal_mode,
        ),
        match report.vec_version {
            Some(version) => Check::new("sqlite_vec", CheckStatus::Ok, version),
            None => Check::new("sqlite_vec", CheckStatus::Fail, "extensao nao carregada"),
        },
        Check::from_bool(
            "fts5",
            report.fts5,
            format!("SQLite {}", report.sqlite_version),
        ),
        match report.memory_fts {
            Some(result) => Check::from_bool("memory_fts", result == "ok", result),
            None => Check::new(
                "memory_fts",
                CheckStatus::Skip,
                "memorias ainda nao migradas",
            ),
        },
    ]
}

/// Biblioteca do ONNX Runtime do provedor local de embeddings (007). Sem `ORT_DYLIB_PATH`,
/// o item é pulado: o provedor pode ser remoto ou `off`.
fn onnx_runtime_check(path: Option<String>) -> Check {
    match path.filter(|path| !path.trim().is_empty()) {
        None => Check::new(
            "onnx_runtime",
            CheckStatus::Skip,
            "ORT_DYLIB_PATH ausente; embedding.provider = local indisponivel",
        ),
        Some(path) => Check::from_bool("onnx_runtime", Path::new(&path).is_file(), path),
    }
}

#[derive(Debug, Deserialize)]
struct LtxFile {
    timestamp: String,
}

/// Idade da última réplica pelo `litestream ltx` (D-009). Fora do contêiner, sem o binário,
/// o item é pulado.
pub async fn backup_check(settings: &DatabaseSettings) -> Check {
    backup_measure(settings).await.0
}

/// Como `backup_check`, com o instante da réplica mais recente (amostrador de `get_status` e
/// `/metrics`, 008 Q-009).
pub async fn backup_measure(settings: &DatabaseSettings) -> (Check, Option<DateTime<Utc>>) {
    if !settings.litestream_enabled {
        return (
            Check::new(
                "backup",
                CheckStatus::Skip,
                "LITESTREAM_ENABLED=false: sem backup",
            ),
            None,
        );
    }
    let path = settings.path.display().to_string();
    let output = tokio::task::spawn_blocking(move || {
        Command::new("litestream")
            .args(["ltx", "-json", "-level", "all", &path])
            .output()
    })
    .await;
    let output = match output {
        Ok(Ok(output)) => output,
        Ok(Err(error)) if error.kind() == ErrorKind::NotFound => {
            return (
                Check::new(
                    "backup",
                    CheckStatus::Skip,
                    "litestream nao instalado: sem backup",
                ),
                None,
            );
        }
        Ok(Err(error)) => {
            return (
                Check::new("backup", CheckStatus::Fail, error.to_string()),
                None,
            );
        }
        Err(error) => {
            return (
                Check::new("backup", CheckStatus::Fail, error.to_string()),
                None,
            );
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().next().unwrap_or("litestream ltx falhou");
        return (
            Check::new("backup", CheckStatus::Fail, detail.to_owned()),
            None,
        );
    }
    replica_age(&output.stdout, Utc::now())
}

fn replica_age(stdout: &[u8], now: DateTime<Utc>) -> (Check, Option<DateTime<Utc>>) {
    let files: Vec<LtxFile> = match serde_json::from_slice(stdout) {
        Ok(files) => files,
        Err(error) => {
            return (
                Check::new(
                    "backup",
                    CheckStatus::Fail,
                    format!("saida do litestream invalida: {error}"),
                ),
                None,
            );
        }
    };
    let latest = files
        .iter()
        .filter_map(|file| DateTime::parse_from_rfc3339(&file.timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .max();
    match latest {
        Some(latest) => (
            Check::new(
                "backup",
                CheckStatus::Ok,
                format!(
                    "ultima replica ha {}s ({})",
                    (now - latest).num_seconds().max(0),
                    latest.to_rfc3339()
                ),
            ),
            Some(latest),
        ),
        None => (
            Check::new("backup", CheckStatus::Fail, "nenhuma replica encontrada"),
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onnx_runtime_is_skipped_without_the_variable_and_checked_with_it() {
        assert_eq!(onnx_runtime_check(None).status, CheckStatus::Skip);
        assert_eq!(
            onnx_runtime_check(Some("/nao/existe.so".to_owned())).status,
            CheckStatus::Fail
        );
        let file = tempfile::NamedTempFile::new().expect("file");
        let path = file.path().display().to_string();
        assert_eq!(onnx_runtime_check(Some(path)).status, CheckStatus::Ok);
    }

    #[test]
    fn replica_age_uses_the_newest_ltx_file() {
        let now = DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
            .expect("now")
            .with_timezone(&Utc);
        let stdout = br#"[
            {"level":0,"min_txid":"1","max_txid":"1","size":10,"timestamp":"2026-10-30T11:00:00Z"},
            {"level":9,"min_txid":"1","max_txid":"2","size":10,"timestamp":"2026-10-30T11:59:30Z"}
        ]"#;
        let (check, latest) = replica_age(stdout, now);
        assert!(latest.is_some());
        assert_eq!(check.status, CheckStatus::Ok);
        assert!(
            check.detail.starts_with("ultima replica ha 30s"),
            "{}",
            check.detail
        );

        assert_eq!(replica_age(b"[]", now).0.status, CheckStatus::Fail);
        assert_eq!(replica_age(b"nao json", now).0.status, CheckStatus::Fail);
    }
}
