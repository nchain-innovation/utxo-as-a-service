use chain_gang::network::Network;
use serde::Deserialize;
use std::env;
use std::net::IpAddr;

use crate::uaas::collection::Collection;

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkSettings {
    pub ip: Vec<String>,
    pub port: u16,
    pub timeout_period: f64,
    pub block_request_period: u64,
    pub start_block_hash: String,
    pub start_block_height: u32,
    pub startup_load_from_database: bool,
    pub block_file: String,
    pub save_blocks: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub mysql_url: String,
    pub mysql_url_docker: String,
    pub ms_delay: u64,
    pub retries: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Service {
    pub user_agent: String,
    pub network: String,
    pub rust_address: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub service: Service,
    pub mainnet: NetworkSettings,
    pub testnet: NetworkSettings,
    pub database: DatabaseConfig,
    pub collection: Vec<Collection>,
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

    pub fn get_network_settings(&self) -> &NetworkSettings {
        match self.service.network.as_str() {
            "mainnet" => &self.mainnet,
            "testnet" => &self.testnet,
            "stn" => panic!("no settings for STN"),
            _ => panic!("unable to decode network"),
        }
    }

    pub fn get_ips(&self) -> Result<Vec<IpAddr>, &str> {
        let mut ip_list: Vec<IpAddr> = Vec::new();
        for ip in self.get_network_settings().ip.iter() {
            match ip.parse() {
                Ok(value) => ip_list.push(value),
                Err(_) => return Err("unable to parse ip address"),
            }
        }
        Ok(ip_list)
    }

    pub fn get_mysql_url(&self) -> &str {
        // Return the sql_url for the current environment

        // APP_ENV=docker means that we are in docker, otherwise we are on raw machine :-)
        match env::var_os("APP_ENV") {
            Some(_) => &self.database.mysql_url_docker,
            None => &self.database.mysql_url,
        }
    }
}

fn read_config(filename: &str) -> std::io::Result<Config> {
    // Given filename read the config
    let content = std::fs::read_to_string(filename)?;
    Ok(toml::from_str(&content)?)
}

// Example environment var
// BNAR_CONFIG='{"user_agent": "/Bitcoin SV:1.0.9/","ip": ["18.157.234.254",  "65.21.201.45" ], "port": 8333, "network": "Mainnet", "timeout_period": 60.0}'
// cargo run

pub fn get_config(env_var: &str, filename: &str) -> Option<Config> {
    // read config try env var, then filename, panic if fails

    match env::var_os(env_var) {
        Some(content) => {
            let val = content.into_string().unwrap();
            // Parse to Config
            match serde_json::from_str(&val) {
                Ok(config) => Some(config),
                Err(e) => panic!("Error parsing JSON environment var {:?}", e),
            }
        }
        None => {
            // Read config
            let config = match read_config(filename) {
                Ok(config) => config,
                Err(error) => panic!("Error reading config file {:?}", error),
            };
            Some(config)
        }
    }
}
