use std::net::SocketAddr;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read server.conf: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid server.conf: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid bind address '{0}'")]
    Bind(String),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawConfig {
    mc_version:          String,
    bind:                String,
    online_mode:         bool,
    max_players:         u32,
    view_distance:       u8,
    simulation_distance: u8,
    motd:                String,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub mc_version:          String,
    pub bind:                SocketAddr,
    pub max_players:         u32,
    pub view_distance:       u8,
    pub simulation_distance: u8,
    pub motd:                String,
}

impl ServerConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    pub fn parse(contents: &str) -> Result<Self, ConfigError> {
        let raw: RawConfig = toml::from_str(contents)?;
        if raw.online_mode {
            return Err(ConfigError::Invalid(
                "onlineMode=true is not supported yet; use offline mode".into(),
            ));
        }
        if raw.max_players == 0 || raw.max_players > 100_000 {
            return Err(ConfigError::Invalid("maxPlayers must be between 1 and 100000".into()));
        }
        if !(2..=32).contains(&raw.view_distance) {
            return Err(ConfigError::Invalid("viewDistance must be between 2 and 32".into()));
        }
        if !(2..=32).contains(&raw.simulation_distance) || raw.simulation_distance > raw.view_distance {
            return Err(ConfigError::Invalid("simulationDistance must be between 2 and viewDistance".into()));
        }
        if raw.motd.len() > 32767 {
            return Err(ConfigError::Invalid("motd is too long".into()));
        }
        let bind = raw
            .bind
            .parse()
            .map_err(|_| ConfigError::Bind(raw.bind.clone()))?;
        Ok(Self {
            mc_version: raw.mc_version,
            bind,
            max_players: raw.max_players,
            view_distance: raw.view_distance,
            simulation_distance: raw.simulation_distance,
            motd: raw.motd,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ServerConfig;

    const VALID: &str = "mcVersion = '1.19.2'\nbind = '127.0.0.1:25565'\nonlineMode = false\nmaxPlayers = 20\nviewDistance = 4\nsimulationDistance = 4\nmotd = 'RustBukkit'\n";

    #[test]
    fn parses_server_conf() {
        let config = ServerConfig::parse(VALID).unwrap();
        assert_eq!(config.mc_version, "1.19.2");
        assert_eq!(config.bind.port(), 25565);
    }

    #[test]
    fn rejects_online_mode_and_invalid_distance() {
        assert!(ServerConfig::parse(&VALID.replace("onlineMode = false", "onlineMode = true")).is_err());
        assert!(ServerConfig::parse(&VALID.replace("viewDistance = 4", "viewDistance = 1")).is_err());
    }
}
