//! Árvore de comandos. O binário atende pelos nomes `engramind` e `eng` (D-003).

pub mod output;

use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};

use crate::{
    HttpSettings, Settings, build_app,
    config::DatabaseSettings,
    core::pepper::{self, PepperCheck},
    doctor::{self, CheckStatus},
    observability::{self, LogTarget},
    store::SqliteStore,
};

#[derive(Debug, Parser)]
#[command(
    name = "engramind",
    version,
    about = "Rede de memórias para agentes de IA",
    long_about = "Rede de memórias para agentes de IA. Também invocável como `eng`."
)]
pub struct Cli {
    /// Saída em JSON.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Sobe o servidor HTTP (aplica as migrações pendentes antes).
    Serve,
    /// Aplica as migrações pendentes e sai.
    Migrate,
    /// Confere integridade, migrações, extensões e a idade da última réplica.
    Doctor,
}

pub async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Serve => serve().await.map(|()| ExitCode::SUCCESS),
        Command::Migrate => migrate(cli.json).await,
        Command::Doctor => run_doctor(cli.json).await,
    }
}

async fn serve() -> anyhow::Result<()> {
    observability::init_logging(LogTarget::Stdout).context("observabilidade invalida")?;
    let settings = Settings::from_env().context("configuracao invalida")?;
    let store = SqliteStore::open(&settings.database.path, settings.database.read_pool_size)
        .context("falha ao abrir o banco")?;
    let migrations = store.migrate().await.context("falha ao migrar o banco")?;
    if migrations.newly_applied() > 0 {
        tracing::info!(
            event = "database.migrated",
            before = migrations.before,
            after = migrations.after,
            "migracoes aplicadas"
        );
    }
    match pepper::enforce(&store, &settings.token_pepper)
        .await
        .context("falha ao validar TOKEN_PEPPER")?
    {
        PepperCheck::Enrolled => {
            tracing::info!(
                event = "security.pepper_enrolled",
                "verificador do TOKEN_PEPPER registrado"
            );
        }
        PepperCheck::Verified => {}
    }

    let app = build_app(store, HttpSettings::from(&settings));
    let listener = tokio::net::TcpListener::bind(settings.address())
        .await
        .context("falha ao abrir porta HTTP")?;
    tracing::info!(
        event = "application.started",
        version = env!("CARGO_PKG_VERSION"),
        environment = %settings.environment,
        host = %settings.host,
        port = settings.port,
        litestream = settings.database.litestream_enabled,
        "Engramind iniciado"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("servidor HTTP falhou")?;
    tracing::info!(event = "application.stopped", "Engramind encerrado");
    Ok(())
}

async fn migrate(json: bool) -> anyhow::Result<ExitCode> {
    observability::init_logging(LogTarget::Stderr).context("observabilidade invalida")?;
    let settings = DatabaseSettings::from_env().context("configuracao invalida")?;
    let store = SqliteStore::open(&settings.path, settings.read_pool_size)
        .context("falha ao abrir o banco")?;
    let outcome = store.migrate().await.context("falha ao migrar o banco")?;
    if json {
        output::json(&serde_json::json!({
            "applied": outcome.newly_applied(),
            "before": outcome.before,
            "after": outcome.after,
            "expected": outcome.expected,
        }));
    } else {
        output::line(&format!(
            "{} migracao(oes) aplicada(s); schema em {} de {}",
            outcome.newly_applied(),
            outcome.after,
            outcome.expected
        ));
    }
    Ok(ExitCode::SUCCESS)
}

async fn run_doctor(json: bool) -> anyhow::Result<ExitCode> {
    observability::init_logging(LogTarget::Stderr).context("observabilidade invalida")?;
    let settings = DatabaseSettings::from_env().context("configuracao invalida")?;
    let report = doctor::run(&settings).await;
    if json {
        output::json(&report);
    } else {
        output::line(&format!(
            "engramind {} - {}",
            report.version, report.database_path
        ));
        for check in &report.checks {
            let mark = match check.status {
                CheckStatus::Ok => "ok  ",
                CheckStatus::Fail => "FALHA",
                CheckStatus::Skip => "pula",
            };
            output::line(&format!("[{mark}] {}: {}", check.name, check.detail));
        }
    }
    Ok(if report.healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
