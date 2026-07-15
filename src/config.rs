//! Config file + env resolution (aliases, credentials, defaults).
//!
//! Precedence: CLI flags > env vars > config file.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::emit::SwbError;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub default_timeout: Option<u64>,
    pub credentials: Option<Credentials>,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Credentials {
    pub token: Option<String>,
    pub secret: Option<String>,
}

/// `$XDG_CONFIG_HOME/swb/config.toml`, fallback `~/.config/swb/config.toml`.
pub fn config_path() -> Option<PathBuf> {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(x) if !x.is_empty() => Some(PathBuf::from(x).join("swb/config.toml")),
        _ => std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".config/swb/config.toml")),
    }
}

/// A missing config file is not an error; an unreadable or invalid one is.
pub fn load() -> Result<Config, SwbError> {
    let Some(path) = config_path() else {
        return Ok(Config::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(SwbError::other(format!(
            "cannot read {}: {e}",
            path.display()
        ))),
    }
}

pub fn parse(text: &str) -> Result<Config, SwbError> {
    toml::from_str(text).map_err(|e| SwbError::other(format!("invalid config: {e}")))
}

impl Config {
    /// Resolve an alias to a device ID; unknown names pass through verbatim.
    pub fn resolve_device(&self, name: &str) -> String {
        self.aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// Env values win over config-file values. The error never echoes
    /// credential values.
    pub fn credentials(
        &self,
        env_token: Option<String>,
        env_secret: Option<String>,
    ) -> Result<(String, String), SwbError> {
        let file = self.credentials.as_ref();
        let token = env_token.or_else(|| file.and_then(|c| c.token.clone()));
        let secret = env_secret.or_else(|| file.and_then(|c| c.secret.clone()));
        match (token, secret) {
            (Some(t), Some(s)) if !t.is_empty() && !s.is_empty() => Ok((t, s)),
            _ => Err(SwbError::auth(
                "missing credentials: set SWITCHBOT_TOKEN and SWITCHBOT_SECRET \
                 or [credentials] in the config file",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let cfg = parse(
            r#"
default_timeout = 30

[credentials]
token = "T"
secret = "S"

[aliases]
living_co2 = "DEVICEID123"
"#,
        )
        .unwrap();
        assert_eq!(cfg.default_timeout, Some(30));
        assert_eq!(cfg.resolve_device("living_co2"), "DEVICEID123");
        assert_eq!(cfg.resolve_device("RAWID"), "RAWID");
        let (t, s) = cfg.credentials(None, None).unwrap();
        assert_eq!((t.as_str(), s.as_str()), ("T", "S"));
    }

    #[test]
    fn empty_config_is_valid() {
        let cfg = parse("").unwrap();
        assert_eq!(cfg.default_timeout, None);
        assert!(cfg.aliases.is_empty());
    }

    #[test]
    fn invalid_toml_is_an_error() {
        let err = parse("default_timeout = [").unwrap_err();
        assert_eq!(err.kind, crate::emit::ErrorKind::Other);
    }

    #[test]
    fn env_credentials_take_precedence() {
        let cfg = parse("[credentials]\ntoken = \"filetok\"\nsecret = \"filesec\"").unwrap();
        let (t, s) = cfg.credentials(Some("envtok".to_string()), None).unwrap();
        assert_eq!((t.as_str(), s.as_str()), ("envtok", "filesec"));
    }

    #[test]
    fn missing_credentials_is_auth_error() {
        let cfg = parse("").unwrap();
        let err = cfg.credentials(None, None).unwrap_err();
        assert_eq!(err.kind, crate::emit::ErrorKind::Auth);
        // Never leak partial secrets into detail.
        assert!(!err.detail.contains("filetok"));
    }

    #[test]
    fn config_path_prefers_xdg() {
        // Only shape-check: exact env manipulation is racy in parallel tests.
        if let Some(p) = config_path() {
            assert!(p.ends_with("swb/config.toml"));
        }
    }
}
