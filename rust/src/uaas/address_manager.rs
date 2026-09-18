use std::net::IpAddr;

use retry::{delay, retry};

use chain_gang::messages::Addr;

use crate::{config::Config, db::PooledConn};

pub struct AddressManager {
    addresses: Vec<IpAddr>,
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

    fn read_table(&mut self) {
        // `ip` is an `inet` column, so it comes back as an IpAddr rather than
        // as text that has to be reparsed to be compared.
        let rows = match self.conn.query("SELECT ip FROM addr", &[]) {
            Ok(rows) => rows,
            Err(err) => {
                log::error!("Unable to load addr table: {err:?}");
                return;
            }
        };
        for row in rows {
            self.addresses.push(row.get(0));
        }
    }

    pub fn setup(&mut self) {
        // The `addr` table is created by V9__addr.sql. This used to probe
        // INFORMATION_SCHEMA and issue a CREATE TABLE before reading.
        self.read_table();
    }

    pub fn on_addr(&mut self, addr: Addr) {
        if addr.addrs.is_empty() {
            return;
        }

        for address in addr.addrs.iter() {
            // chain-gang carries every address as IPv6, using IPv4-mapped form
            // for IPv4. to_canonical unmaps those, so an IPv4 peer is stored as
            // an IPv4 inet rather than as ::ffff:a.b.c.d.
            let ip = IpAddr::from(address.addr.ip).to_canonical();
            if self.addresses.contains(&ip) {
                continue;
            }

            // `services` is a u64 bitfield on the wire and the column is
            // `bigint`, which is signed. The old column was `int` and silently
            // rejected anything above 2^31. Reject explicitly rather than
            // wrapping: a services flag we cannot represent is a peer we record
            // without, not a negative number written to the database.
            let services = match i64::try_from(address.addr.services) {
                Ok(services) => services,
                Err(_) => {
                    log::warn!(
                        "Peer {ip} advertises services {} which does not fit a signed 64-bit column; recording 0",
                        address.addr.services
                    );
                    0
                }
            };
            // A port is u16, so this widening cannot fail.
            let port = i32::from(address.addr.port);

            let result = retry(
                delay::Fixed::from_millis(self.ms_delay).take(self.retries),
                || {
                    // The table now has `ip` as its primary key, which it did
                    // not before. Seeing the same peer twice is ordinary, so
                    // refresh what we know rather than failing the insert.
                    self.conn.execute(
                        "INSERT INTO addr (ip, services, port) VALUES ($1, $2, $3) \
                         ON CONFLICT (ip) DO UPDATE \
                         SET services = EXCLUDED.services, \
                             port = EXCLUDED.port, \
                             last_seen = now()",
                        &[&ip, &services, &port],
                    )
                },
            );
            if let Err(err) = result {
                log::error!("Unable to insert addr {ip}: {err:?}");
                continue;
            }

            self.addresses.push(ip);
        }
    }
}
