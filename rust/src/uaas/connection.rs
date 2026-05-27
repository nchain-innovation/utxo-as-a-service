use chrono::Utc;
use mysql::{prelude::*, *};
use retry::{delay, retry};
use std::net::IpAddr;

use crate::config::Config;

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

    fn create_table(&mut self) {
        let tables: Vec<String> = match self.conn.query(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';",
        ) {
            Ok(tables) => tables,
            Err(err) => {
                log::error!("Unable to list database tables for connect log: {err:?}");
                return;
            }
        };

        if !tables.iter().any(|x| x.as_str() == "connect") {
            log::info!("Table connect not found - creating");
            if let Err(err) = self.conn.query_drop(
                "CREATE TABLE connect (date VARCHAR(64), ip VARCHAR(64), event VARCHAR(64));",
            ) {
                log::error!("Unable to create connect table: {err:?}");
            }
        }
    }

    pub fn setup(&mut self) {
        self.create_table();
    }

    fn insert_data(&mut self, ip: &IpAddr, event: &str) {
        let connect_insert = match self
            .conn
            .prep("INSERT INTO connect (date, ip, event) VALUES (:date, :ip, :event)")
        {
            Ok(stmt) => stmt,
            Err(err) => {
                log::error!("Unable to prepare connect insert statement: {err:?}");
                return;
            }
        };

        let date = Utc::now();
        let date_str = date.format("%Y-%m-%d %H:%M:%S").to_string();

        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.exec_drop(
                    &connect_insert,
                    params! {
                        "date" => date_str.clone(),
                        "ip" => ip.to_string(),
                        "event" => event
                    },
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
