use std::net::IpAddr;

use retry::{delay, retry};

use crate::{config::Config, db::PooledConn};

pub struct Connection {
    conn: PooledConn,
    // Retry database connections
    ms_delay: u64,
    retries: usize,
}

impl Connection {
    pub fn new(config: &Config, conn: PooledConn) -> Self {
        Connection {
            conn,
            ms_delay: config.database.ms_delay,
            retries: config.database.retries,
        }
    }

    pub fn setup(&mut self) {
        // Nothing to do. The `connect` table is created by V10__connect.sql;
        // this used to probe INFORMATION_SCHEMA and issue a CREATE TABLE.
    }

    fn insert_data(&mut self, ip: &IpAddr, event: &str) {
        // Neither the timestamp nor the id is supplied. `event_time` defaults
        // to now() and `id` is GENERATED ALWAYS AS IDENTITY, which replaces the
        // formatted VARCHAR(64) date this used to build with chrono — that
        // could not be range-queried or sorted without parsing every row.
        //
        // `ip` goes in as an IpAddr against an `inet` column, so the address is
        // validated and stored as an address rather than as text.
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.execute(
                    "INSERT INTO connect (ip, event) VALUES ($1, $2)",
                    &[ip, &event],
                )
            },
        );
        if let Err(err) = result {
            log::error!("Unable to write connect event for {ip}: {err:?}");
        }
    }

    pub fn on_connect(&mut self, ip: &IpAddr) {
        self.insert_data(ip, "Connect")
    }

    pub fn on_disconnect(&mut self, ip: &IpAddr) {
        self.insert_data(ip, "Disconnect")
    }
}
