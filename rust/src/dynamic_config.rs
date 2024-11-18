use crate::config::{CollectionConfig, Config};
use serde::{Deserialize, Serialize};

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
    let config: DynamicConfigConfig = toml::from_str(&content)?;
    Ok(config.collection)
}

fn save_dynamic_config(filename: &str, clients: &[CollectionConfig]) -> std::io::Result<()> {
    let config = DynamicConfigConfig {
        collection: clients.to_vec(),
    };
    let content = toml::to_string(&config).unwrap();
    std::fs::write(filename, content)?;
    Ok(())
}

impl DynamicConfig {
    pub fn new(config: &Config) -> Self {
        let filename = config.dynamic_config.filename.clone();

        let collection = match read_dynamic_config(&filename) {
            Ok(clients) => clients,
            Err(e) => {
                println!("error {:?}", e);
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
        save_dynamic_config(&self.filename, &self.collection).unwrap();
    }
}
