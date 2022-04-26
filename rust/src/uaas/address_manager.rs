use mysql::prelude::*;
use mysql::*;

use crate::config::Config;
use sv::messages::Addr;

pub struct AddressManager {
    addresses: Vec<String>,
    conn: PooledConn,
}

impl AddressManager {
    pub fn new(_config: &Config, conn: PooledConn) -> Self {
        let mut a = AddressManager {
            addresses: Vec::new(),
            conn: conn,
        };
        a.create_table();
        a.read_table();
        a
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

        if tables.iter().find(|x| x.as_str() == "addr") == None {
            self.conn
                .query_drop(
                    r"CREATE TABLE addr (
                ip text,
                services int,
                port int
            )",
                )
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

    pub fn on_addr(&mut self, addr: Addr) {
        // On receiving an Addr message
        let addr_insert = self
            .conn
            .prep("INSERT INTO addr (ip, services, port) VALUES (:ip, :services, :port)")
            .unwrap();

        for address in addr.addrs.iter() {
            // Check to see if we have seen this address already
            if self
                .addresses
                .iter()
                .find(|x| *x == &format!("{}", address.addr.ip))
                == None
            {
                // if not add it to the table
                let ip_addr = format!("{}", address.addr.ip);

                self.conn.exec_drop(&addr_insert,
                    params! { "ip" => ip_addr.clone() , "services" => address.addr.services, "port" => address.addr.port} ).unwrap();
                self.addresses.push(ip_addr);
            }
        }
    }
}
