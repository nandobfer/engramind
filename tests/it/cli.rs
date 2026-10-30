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

mod client {
    //! A CLI como cliente HTTP: conexão, perfis, `--json`, códigos de saída e `help --json`.

    use std::{
        path::Path,
        process::{Command, Output},
    };

    use serde_json::Value;

    use super::{bin, text};
    use crate::support::{ADMIN_TOKEN, TestContext};

    /// Ambiente limpo com `HOME`/`XDG_CONFIG_HOME` no diretório do teste.
    fn run(home: &Path, args: &[&str], vars: &[(&str, &str)]) -> Output {
        let mut command = Command::new(bin());
        command
            .args(args)
            .env_clear()
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home);
        for (name, value) in vars {
            command.env(name, value);
        }
        command.output().expect("run engramind")
    }

    async fn blocking(home: &Path, args: &[&str], vars: &[(&str, &str)]) -> Output {
        let home = home.to_owned();
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        tokio::task::spawn_blocking(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let vars: Vec<(&str, &str)> = vars
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            run(&home, &args, &vars)
        })
        .await
        .expect("join")
    }

    fn stdout_json(output: &Output) -> Value {
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("JSON: {}", text(output)))
    }

    #[test]
    fn help_json_lists_every_operation_and_local_command() {
        let dir = tempfile::tempdir().expect("tempdir");
        let output = run(dir.path(), &["help", "--json"], &[]);
        assert!(output.status.success(), "{}", text(&output));
        let help = stdout_json(&output);
        assert_eq!(help["version"], env!("CARGO_PKG_VERSION"));
        let commands = help["commands"].as_array().expect("commands");
        for operation in engramind::core::operations::OPERATIONS {
            let path = operation.cli.join(" ");
            let command = commands
                .iter()
                .find(|command| command["path"] == path.as_str())
                .unwrap_or_else(|| panic!("sem comando: {path}"));
            assert_eq!(command["operation"], operation.name);
        }
        let issue = commands
            .iter()
            .find(|command| command["path"] == "tokens issue")
            .expect("tokens issue");
        let longs: Vec<&str> = issue["args"]
            .as_array()
            .expect("args")
            .iter()
            .filter_map(|arg| arg["long"].as_str())
            .collect();
        for flag in [
            "--app",
            "--agent",
            "--kind",
            "--scopes",
            "--idempotency-key",
        ] {
            assert!(longs.contains(&flag), "{flag}: {longs:?}");
        }
        let exits = help["exit_codes"].as_array().expect("exit codes");
        assert!(
            exits
                .iter()
                .any(|exit| exit["code"] == "SCOPE_REQUIRED" && exit["exit"] == 4)
        );
        let plain = run(dir.path(), &["help"], &[]);
        assert!(text(&plain).contains("tokens"), "{}", text(&plain));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commands_resolve_flags_env_and_profiles_and_map_exit_codes() {
        let context = TestContext::new().await;
        let url = context.serve().await;
        let home = context.dir.path().join("home");
        std::fs::create_dir_all(&home).expect("home");

        // Sem URL e token: uso inválido local.
        let missing = blocking(&home, &["whoami", "--json"], &[]).await;
        assert_eq!(missing.status.code(), Some(2), "{}", text(&missing));
        assert_eq!(stdout_json(&missing)["error"]["code"], "CLI_CONFIG_INVALID");

        // Variáveis de ambiente.
        let env = [
            ("ENGRAMIND_URL", url.as_str()),
            ("ENGRAMIND_TOKEN", ADMIN_TOKEN),
        ];
        let whoami = blocking(&home, &["whoami", "--json"], &env).await;
        assert!(whoami.status.success(), "{}", text(&whoami));
        assert_eq!(stdout_json(&whoami)["token_id"], "tk_00000001");

        // Saída legível e aviso do segredo no stderr.
        let created = blocking(
            &home,
            &["apps", "create", "--slug", "app-a", "--name", "App A"],
            &env,
        )
        .await;
        assert!(created.status.success(), "{}", text(&created));
        assert!(String::from_utf8_lossy(&created.stdout).contains("slug: app-a"));
        let issued = blocking(
            &home,
            &[
                "tokens",
                "issue",
                "--app",
                "app-a",
                "--kind",
                "admin",
                "--scopes",
                "read",
                "--idempotency-key",
                "k1",
            ],
            &env,
        )
        .await;
        assert!(issued.status.success(), "{}", text(&issued));
        assert!(String::from_utf8_lossy(&issued.stdout).contains("secret: eng.tk_"));
        assert!(String::from_utf8_lossy(&issued.stderr).contains("guarde o segredo"));
        let replay = blocking(
            &home,
            &[
                "tokens",
                "issue",
                "--app",
                "app-a",
                "--kind",
                "admin",
                "--scopes",
                "read",
                "--idempotency-key",
                "k1",
                "--json",
            ],
            &env,
        )
        .await;
        assert!(stdout_json(&replay).get("secret").is_none());

        // Códigos de saída.
        let not_found = blocking(&home, &["apps", "get", "nao-existe", "--json"], &env).await;
        assert_eq!(not_found.status.code(), Some(5));
        assert_eq!(stdout_json(&not_found)["error"]["code"], "APP_NOT_FOUND");
        let conflict = blocking(
            &home,
            &["apps", "create", "--slug", "app-a", "--name", "X"],
            &env,
        )
        .await;
        assert_eq!(conflict.status.code(), Some(6));
        assert!(String::from_utf8_lossy(&conflict.stderr).contains("erro [SLUG_ALREADY_EXISTS]"));
        let invalid = blocking(&home, &["settings", "set", "limits.max_batch", "0"], &env).await;
        assert_eq!(invalid.status.code(), Some(2));
        let bad_token = blocking(
            &home,
            &["whoami"],
            &[
                ("ENGRAMIND_URL", url.as_str()),
                ("ENGRAMIND_TOKEN", "eng.x"),
            ],
        )
        .await;
        assert_eq!(bad_token.status.code(), Some(3));
        let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let closed_url = format!("http://{}", closed.local_addr().expect("addr"));
        drop(closed);
        let offline = blocking(
            &home,
            &["whoami", "--json"],
            &[
                ("ENGRAMIND_URL", closed_url.as_str()),
                ("ENGRAMIND_TOKEN", ADMIN_TOKEN),
            ],
        )
        .await;
        assert_eq!(offline.status.code(), Some(8), "{}", text(&offline));
        assert_eq!(
            stdout_json(&offline)["error"]["code"],
            "CLI_CONNECTION_FAILED"
        );
        assert!(!text(&offline).contains(ADMIN_TOKEN));

        // Perfis: arquivo 0600, token mascarado, perfil padrão e precedência da flag.
        let added = blocking(
            &home,
            &[
                "profiles",
                "add",
                "admin",
                "--url",
                url.as_str(),
                "--token",
                ADMIN_TOKEN,
            ],
            &[],
        )
        .await;
        assert!(added.status.success(), "{}", text(&added));
        let file = home.join("engramind/config.toml");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&file)
                .expect("config")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let listed = blocking(&home, &["profiles", "list", "--json"], &[]).await;
        let profiles = stdout_json(&listed);
        assert_eq!(profiles["profiles"][0]["token"], "eng.tk_00000001.***");
        assert_eq!(profiles["profiles"][0]["default"], true);
        assert!(!text(&listed).contains(ADMIN_TOKEN));

        let via_profile = blocking(&home, &["whoami", "--json"], &[]).await;
        assert!(via_profile.status.success(), "{}", text(&via_profile));
        let env_wins = blocking(&home, &["whoami"], &[("ENGRAMIND_TOKEN", "eng.x")]).await;
        assert_eq!(env_wins.status.code(), Some(3), "a variavel vence o perfil");
        let flag_wins = blocking(
            &home,
            &["whoami", "--json", "--token", ADMIN_TOKEN],
            &[("ENGRAMIND_TOKEN", "eng.x")],
        )
        .await;
        assert!(flag_wins.status.success(), "a flag vence a variavel");

        let unknown = blocking(&home, &["whoami", "--profile", "outro"], &[]).await;
        assert_eq!(unknown.status.code(), Some(2));
        let used = blocking(&home, &["profiles", "use", "outro"], &[]).await;
        assert_eq!(used.status.code(), Some(2));
        let removed = blocking(&home, &["profiles", "remove", "admin"], &[]).await;
        assert!(removed.status.success(), "{}", text(&removed));
        let gone = blocking(&home, &["whoami"], &[]).await;
        assert_eq!(gone.status.code(), Some(2));
    }
}

/// Critério de sucesso da 002: banco vazio e bootstrap no ambiente; o gestor cria app, agente e
/// token `agent` pela CLI, e o novo token responde `whoami`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_bootstraps_the_first_admin_and_the_cli_provisions_an_agent() {
    use std::process::Stdio;

    const BOOTSTRAP: &str = "eng.tk_0000beef.segredo-do-bootstrap-pelo-ambiente-com-32";
    let dir = tempfile::tempdir().expect("tempdir");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
        .to_string();
    let mut vars = serve_vars();
    vars.retain(|(name, _)| *name != "APP_PORT");
    vars.extend([
        ("APP_HOST", "127.0.0.1"),
        ("APP_PORT", port.as_str()),
        ("BOOTSTRAP_ADMIN_TOKEN", BOOTSTRAP),
        ("LOG_FORMAT", "json"),
    ]);
    let mut server = Command::new(bin());
    server
        .arg("serve")
        .current_dir(dir.path())
        .env_clear()
        .env("LOG_ANSI", "false")
        .env("DATABASE_PATH", dir.path().join("engramind.db"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in &vars {
        server.env(name, value);
    }
    let mut server = server.spawn().expect("spawn serve");
    let url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let mut ready = false;
    for _ in 0..100 {
        if client
            .get(format!("{url}/health/ready"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(ready, "servidor nao ficou pronto");

    let home = dir.path().join("home");
    let cli = |args: Vec<String>, token: String| {
        let home = home.clone();
        let url = url.clone();
        tokio::task::spawn_blocking(move || {
            Command::new(bin())
                .args(&args)
                .arg("--json")
                .env_clear()
                .env("HOME", &home)
                .env("XDG_CONFIG_HOME", &home)
                .env("ENGRAMIND_URL", url)
                .env("ENGRAMIND_TOKEN", token)
                .output()
                .expect("cli")
        })
    };
    let words = |line: &str| line.split(' ').map(str::to_owned).collect::<Vec<_>>();
    for line in [
        "apps create --slug app-a --name AppA",
        "agents create --app app-a --slug curador --name Curador",
    ] {
        let output = cli(words(line), BOOTSTRAP.to_owned()).await.expect("join");
        assert!(output.status.success(), "{line}: {}", text(&output));
    }
    let issued = cli(
        words("tokens issue --agent app-a/curador --scopes read,write"),
        BOOTSTRAP.to_owned(),
    )
    .await
    .expect("join");
    assert!(issued.status.success(), "{}", text(&issued));
    let issued: serde_json::Value = serde_json::from_slice(&issued.stdout).expect("json");
    let secret = issued["secret"].as_str().expect("secret").to_owned();
    let whoami = cli(words("whoami"), secret.clone()).await.expect("join");
    assert!(whoami.status.success(), "{}", text(&whoami));
    let whoami: serde_json::Value = serde_json::from_slice(&whoami.stdout).expect("json");
    assert_eq!(whoami["agent"]["slug"], "app-a/curador");
    assert_eq!(whoami["kind"], "agent");

    server.kill().expect("kill");
    let output = server.wait_with_output().expect("output");
    let logs = String::from_utf8_lossy(&output.stdout);
    assert!(logs.contains("security.bootstrap_created"), "{logs}");
    for forbidden in [BOOTSTRAP, secret.as_str(), "segredo-do-bootstrap"] {
        assert!(!logs.contains(forbidden), "{forbidden} no log");
    }
}
