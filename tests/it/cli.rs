//! O binário de verdade: startup recusado, `migrate`, `doctor` e o nome `eng`.

use std::{
    path::Path,
    process::{Command, Output},
};

const PEPPER: &str = "pepper-de-teste-com-mais-de-32-caracteres";
const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// Caminho do binário. Com `cargo nextest run --archive-file` (shards do CI) o caminho de
/// compilação não existe; o nextest informa o caminho extraído em `NEXTEST_BIN_EXE_engramind`.
fn bin() -> String {
    std::env::var("NEXTEST_BIN_EXE_engramind")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_engramind").to_owned())
}

/// Ambiente limpo, no diretório do teste (sem `.env` do repositório).
fn command(dir: &Path, args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut command = Command::new(bin());
    command
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("LOG_ANSI", "false")
        .env("DATABASE_PATH", dir.join("engramind.db"));
    for (name, value) in vars {
        command.env(name, value);
    }
    command.output().expect("run engramind")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn serve_vars() -> Vec<(&'static str, &'static str)> {
    // Porta 0 nunca chega a ser aberta: os testes só exercitam falhas anteriores ao bind.
    vec![
        ("APP_PORT", "0"),
        ("TOKEN_PEPPER", PEPPER),
        ("SECRETS_KEY", KEY),
    ]
}

#[test]
fn serve_refuses_to_start_without_required_variables() {
    let dir = tempfile::tempdir().expect("tempdir");
    for missing in ["APP_PORT", "TOKEN_PEPPER", "SECRETS_KEY"] {
        let vars: Vec<_> = serve_vars()
            .into_iter()
            .filter(|(name, _)| *name != missing)
            .collect();
        let output = command(dir.path(), &["serve"], &vars);
        assert!(!output.status.success(), "{missing}");
        let text = text(&output);
        assert!(text.contains("application.failed"), "{text}");
        assert!(text.contains(missing), "{text}");
        assert!(!text.contains(PEPPER) && !text.contains(KEY), "{text}");
    }
}

#[test]
fn invalid_log_configuration_prevents_startup() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, value) in [
        ("LOG_FORMAT", "xml"),
        ("LOG_ANSI", "talvez"),
        ("RUST_LOG", "engramind=nivel"),
    ] {
        let mut vars = serve_vars();
        vars.push((name, value));
        let output = command(dir.path(), &["serve"], &vars);
        assert!(!output.status.success(), "{name}={value}");
        assert!(text(&output).contains(name), "{}", text(&output));
    }
}

#[test]
fn serve_refuses_a_divergent_pepper() {
    let dir = tempfile::tempdir().expect("tempdir");
    // APP_PORT inválido para o bind faria o primeiro serve falhar depois do pepper; em vez
    // disso, o pepper é registrado com um serve que falha no bind de uma porta ocupada.
    let busy = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = busy.local_addr().expect("addr").port().to_string();
    let mut vars = vec![
        ("APP_HOST", "127.0.0.1"),
        ("APP_PORT", port.as_str()),
        ("TOKEN_PEPPER", PEPPER),
        ("SECRETS_KEY", KEY),
    ];
    let first = command(dir.path(), &["serve"], &vars);
    assert!(
        text(&first).contains("falha ao abrir porta HTTP"),
        "{}",
        text(&first)
    );

    vars[2] = (
        "TOKEN_PEPPER",
        "outro-pepper-tambem-com-mais-de-32-caracteres",
    );
    let second = command(dir.path(), &["serve"], &vars);
    assert!(!second.status.success());
    let text = text(&second);
    assert!(text.contains("TOKEN_PEPPER nao corresponde"), "{text}");
    assert!(!text.contains("outro-pepper"), "{text}");
}

#[test]
fn migrate_and_doctor_report_in_json() {
    let dir = tempfile::tempdir().expect("tempdir");

    let missing = command(dir.path(), &["doctor", "--json"], &[]);
    assert!(!missing.status.success());

    let migrate = command(dir.path(), &["migrate", "--json"], &[]);
    assert!(migrate.status.success(), "{}", text(&migrate));
    let report: serde_json::Value = serde_json::from_slice(&migrate.stdout).expect("json");
    assert_eq!(report["before"], 0);
    assert_eq!(report["after"], report["expected"]);

    let again = command(dir.path(), &["migrate", "--json"], &[]);
    let report: serde_json::Value = serde_json::from_slice(&again.stdout).expect("json");
    assert_eq!(report["applied"], 0);

    let doctor = command(dir.path(), &["doctor", "--json"], &[]);
    assert!(doctor.status.success(), "{}", text(&doctor));
    let report: serde_json::Value = serde_json::from_slice(&doctor.stdout).expect("json");
    assert_eq!(report["healthy"], true);
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    let checks = report["checks"].as_array().expect("checks");
    let status = |name: &str| {
        checks
            .iter()
            .find(|check| check["name"] == name)
            .map(|check| check["status"].clone())
    };
    for name in [
        "database",
        "integrity",
        "migrations",
        "journal_mode",
        "sqlite_vec",
        "fts5",
    ] {
        assert_eq!(status(name), Some("ok".into()), "{name}");
    }
    assert_eq!(status("backup"), Some("skip".into()));
}

#[cfg(unix)]
#[test]
fn the_binary_also_answers_as_eng() {
    let dir = tempfile::tempdir().expect("tempdir");
    let link = dir.path().join("eng");
    std::os::unix::fs::symlink(bin(), &link).expect("symlink");
    let output = Command::new(&link)
        .arg("--version")
        .output()
        .expect("run eng");
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")),
        "{}",
        text(&output)
    );
    let help = Command::new(&link).arg("--help").output().expect("help");
    let help = String::from_utf8_lossy(&help.stdout);
    for command in ["serve", "migrate", "doctor"] {
        assert!(help.contains(command), "{help}");
    }
}
