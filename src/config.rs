use serde::Deserialize;
use std::net::IpAddr;
use sv::network::Network;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub user_agent: String,
    pub ip: Vec<String>,
    pub port: u16,
    pub network: String,
    pub timeout_period: f64,
}

impl Config {
    pub fn get_network(&self) -> Result<Network, &str> {
        match self.network.as_str() {
            "Mainnet" => Ok(Network::Mainnet),
            "Testnet" => Ok(Network::Testnet),
            "STN" => Ok(Network::STN),
            _ => Err("unable to decode network"),
        }
    }

    pub fn get_ips(&self) -> Result<Vec<IpAddr>, &str> {
        let mut ip_list: Vec<IpAddr> = Vec::new();
        for ip in self.ip.iter() {
            match ip.parse() {
                Ok(value) => ip_list.push(value),
                Err(_) => return Err("unable to parse ip address"),
            }
        }
        Ok(ip_list)
    }
}

pub fn read_config(filename: &str) -> std::io::Result<Config> {
    let content = std::fs::read_to_string(filename)?;
    Ok(toml::from_str(&content)?)
}
