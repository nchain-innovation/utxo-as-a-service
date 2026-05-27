use chain_gang::{network::Network, util::Hash256};
use serde::{Deserialize, Serialize};
use std::{env, io, net::IpAddr};

#[derive(Debug, Deserialize, Clone)]
pub struct Service {
    pub user_agent: String,
    pub network: String,
    pub rust_address: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkSettings {
    pub ip: Vec<String>,
    pub port: u16,
    pub timeout_period: f64,
    pub start_block_hash: String,
    pub start_block_height: u32,
    pub startup_load_from_database: bool,
    pub block_file: String,
    pub save_blocks: bool,
    pub save_txs: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub mysql_url: String,
    pub mysql_url_docker: String,
    pub ms_delay: u64,
    pub retries: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrphanConfig {
    pub detect: bool,
    pub threshold: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    pub level: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct CollectionConfig {
    pub name: String,
    pub track_descendants: bool,
    pub address: Option<String>,
    pub locking_script_pattern: Option<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct DynamicConfigConfig {
    pub filename: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WebInterfaceConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub rate_limit_per_minute: u32,
    #[serde(default = "default_max_broadcast_tx_bytes")]
    pub max_broadcast_tx_bytes: usize,
}

fn default_max_broadcast_tx_bytes() -> usize {
    1_000_000
}

impl Default for WebInterfaceConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            rate_limit_per_minute: 0,
            max_broadcast_tx_bytes: default_max_broadcast_tx_bytes(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub service: Service,
    pub mainnet: NetworkSettings,
    pub testnet: NetworkSettings,
    pub database: DatabaseConfig,
    pub orphan: OrphanConfig,
    pub logging: LoggingConfig,
    pub dynamic_config: DynamicConfigConfig,

    #[serde(default)]
    pub web_interface: WebInterfaceConfig,

    pub collection: Vec<CollectionConfig>,
}

impl Config {
    pub fn get_network(&self) -> Result<Network, &str> {
        match self.service.network.as_str() {
            "mainnet" => Ok(Network::BSV_Mainnet),
            "testnet" => Ok(Network::BSV_Testnet),
            "stn" => Ok(Network::BSV_STN),
            _ => Err("unable to decode network"),
        }
    }

    pub fn get_network_settings(&self) -> Result<&NetworkSettings, &'static str> {
        match self.service.network.as_str() {
            "mainnet" => Ok(&self.mainnet),
            "testnet" => Ok(&self.testnet),
            "stn" => Err("no settings for STN"),
            _ => Err("unable to decode network"),
        }
    }

    pub fn get_ips(&self) -> Result<Vec<IpAddr>, String> {
        let mut ip_list: Vec<IpAddr> = Vec::new();
        for ip in self
            .get_network_settings()
            .map_err(|e| e.to_string())?
            .ip
            .iter()
        {
            ip.parse()
                .map(|value| ip_list.push(value))
                .map_err(|_| format!("unable to parse ip address '{ip}'"))?;
        }
        Ok(ip_list)
    }

    pub fn validate_startup(&self) -> Result<(), String> {
        let settings = self.get_network_settings().map_err(|err| err.to_string())?;
        if settings.ip.is_empty() {
            return Err("network ip list must not be empty".into());
        }
        self.get_ips()?;
        self.get_network().map_err(|err| err.to_string())?;
        Hash256::decode(&settings.start_block_hash).map_err(|err| {
            format!(
                "invalid start_block_hash '{}': {err:?}",
                settings.start_block_hash
            )
        })?;
        Ok(())
    }

    pub fn get_mysql_url(&self) -> &str {
        // Return the sql_url for the current environment

        // APP_ENV=docker means that we are in docker, otherwise we are on raw machine :-)
        match env::var_os("APP_ENV") {
            Some(_) => &self.database.mysql_url_docker,
            None => &self.database.mysql_url,
        }
    }

    // Return the log level (as a log::Level type) from the config
    pub fn get_log_level(&self) -> log::Level {
        match self.logging.level.as_str() {
            "error" => log::Level::Error,
            "warn" | "warning" => log::Level::Warn,
            "info" | "information" => log::Level::Info,
            "debug" => log::Level::Debug,
            "trace" => log::Level::Trace,
            _ => log::Level::Warn,
        }
    }
}

fn read_config(filename: &str) -> std::io::Result<Config> {
    // Given filename read the config
    let content = std::fs::read_to_string(filename)?;
    toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

// Example environment var
// BNAR_CONFIG='{"user_agent": "/Bitcoin SV:1.0.9/","ip": ["18.157.234.254",  "65.21.201.45" ], "port": 8333, "network": "Mainnet", "timeout_period": 60.0}'
// cargo run

pub fn get_config(env_var: &str, filename: &str) -> Result<Config, String> {
    match env::var_os(env_var) {
        Some(content) => {
            let val = content
                .into_string()
                .map_err(|_| format!("environment variable {env_var} contains invalid UTF-8"))?;
            serde_json::from_str(&val)
                .map_err(|err| format!("error parsing JSON environment variable {env_var}: {err}"))
        }
        None => read_config(filename).map_err(|err| format!("error reading config file: {err}")),
    }
}
