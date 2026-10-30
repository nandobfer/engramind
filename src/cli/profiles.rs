//! Perfis de conexão em `~/.config/engramind/config.toml` (D-010): URL, token e canais por
//! perfil. O arquivo é gravado com permissão `0600`.

use std::{collections::BTreeMap, fs, io::Write as _, path::PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub url: String,
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

pub fn path() -> anyhow::Result<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join("engramind").join("config.toml"))
        .context("diretorio de configuracao do usuario indisponivel")
}

pub fn load() -> anyhow::Result<ConfigFile> {
    let path = path()?;
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("{} invalido", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(error) => Err(error).with_context(|| format!("falha ao ler {}", path.display())),
    }
}

pub fn save(config: &ConfigFile) -> anyhow::Result<PathBuf> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("falha ao criar {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(config).context("falha ao serializar os perfis")?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("falha ao gravar {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("falha ao proteger {}", path.display()))?;
    }
    file.write_all(text.as_bytes())
        .with_context(|| format!("falha ao gravar {}", path.display()))?;
    Ok(path)
}

pub fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("nome de perfil invalido: use letras, digitos, '-' ou '_' (ate 64)");
    }
    Ok(())
}

/// Token mascarado para listagens: só o prefixo público `eng.<token_id>`.
pub fn mask(token: &str) -> String {
    let mut parts = token.splitn(3, '.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(scheme), Some(id), Some(_)) => format!("{scheme}.{id}.***"),
        _ => "***".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_the_secret_and_round_trips_toml() {
        assert_eq!(mask("eng.tk_00000001.segredo"), "eng.tk_00000001.***");
        assert_eq!(mask("qualquer"), "***");
        let mut config = ConfigFile {
            default_profile: Some("prod".to_owned()),
            ..ConfigFile::default()
        };
        config.profiles.insert(
            "prod".to_owned(),
            Profile {
                url: "http://localhost:8000".to_owned(),
                token: "eng.tk_00000001.x".to_owned(),
                channels: None,
            },
        );
        let text = toml::to_string_pretty(&config).expect("toml");
        assert_eq!(toml::from_str::<ConfigFile>(&text).expect("parse"), config);
        assert!(validate_name("agente-1").is_ok());
        assert!(validate_name("com espaco").is_err());
    }
}
