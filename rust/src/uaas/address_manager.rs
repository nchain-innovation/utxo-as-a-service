use mysql::prelude::*;
use mysql::*;
use retry::{delay, retry};

use crate::config::Config;
use sv::messages::Addr;

pub struct AddressManager {
    addresses: Vec<String>,
    conn: PooledConn,
}

impl AddressManager {
    pub fn new(_config: &Config, conn: PooledConn) -> Self {
        AddressManager {
            addresses: Vec::new(),
            conn,
        }
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

        if !tables.iter().any(|x| x.as_str() == "addr") {
            println!("Table addr not found - creating");
            self.conn
                .query_drop("CREATE TABLE addr (ip text, services int, port int);")
                .unwrap();
        }
    }

    fn read_table(&mut self) {
        // Read the contents of the database into vec so that we can check for duplicates
        let contents = self
            .conn
            .query_map("SELECT ip  FROM addr", |ip: String| ip)
            .unwrap();
        for c in contents {
            self.addresses.push(c);
        }
    }

    pub fn setup(&mut self) {
        // Do startup setup stuff
        self.create_table();
        self.read_table();
    }

    pub fn on_addr(&mut self, addr: Addr) {
        // On receiving an Addr message
        let addr_insert = self
            .conn
            .prep("INSERT INTO addr (ip, services, port) VALUES (:ip, :services, :port)")
            .unwrap();
        // Got this once, keep an eye to see if occurs more often
        // This appears to be a mysql connection issue
        // Could potentially be caused by the fact that this is the first mysql call made by the service
        // thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: CodecError { Packets out of sync }', src/uaas/address_manager.rs:61:14

        for address in addr.addrs.iter() {
            // Check to see if we have seen this address already
            if !self
                .addresses
                .iter()
                .any(|x| x == &format!("{}", address.addr.ip))
            {
                // if not add it to the table
                let ip_addr = format!("{}", address.addr.ip);

                let result = retry(delay::Fixed::from_millis(200).take(3), || {
                    self.conn.exec_drop(&addr_insert, params! { "ip" => ip_addr.clone() , "services" => address.addr.services, "port" => address.addr.port})
                });
                result.unwrap();

                self.addresses.push(ip_addr);
            }
        }
    }
}
