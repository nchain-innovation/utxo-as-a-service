use crate::config::{CollectionConfig, Config};
use serde::{Deserialize, Serialize};
use std::io;

// Represents the service's dynamically configurable elements
#[derive(Debug, Clone)]
pub struct DynamicConfig {
    filename: String,
    pub collection: Vec<CollectionConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DynamicConfigConfig {
    pub collection: Vec<CollectionConfig>,
}

fn read_dynamic_config(filename: &str) -> std::io::Result<Vec<CollectionConfig>> {
    let content = std::fs::read_to_string(filename)?;
    let config: DynamicConfigConfig =
        toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(config.collection)
}

fn save_dynamic_config(filename: &str, clients: &[CollectionConfig]) -> std::io::Result<()> {
    let config = DynamicConfigConfig {
        collection: clients.to_vec(),
    };
    let content =
        toml::to_string(&config).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(filename, content)?;
    Ok(())
}

impl DynamicConfig {
    pub fn new(config: &Config) -> Self {
        let filename = config.dynamic_config.filename.clone();

        let collection = match read_dynamic_config(&filename) {
            Ok(clients) => clients,
            Err(e) => {
                log::error!("Error reading dynamic config file {:?}", e);
                Vec::new()
            }
        };

        DynamicConfig {
            filename,
            collection,
        }
    }

    pub fn add(&mut self, monitor: &CollectionConfig) {
        log::info!("add monitor {:?}", &monitor);

        self.collection.push(monitor.clone());
        self.save();
    }

    pub fn delete(&mut self, name: &str) {
        if let Some(index) = self.collection.iter().position(|c| c.name == name) {
            log::info!("delete monitor {}", name);

            self.collection.remove(index);
            self.save();
        }
    }

    fn save(&self) {
        if let Err(err) = save_dynamic_config(&self.filename, &self.collection) {
            log::error!(
                "Unable to save dynamic config to {}: {err:?}",
                self.filename
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        CollectionConfig, Config, DatabaseConfig, DynamicConfigConfig as RootDynamicConfigConfig,
        LoggingConfig, NetworkSettings, OrphanConfig, Service, WebInterfaceConfig,
    };

    fn sample_root_config(filename: &str) -> Config {
        Config {
            service: Service {
                user_agent: "/Bitcoin SV:1.0.11/".to_string(),
                network: "testnet".to_string(),
                rust_address: "127.0.0.1:8081".to_string(),
            },
            mainnet: NetworkSettings {
                ip: vec!["127.0.0.1".to_string()],
                port: 8333,
                timeout_period: 60.0,
                start_block_hash: "a".repeat(64),
                start_block_height: 1,
                startup_load_from_database: true,
                block_file: "../data/main-block.dat".to_string(),
                save_blocks: false,
                save_txs: false,
            },
            testnet: NetworkSettings {
                ip: vec!["127.0.0.1".to_string()],
                port: 18333,
                timeout_period: 60.0,
                start_block_hash: "b".repeat(64),
                start_block_height: 1,
                startup_load_from_database: false,
                block_file: "../data/test-net.dat".to_string(),
                save_blocks: false,
                save_txs: false,
            },
            database: DatabaseConfig {
                mysql_url: "mysql://local".to_string(),
                mysql_url_docker: "mysql://docker".to_string(),
                ms_delay: 300,
                retries: 3,
            },
            orphan: OrphanConfig {
                detect: false,
                threshold: 100,
            },
            logging: LoggingConfig {
                level: "info".to_string(),
            },
            dynamic_config: RootDynamicConfigConfig {
                filename: filename.to_string(),
            },
            web_interface: WebInterfaceConfig::default(),
            collection: Vec::new(),
        }
    }

    #[test]
    fn cfg06_add_monitor_persists_to_dynamic_config_file() {
        let dir =
            std::env::temp_dir().join(format!("uaas_dynamic_config_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("dynamic.toml");
        let config = sample_root_config(path.to_str().unwrap());
        let mut dynamic = DynamicConfig::new(&config);
        dynamic.add(&CollectionConfig {
            name: "runtime-monitor".to_string(),
            track_descendants: false,
            address: Some("mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5".to_string()),
            locking_script_pattern: None,
        });
        let saved = std::fs::read_to_string(&path).expect("dynamic config file");
        assert!(saved.contains("runtime-monitor"));
    }
}
