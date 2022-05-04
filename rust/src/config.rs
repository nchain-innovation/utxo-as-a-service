use serde::Deserialize;
use std::env;
use std::net::IpAddr;
use sv::network::Network;

#[derive(Debug, Deserialize, Clone)]
pub struct Service {
    pub user_agent: String,
    pub ip: Vec<String>,
    pub port: u16,
    pub network: String,
    pub timeout_period: f64,
    pub mysql_url: String,
    pub start_block_hash: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Shared {
    pub block_file: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub service: Service,
    pub shared: Shared,
}

impl Config {
    pub fn get_network(&self) -> Result<Network, &str> {
        match self.service.network.as_str() {
            "Mainnet" => Ok(Network::Mainnet),
            "Testnet" => Ok(Network::Testnet),
            "STN" => Ok(Network::STN),
            _ => Err("unable to decode network"),
        }
    }

    pub fn get_ips(&self) -> Result<Vec<IpAddr>, &str> {
        let mut ip_list: Vec<IpAddr> = Vec::new();
        for ip in self.service.ip.iter() {
            match ip.parse() {
                Ok(value) => ip_list.push(value),
                Err(_) => return Err("unable to parse ip address"),
            }
        }
        Ok(ip_list)
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