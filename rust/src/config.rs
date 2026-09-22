use chain_gang::{network::Network, util::Hash256};
use serde::{Deserialize, Serialize};
use std::{env, io, net::IpAddr};

/// What a tracked config carries in place of a real credential or address.
///
/// This repository is public, so `data/uaasr*.toml` must not hold a working
/// connection URL or the address of a real node. A deployment supplies those
/// through `UAAS_POSTGRES_URL` or an untracked config; what stays in the file
/// is this marker, so a missing setting fails loudly and says what to set
/// rather than quietly dialling something (CS-450).
pub const CREDENTIAL_PLACEHOLDER: &str = "CHANGE-ME";

/// The peer address the tracked configs ship with.
///
/// TEST-NET-1 (RFC 5737): a valid address that is guaranteed not to route, so
/// the service starts, stays healthy and simply never syncs. That is why the
/// peer list is not treated like the database URL — without a database nothing
/// can work and refusing is right, whereas without a peer everything works
/// except the one thing the operator came for. Refusing there would stop the
/// compose stack coming up from a clean clone, which is a worse default than
/// starting and saying so.
///
/// It warns rather than informs deliberately: `release_max_level_warn`
/// compiles `info!` out of release builds, so an `info!` here would be
/// invisible in exactly the deployment that needs it.
pub const PLACEHOLDER_PEER: &str = "192.0.2.1";

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
    /// libpq connection URL, used when the service runs on the host.
    pub postgres_url: String,
    /// The same database as seen from inside a container.
    pub postgres_url_docker: String,
    pub ms_delay: u64,
    pub retries: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrphanConfig {
    pub detect: bool,
    pub threshold: usize,
}

/// When an unconfirmed spend is given up on (CS-423).
#[derive(Debug, Deserialize, Clone)]
pub struct MempoolConfig {
    /// Blocks an unconfirmed spend may go unmined before its outpoint is
    /// returned to the spendable set and its mempool row removed.
    ///
    /// A policy choice with a cost on both sides. Too low and a slow but valid
    /// spend is briefly counted as spendable while it is still in flight; too
    /// high and a dropped spend under-reports the UTXO set for longer. The
    /// default is 144 — roughly a day at ten-minute blocks — on the basis that
    /// a transaction unmined for a day is not going to be mined.
    ///
    /// `0` disables eviction entirely, which is the pre-CS-423 behaviour.
    #[serde(default = "default_eviction_blocks")]
    pub eviction_blocks: i32,
}

fn default_eviction_blocks() -> i32 {
    144
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            eviction_blocks: default_eviction_blocks(),
        }
    }
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

    /// Defaulted so an existing config file keeps working without an edit.
    #[serde(default)]
    pub mempool: MempoolConfig,

    #[serde(default)]
    pub web_interface: WebInterfaceConfig,

    #[serde(default)]
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
        // Before `get_ips`, which would otherwise report this as an address
        // that failed to parse and leave the reader none the wiser.
        if settings
            .ip
            .iter()
            .any(|ip| ip.contains(CREDENTIAL_PLACEHOLDER))
        {
            return Err(format!(
                "network ip list still contains the {CREDENTIAL_PLACEHOLDER} placeholder. \
                 Set a peer address in a config that is not tracked by git."
            ));
        }
        // Shipped default: valid, unroutable, and useless for syncing. Said
        // once, loudly, rather than left to be inferred from a peer that never
        // connects.
        if settings.ip.iter().any(|ip| ip == PLACEHOLDER_PEER) {
            log::warn!(
                "peer address is the shipped placeholder {PLACEHOLDER_PEER}; the service \
                 will start but will never sync. Set a real peer in a config that is not \
                 tracked by git, or in UAASR_CONFIG."
            );
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

    /// The libpq connection URL for this environment.
    ///
    /// `UAAS_POSTGRES_URL` wins over the config file. That is the same
    /// variable `uaas migrate` already honours, so one setting serves both the
    /// migration and the service, and a deployment never edits a tracked file
    /// to supply a credential. This repository is public (CS-450).
    ///
    /// An empty value counts as unset, so `UAAS_POSTGRES_URL=` in a shell or a
    /// compose file falls back to the config rather than failing to parse.
    ///
    /// Falling back, `APP_ENV=docker` selects the in-container host.
    /// `python/src/database.py::connection_url` makes the same two choices in
    /// the same order, and `test_requirements_source.py` pins that they agree —
    /// a change here needs the same change there.
    pub fn get_postgres_url(&self) -> Result<String, String> {
        if let Some(url) = env::var_os("UAAS_POSTGRES_URL") {
            let url = url.into_string().map_err(|_| {
                "environment variable UAAS_POSTGRES_URL contains invalid UTF-8".to_string()
            })?;
            if !url.is_empty() {
                return Ok(url);
            }
        }

        let (key, url) = match env::var_os("APP_ENV") {
            Some(_) => ("postgres_url_docker", &self.database.postgres_url_docker),
            None => ("postgres_url", &self.database.postgres_url),
        };

        // Refuse rather than dial. The tracked configs carry a placeholder, so
        // reaching here with one means nothing supplied a real URL, and
        // attempting the connection would fail with a parse error that says
        // nothing about what to do.
        if url.contains(CREDENTIAL_PLACEHOLDER) {
            return Err(format!(
                "database.{key} is still the {CREDENTIAL_PLACEHOLDER} placeholder. \
                 Set UAAS_POSTGRES_URL, or point UAASR_CONFIG at a config that is \
                 not tracked by git. Do not commit a real URL: this repository is \
                 public."
            ));
        }
        Ok(url.clone())
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

/// An absolute, `..`-free rendering of `filename`, for an error message.
///
/// `std::path::absolute` joins with the working directory but leaves `..` in
/// place, so the container default comes out as `/app/bin/../data/uaasr.toml`
/// — accurate and useless to someone reading a crash log. `canonicalize` would
/// tidy it but requires the file to exist, which at this call site is the one
/// thing known to be false.
///
/// Purely lexical, so it is wrong if a component is a symlink to elsewhere.
/// That is acceptable for a message; nothing opens this path.
fn display_path(filename: &str) -> String {
    use std::path::{Component, PathBuf};

    let Ok(absolute) = std::path::absolute(filename) else {
        return filename.to_string();
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.display().to_string()
}

pub fn get_config(env_var: &str, filename: &str) -> Result<Config, String> {
    match env::var_os(env_var) {
        Some(content) => {
            let val = content
                .into_string()
                .map_err(|_| format!("environment variable {env_var} contains invalid UTF-8"))?;
            serde_json::from_str(&val)
                .map_err(|err| format!("error parsing JSON environment variable {env_var}: {err}"))
        }
        None => read_config(filename).map_err(|err| {
            // Names the path and what to do about it. The published image
            // carries no config at all (CS-401), so "not found" here is
            // ordinarily a missing mount rather than a missing file, and the
            // bare io::Error does not say which file it was looking for.
            if err.kind() == io::ErrorKind::NotFound {
                // Resolved, because the default is relative and
                // "../data/uaasr.toml" tells an operator looking at a
                // container nothing.
                let resolved = display_path(filename);
                format!(
                    "no configuration at {resolved}. The image ships without one: mount a \
                     directory containing uaasr.toml at /app/data, or set {env_var} to the \
                     configuration as JSON."
                )
            } else {
                format!("error reading config file {filename}: {err}")
            }
        }),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::net::IpAddr;

    pub(crate) fn sample_config() -> Config {
        let content = r#"
            [service]
            user_agent = "/Bitcoin SV:1.0.11/"
            network = "testnet"
            rust_address = "127.0.0.1:8081"

            [mainnet]
            ip = ["127.0.0.1"]
            port = 8333
            start_block_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            start_block_height = 1
            timeout_period = 60.0
            startup_load_from_database = true
            block_file = "../data/main-block.dat"
            save_blocks = false
            save_txs = false

            [testnet]
            ip = ["127.0.0.1", "127.0.0.2"]
            port = 18333
            start_block_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            start_block_height = 1
            timeout_period = 60.0
            startup_load_from_database = false
            block_file = "../data/test-net.dat"
            save_blocks = false
            save_txs = false

            [database]
            postgres_url = "postgresql://local"
            postgres_url_docker = "postgresql://docker"
            ms_delay = 300
            retries = 3

            [orphan]
            detect = false
            threshold = 100

            [logging]
            level = "info"

            [dynamic_config]
            filename = "../data/dynamic.toml"

            [[collection]]
            name = "demo"
            track_descendants = false
            address = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"

            # Selects any p2pkh output and captures the hash160 as the
            # identifier. Present because CS-421 records only what a monitor
            # selects: without a pattern that matches them, the utxo fixtures
            # would be filtered out and the tests would assert nothing.
            [[collection]]
            name = "fixtures"
            track_descendants = false
            locking_script_pattern = "76a914(?<identifier>[0-9a-f]{40})88ac"

            [utxo]
            complete = 6
        "#;
        toml::from_str(content).expect("sample config should parse")
    }

    #[test]
    fn cfg01_reads_config_from_toml_file() {
        let content = r#"
[service]
user_agent = "/Bitcoin SV:1.0.11/"
network = "testnet"
rust_address = "127.0.0.1:8081"

[mainnet]
ip = ["127.0.0.1"]
port = 8333
start_block_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
start_block_height = 1
timeout_period = 60.0
startup_load_from_database = true
block_file = "../data/main-block.dat"
save_blocks = false
save_txs = false

[testnet]
ip = ["127.0.0.1"]
port = 18333
start_block_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
start_block_height = 1
timeout_period = 60.0
startup_load_from_database = false
block_file = "../data/test-net.dat"
save_blocks = false
save_txs = false

[database]
postgres_url = "postgresql://local"
postgres_url_docker = "postgresql://docker"
ms_delay = 300
retries = 3

[orphan]
detect = false
threshold = 100

[logging]
level = "info"

[dynamic_config]
filename = "../data/dynamic.toml"
"#;
        toml::from_str::<Config>(content).expect("inline config should parse");
        let dir = std::env::temp_dir().join(format!("uaas_config_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("uaasr.toml");
        std::fs::write(&path, content).expect("write temp config");
        let config = read_config(path.to_str().unwrap()).expect("config should load from file");
        assert_eq!(config.service.network, "testnet");
    }

    /// Serialises the tests that mutate process environment.
    ///
    /// `set_var` is process-wide and the harness runs tests on several
    /// threads, so without this `cfg03` and `cfg14`–`cfg16` race: one clears
    /// `APP_ENV` or `UAAS_POSTGRES_URL` while another is relying on it. cfg03
    /// predates the others and was already unguarded; it is included here
    /// rather than left as the one that can still lose.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Clears both variables this module sets, so a test starts from a known
    /// state whatever ran before and whatever panicked.
    fn clear_env() {
        unsafe {
            std::env::remove_var("APP_ENV");
            std::env::remove_var("UAAS_POSTGRES_URL");
        }
    }

    #[test]
    fn cfg03_uses_docker_postgres_url_when_app_env_set() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let config = sample_config();
        unsafe {
            std::env::set_var("APP_ENV", "docker");
        }
        let url = config
            .get_postgres_url()
            .expect("sample config has a real url");
        clear_env();
        assert_eq!(url, "postgresql://docker");
    }

    #[test]
    fn cfg14_the_environment_url_wins_over_the_config_file() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let config = sample_config();
        unsafe {
            std::env::set_var("UAAS_POSTGRES_URL", "postgresql://from-the-environment");
        }
        let url = config.get_postgres_url();
        clear_env();
        assert_eq!(
            url.expect("an environment url is accepted"),
            "postgresql://from-the-environment",
            "UAAS_POSTGRES_URL must win, or a deployment has to edit a tracked file"
        );
    }

    #[test]
    fn cfg15_an_empty_environment_url_falls_back_to_the_config() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let config = sample_config();
        unsafe {
            // What `UAAS_POSTGRES_URL=` in a shell or an unset compose
            // interpolation produces. Treated as absent rather than as a URL
            // that cannot parse.
            std::env::set_var("UAAS_POSTGRES_URL", "");
        }
        let url = config.get_postgres_url();
        clear_env();
        assert_eq!(url.expect("falls back"), "postgresql://local");
    }

    #[test]
    fn cfg16_a_placeholder_url_is_refused_and_says_what_to_set() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let mut config = sample_config();
        config.database.postgres_url =
            format!("postgresql://uaas:{CREDENTIAL_PLACEHOLDER}@localhost/uaas_db");
        let err = config
            .get_postgres_url()
            .expect_err("a placeholder must not be dialled");
        clear_env();
        assert!(
            err.contains("UAAS_POSTGRES_URL"),
            "the message must name what to set, got: {err}"
        );
        assert!(
            err.contains("postgres_url"),
            "and which key is at fault, got: {err}"
        );
    }

    #[test]
    fn cfg17_a_placeholder_peer_address_is_refused_before_it_is_parsed() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let mut config = sample_config();
        config.testnet.ip = vec![CREDENTIAL_PLACEHOLDER.to_string()];
        let err = config
            .validate_startup()
            .expect_err("a placeholder peer address must not start the service");
        assert!(
            err.contains(CREDENTIAL_PLACEHOLDER),
            "the message must name the placeholder rather than report a parse \
             failure, got: {err}"
        );
    }

    #[test]
    fn cfg18_the_shipped_placeholder_peer_still_starts_the_service() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let mut config = sample_config();
        config.testnet.ip = vec![PLACEHOLDER_PEER.to_string()];
        // Unroutable, so it never syncs — but it must not stop startup, or
        // `docker compose up` fails from a clean clone.
        config
            .validate_startup()
            .expect("the shipped placeholder peer must not refuse startup");
    }

    #[test]
    fn cfg04_reads_active_network_port_and_ips() {
        let config = sample_config();
        let settings = config.get_network_settings().expect("testnet settings");
        assert_eq!(settings.port, 18333);
        let ips = config.get_ips().expect("ip list");
        assert_eq!(
            ips,
            vec![
                "127.0.0.1".parse::<IpAddr>().unwrap(),
                "127.0.0.2".parse::<IpAddr>().unwrap(),
            ]
        );
        assert!(!settings.startup_load_from_database);
    }

    #[test]
    fn rel03_validate_startup_rejects_empty_ip_list() {
        let mut config = sample_config();
        config.testnet.ip.clear();
        let err = config
            .validate_startup()
            .expect_err("empty ip list should fail");
        assert!(err.contains("ip list must not be empty"));
    }

    #[test]
    fn sync02_config_provides_multiple_peer_ips_for_failover() {
        let config = sample_config();
        assert!(config.get_ips().expect("ips").len() >= 2);
    }

    #[test]
    fn sync08_startup_load_flag_available_per_network() {
        let config = sample_config();
        assert!(!config.testnet.startup_load_from_database);
        assert!(config.mainnet.startup_load_from_database);
    }

    /// The published image ships no configuration (CS-401), so a service that
    /// starts without a mount must say so in terms an operator can act on.
    #[test]
    fn cfg11_a_missing_config_file_names_the_path_and_the_remedy() {
        let missing = std::env::temp_dir().join(format!(
            "uaas_absent_config_{}/uaasr.toml",
            std::process::id()
        ));
        let path = missing.to_str().expect("temp path is utf-8");
        assert!(!missing.exists(), "the fixture must not exist");

        let err = get_config("UAAS_CONFIG_ENV_THAT_IS_NOT_SET", path)
            .expect_err("a missing config file must fail");

        assert!(err.contains(path), "the message must name the file: {err}");
        assert!(err.contains("/app/data"), "and where to mount one: {err}");
        assert!(
            err.contains("UAAS_CONFIG_ENV_THAT_IS_NOT_SET"),
            "and the environment variable that overrides it: {err}"
        );
    }

    /// A file that is present but will not parse is a different fault and must
    /// not be reported as a missing mount.
    #[test]
    fn cfg12_a_malformed_config_file_is_not_reported_as_a_missing_mount() {
        let dir = std::env::temp_dir().join(format!("uaas_bad_config_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("uaasr.toml");
        std::fs::write(&path, "this is not toml = = =").expect("write fixture");
        let path_str = path.to_str().expect("temp path is utf-8");

        let err = get_config("UAAS_CONFIG_ENV_THAT_IS_NOT_SET", path_str)
            .expect_err("unparseable TOML must fail");

        assert!(
            err.contains(path_str),
            "the message must name the file: {err}"
        );
        assert!(
            !err.contains("/app/data"),
            "a parse failure is not a missing mount: {err}"
        );

        std::fs::remove_file(&path).ok();
    }

    /// The message is read off a crash log, so the path in it has to be one an
    /// operator can act on without knowing the working directory.
    #[test]
    fn cfg13_the_reported_path_is_absolute_and_free_of_parent_components() {
        let shown = display_path("../data/uaasr.toml");
        assert!(shown.starts_with('/'), "must be absolute, got {shown}");
        assert!(
            !shown.contains(".."),
            "must not leave .. for the reader to resolve, got {shown}"
        );
        assert!(shown.ends_with("/data/uaasr.toml"), "got {shown}");

        // An already-absolute path is left as it is.
        assert_eq!(display_path("/app/data/uaasr.toml"), "/app/data/uaasr.toml");
    }
}
