use mysql::prelude::*;
use mysql::*;
use retry::{delay, retry};

use crate::config::Config;
use chain_gang::messages::Addr;

pub struct AddressManager {
    addresses: Vec<String>,
    conn: PooledConn,
    // Retry database connections
    ms_delay: u64,
    retries: usize,
}

impl AddressManager {
    pub fn new(config: &Config, conn: PooledConn) -> Self {
        AddressManager {
            addresses: Vec::new(),
            conn,
            ms_delay: config.database.ms_delay,
            retries: config.database.retries,
        }
    }

    fn create_table(&mut self) {
        let tables: Vec<String> = match self.conn.query(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';",
        ) {
            Ok(tables) => tables,
            Err(err) => {
                log::error!("Unable to list database tables for addr manager: {err:?}");
                return;
            }
        };

        if !tables.iter().any(|x| x.as_str() == "addr") {
            log::info!("Table addr not found - creating");
            if let Err(err) = self
                .conn
                .query_drop("CREATE TABLE addr (ip text, services int, port int);")
            {
                log::error!("Unable to create addr table: {err:?}");
            }
        }
    }

    fn read_table(&mut self) {
        let contents = match self.conn.query_map("SELECT ip  FROM addr", |ip: String| ip) {
            Ok(contents) => contents,
            Err(err) => {
                log::error!("Unable to load addr table: {err:?}");
                return;
            }
        };
        for c in contents {
            self.addresses.push(c);
        }
    }

    pub fn setup(&mut self) {
        self.create_table();
        self.read_table();
    }

    pub fn on_addr(&mut self, addr: Addr) {
        let addr_insert = match self
            .conn
            .prep("INSERT INTO addr (ip, services, port) VALUES (:ip, :services, :port)")
        {
            Ok(stmt) => stmt,
            Err(err) => {
                log::error!("Unable to prepare addr insert statement: {err:?}");
                return;
            }
        };

        for address in addr.addrs.iter() {
            if !self
                .addresses
                .iter()
                .any(|x| x == &format!("{}", address.addr.ip))
            {
                let ip_addr = format!("{}", address.addr.ip);

                let result = retry(
                    delay::Fixed::from_millis(self.ms_delay).take(self.retries),
                    || {
                        self.conn.exec_drop(
                            &addr_insert,
                            params! {
                                "ip" => ip_addr.clone(),
                                "services" => address.addr.services,
                                "port" => address.addr.port
                            },
                        )
                    },
                );
                if let Err(err) = result {
                    log::error!("Unable to insert addr {ip_addr}: {err:?}");
                    continue;
                }

                self.addresses.push(ip_addr);
            }
        }
    }
}
