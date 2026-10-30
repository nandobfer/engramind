use std::process::ExitCode;

use clap::Parser;
use engramind::cli::{self, Cli, output};

#[tokio::main]
async fn main() -> ExitCode {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    match cli::run(cli).await {
        Ok(code) => code,
        Err(error) => {
            if tracing::dispatcher::has_been_set() {
                tracing::error!(event = "application.failed", error = %format!("{error:#}"), "aplicacao encerrada com falha");
            } else {
                output::failure(&error);
            }
            ExitCode::FAILURE
        }
    }
}
