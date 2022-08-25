use std::net::IpAddr;

use chrono::Utc;
use mysql::prelude::*;
use mysql::*;
use retry::{delay, retry};

use crate::config::Config;

pub struct Connection {
    conn: PooledConn,
}

impl Connection {
    pub fn new(_config: &Config, conn: PooledConn) -> Self {
        Connection { conn }
    }

    fn create_table(&mut self) {
        // Create tables, if required

        // Check for the tables
        let tables: Vec<String> = self
            .conn
            .query(
                "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';",
            )
            .unwrap();

        if !tables.iter().any(|x| x.as_str() == "connect") {
            println!("Table connect not found - creating");
            self.conn
                .query_drop(
                    "CREATE TABLE connect (date VARCHAR(64), ip VARCHAR(64), event VARCHAR(64));",
                )
                .unwrap();
        }
    }

    pub fn setup(&mut self) {
        // Do startup setup stuff
        self.create_table();
    }

    fn insert_data(&mut self, ip: &IpAddr, event: &str) {
        // On receiving an Connect or Disconnect message, write it to the database
        let connect_insert = self
            .conn
            .prep("INSERT INTO connect (date, ip, event) VALUES (:date, :ip, :event)")
            .unwrap();

        let date = Utc::now();
        let date_str = date.format("%Y-%m-%d %H:%M:%S").to_string();

        let result = retry(delay::Fixed::from_millis(200).take(3), || {
            self.conn.exec_drop(
                &connect_insert,
                params! { "date" => date_str.clone() , "ip" => ip.to_string(), "event" => event},
            )
        });
        result.unwrap();
    }

    pub fn on_connect(&mut self, ip: &IpAddr) {
        self.insert_data(ip, "Connect")
    }

    pub fn on_disconnect(&mut self, ip: &IpAddr) {
        self.insert_data(ip, "Disconnect")
    }
}
