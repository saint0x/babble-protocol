use axum::http::HeaderValue;
use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub public_origin: String,
    pub store_root: PathBuf,
    pub seed_profile: Option<SeedProfile>,
    pub cors_origins: Vec<HeaderValue>,
    pub judgment: crate::provider::JudgmentConfig,
    pub bundle_gateway: Option<crate::gateway::GatewayConfig>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SeedProfile {
    CardFeed,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        moderator_ids_from_env()?;
        let bind_addr = env_socket(
            "BABBLE_API_ADDR",
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8787),
        )?;
        let public_origin = env::var("BABBLE_PUBLIC_ORIGIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("http://{bind_addr}"));
        let store_root = env::var_os("BABBLE_STORE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".babble-node"));
        let seed_profile = match env::var("BABBLE_SEED_PROFILE").ok().as_deref() {
            None | Some("") => None,
            Some("card-feed") => Some(SeedProfile::CardFeed),
            Some(value) => return Err(ConfigError::InvalidSeedProfile(value.to_string())),
        };
        let cors_origins = env::var("BABBLE_CORS_ORIGINS")
            .ok()
            .map(|value| parse_origins(&value))
            .transpose()?
            .unwrap_or_else(default_cors_origins);

        let bundle_gateway = env::var("BABBLE_BUNDLE_GATEWAY_ADDR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                let address = value.parse().map_err(|_| ConfigError::InvalidAddress {
                    name: "BABBLE_BUNDLE_GATEWAY_ADDR",
                    value,
                })?;
                let parents = cors_origins
                    .iter()
                    .map(|value| value.to_str().unwrap_or("").to_owned())
                    .collect::<Vec<_>>();
                crate::gateway::GatewayConfig::loopback(address, &parents)
                    .map_err(ConfigError::InvalidGateway)
            })
            .transpose()?;

        Ok(Self {
            bind_addr,
            public_origin,
            store_root,
            seed_profile,
            cors_origins,
            judgment: crate::provider::JudgmentConfig::from_env()?,
            bundle_gateway,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid BABBLE_MODERATOR_IDS: expected unique comma-separated canonical identity IDs")]
    InvalidModerators,
    #[error("invalid bundle gateway configuration: {0}")]
    InvalidGateway(String),
    #[error("invalid algorithm provider configuration: {0}")]
    InvalidProviderSetting(&'static str),
    #[error("invalid {name}: {value}")]
    InvalidAddress { name: &'static str, value: String },
    #[error("invalid CORS origin: {0}")]
    InvalidCorsOrigin(String),
    #[error("invalid BABBLE_SEED_PROFILE: {0}")]
    InvalidSeedProfile(String),
}

pub(crate) fn moderator_ids_from_env() -> Result<String, ConfigError> {
    let value = match env::var("BABBLE_MODERATOR_IDS") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => String::new(),
        Err(_) => return Err(ConfigError::InvalidModerators),
    };
    babble_graph::moderation::parse_reviewers(&value).map_err(|_| ConfigError::InvalidModerators)?;
    Ok(value)
}

fn env_socket(name: &'static str, default: SocketAddr) -> Result<SocketAddr, ConfigError> {
    let Some(value) = env::var(name).ok().filter(|value| !value.trim().is_empty()) else {
        return Ok(default);
    };
    value.parse().map_err(|_| ConfigError::InvalidAddress {
        name,
        value: value.to_string(),
    })
}

fn parse_origins(value: &str) -> Result<Vec<HeaderValue>, ConfigError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            HeaderValue::from_str(origin)
                .map_err(|_| ConfigError::InvalidCorsOrigin(origin.to_string()))
        })
        .collect()
}

fn default_cors_origins() -> Vec<HeaderValue> {
    [
        "http://127.0.0.1:4321",
        "http://127.0.0.1:4322",
        "http://127.0.0.1:4323",
        "http://127.0.0.1:4324",
        "http://127.0.0.1:4325",
        "http://127.0.0.1:4326",
        "http://127.0.0.1:4327",
        "http://127.0.0.1:4328",
        "http://127.0.0.1:4329",
        "http://localhost:4321",
        "http://localhost:4322",
        "http://localhost:4323",
        "http://localhost:4324",
        "http://localhost:4325",
        "http://localhost:4326",
        "http://localhost:4327",
        "http://localhost:4328",
        "http://localhost:4329",
    ]
    .into_iter()
    .map(HeaderValue::from_static)
    .collect()
}
