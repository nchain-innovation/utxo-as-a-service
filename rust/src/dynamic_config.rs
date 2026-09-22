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

/// How loudly to report a failure to read the dynamic config.
///
/// A missing file is the ordinary state of a fresh deployment:
/// `data/dynamic.toml` holds monitors added at runtime through
/// `POST /collection/monitor`, and until one is added it does not exist.
/// Reporting that at `error` on every first run — and on every run of the test
/// suite — teaches operators that `error` lines are noise, which is exactly
/// when a real one gets scrolled past.
///
/// Anything else is worth an error and must stay one. A file that exists but
/// cannot be read, or that is there but will not parse, means the operator's
/// monitors have silently vanished; the service carries on with an empty
/// collection list either way, so the log line is the only evidence.
///
/// A TOML parse failure arrives here as `InvalidData`, from the conversion in
/// [`read_dynamic_config`].
///
/// A separate function because the branch is the whole point of this change
/// and `log::error!` leaves nothing a test can assert on.
fn level_for(err: &io::Error) -> log::Level {
    match err.kind() {
        io::ErrorKind::NotFound => log::Level::Info,
        _ => log::Level::Error,
    }
}

impl DynamicConfig {
    pub fn new(config: &Config) -> Self {
        let filename = config.dynamic_config.filename.clone();

        let collection = match read_dynamic_config(&filename) {
            Ok(clients) => clients,
            Err(err) => {
                // One call site, two levels, so the message and the level
                // cannot drift apart.
                log::log!(
                    level_for(&err),
                    "{}",
                    match err.kind() {
                        io::ErrorKind::NotFound => format!(
                            "No dynamic config at {filename}; starting with no runtime monitors"
                        ),
                        _ => format!(
                            "Unable to read dynamic config {filename}, so any monitors it \
                             held are not loaded: {err:?}"
                        ),
                    }
                );
                Vec::new()
            }
        };

        DynamicConfig {
            filename,
            collection,
        }
    }

    pub fn add(&mut self, monitor: &CollectionConfig) {
        log::info!("add monitor {:?}", monitor);

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
    use crate::config::MatchProperty;
    use crate::config::{
        CollectionConfig, Config, DatabaseConfig, DynamicConfigConfig as RootDynamicConfigConfig,
        LoggingConfig, MempoolConfig, NetworkSettings, OrphanConfig, Service, WebInterfaceConfig,
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
                postgres_url: "postgresql://local".to_string(),
                postgres_url_docker: "postgresql://docker".to_string(),
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
            mempool: MempoolConfig::default(),
            web_interface: WebInterfaceConfig::default(),
            collection: Vec::new(),
        }
    }

    #[test]
    fn cfg06_add_monitor_persists_to_dynamic_config_file() {
        let dir =
            std::env::temp_dir().join(format!("uaas_dynamic_config_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir for dynamic config test");
        let path = dir.join("dynamic.toml");
        let config = sample_root_config(path.to_str().unwrap());
        let mut dynamic = DynamicConfig::new(&config);
        dynamic.add(&CollectionConfig {
            name: "runtime-monitor".to_string(),
            track_descendants: false,
            address: Some("mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5".to_string()),
            locking_script_pattern: None,
            require: MatchProperty::BytesPresent,
        });
        let saved = std::fs::read_to_string(&path).expect("dynamic config file");
        assert!(saved.contains("runtime-monitor"));
    }

    /// A fresh deployment has no `data/dynamic.toml` until a monitor is added
    /// through the API. That is not a failure, and reporting it as one on
    /// every first run devalues every other error line.
    #[test]
    fn cfg07_a_missing_dynamic_config_is_not_an_error() {
        let missing = std::env::temp_dir().join(format!(
            "uaas_dynamic_config_absent_{}/dynamic.toml",
            std::process::id()
        ));
        assert!(!missing.exists(), "the fixture must not exist");

        let err = read_dynamic_config(missing.to_str().unwrap())
            .expect_err("reading a file that is not there must fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(level_for(&err), log::Level::Info);
    }

    /// The other half, and the one that must not be quietened: a file that is
    /// present but unreadable or malformed means the operator's monitors have
    /// silently vanished. The service carries on with an empty collection
    /// either way, so the log line is the only evidence there is.
    #[test]
    fn cfg08_a_malformed_dynamic_config_is_still_an_error() {
        let dir = std::env::temp_dir().join(format!(
            "uaas_dynamic_config_malformed_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("dynamic.toml");
        std::fs::write(&path, "this is not toml = = =").expect("write malformed fixture");

        let err =
            read_dynamic_config(path.to_str().unwrap()).expect_err("unparseable TOML must fail");
        assert_ne!(
            err.kind(),
            io::ErrorKind::NotFound,
            "a parse failure must not be mistaken for an absent file"
        );
        assert_eq!(level_for(&err), log::Level::Error);

        std::fs::remove_file(&path).ok();
    }

    /// A permissions failure is not a parse failure and not an absent file,
    /// and it means the same thing as a malformed one: monitors lost.
    #[test]
    fn cfg09_an_unreadable_dynamic_config_is_an_error() {
        // Constructed rather than provoked: making a file genuinely unreadable
        // needs a chmod that does nothing when the suite runs as root, which
        // it does in some CI images.
        let err = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(level_for(&err), log::Level::Error);
    }

    /// Construction against a missing file still yields a usable, empty
    /// config — the reporting changed, the behaviour did not.
    #[test]
    fn cfg10_a_missing_file_still_starts_with_no_monitors() {
        let missing = std::env::temp_dir().join(format!(
            "uaas_dynamic_config_absent_new_{}/dynamic.toml",
            std::process::id()
        ));
        let config = sample_root_config(missing.to_str().unwrap());
        let dynamic = DynamicConfig::new(&config);
        assert!(dynamic.collection.is_empty());
    }
}
