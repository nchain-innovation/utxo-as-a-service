use std::collections::HashMap;

/*
    NODE_NONE = 0,
    // Nothing

    NODE_NETWORK = (1 << 0),
    // NODE_NETWORK means that the node is capable of serving the block chain.
    // It is currently set by all Bitcoin SV nodes, and is unset by SPV clients
    // or other peers that just want network services but don't provide them.

    NODE_GETUTXO = (1 << 1),
    // NODE_GETUTXO means the node is capable of responding to the getutxo
    // protocol request. Bitcoin SV does not support this but a patch set
    // called Bitcoin XT does. See BIP 64 for details on how this is
    // implemented.

    NODE_BLOOM = (1 << 2),
    // NODE_BLOOM means the node is capable and willing to handle bloom-filtered
    // connections. Bitcoin SV nodes used to support this by default, without
    // advertising this bit, but no longer do as of protocol version 70011 (=
    // NO_BLOOM_VERSION)

    "NODE_WITNESS": (1 << 3),
    BIP 0144 - This BIP defines new messages and serialization formats for propagation of transactions and blocks committing to segregated witness structures.

    NODE_XTHIN = (1 << 4),
    // NODE_XTHIN means the node supports Xtreme Thinblocks. If this is turned
    // off then the node will not service nor make xthin requests.

    NODE_BITCOIN_CASH = (1 << 5),
    // NODE_BITCOIN_CASH means the node supports Bitcoin Cash and the
    // associated consensus rule changes.
    // This service bit is intended to be used prior until some time after the
    // UAHF activation when the Bitcoin Cash network has adequately separated.
    // TODO: remove (free up) the NODE_BITCOIN_CASH service bit once no longer
    // needed.

    NODE_NETWORK_LIMITED
    https://github.com/bitcoin/bips/blob/master/bip-0159.mediawiki
    if signaled, the peer MUST be capable of serving at least the last 288 blocks (~2 days).
*/

lazy_static! {
    static ref SERVICE_FLAGS: HashMap<u64, &'static str> = [
            ((1 << 0), "NODE_NETWORK"),
            ((1 << 1), "NODE_GETUTXO"),
            ((1 << 2), "NODE_BLOOM"),
            ((1 << 3), "NODE_WITNESS"),
            ((1 << 4), "NODE_XTHIN"),
            ((1 << 5), "NODE_BITCOIN_CASH"),
            ((1 << 6), "NODE_COMPACT_FILTERS"),     // 64
            ((1 << 10), "NODE_NETWORK_LIMITED"),
        ].iter().rev().copied().collect();
}

pub fn decode_services(nservice: u64) -> Vec<&'static str> {
    // Given a service value return a vec of strings
    if nservice != 0 {
        let mut retval: Vec<&str> = Vec::new();
        for (key, value) in SERVICE_FLAGS.iter() {
            if (nservice & key) > 0 {
                retval.push(*value);
            }
        }
        retval
    } else {
        vec!["NODE_NONE"]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::decode_services;

    #[test]
    fn test_all() {
        let mut tests: HashMap<u64, Vec<&'static str>> = HashMap::new();
        tests.insert(0, vec!["NODE_NONE"]);
        tests.insert(24, vec!["NODE_XTHIN", "NODE_WITNESS"]);
        tests.insert(25, vec!["NODE_WITNESS", "NODE_NETWORK", "NODE_XTHIN"]);
        tests.insert(37, vec!["NODE_NETWORK", "NODE_BLOOM", "NODE_BITCOIN_CASH"]);
        tests.insert(
            1061,
            vec![
                "NODE_NETWORK",
                "NODE_BLOOM",
                "NODE_BITCOIN_CASH",
                "NODE_NETWORK_LIMITED",
            ],
        );
        tests.insert(
            1069,
            vec![
                "NODE_NETWORK",
                "NODE_BLOOM",
                "NODE_WITNESS",
                "NODE_BITCOIN_CASH",
                "NODE_NETWORK_LIMITED",
            ],
        );

        for (key, value) in tests.into_iter() {
            let mut a: Vec<&str> = decode_services(key);
            let mut b: Vec<&str> = value;
            a.sort();
            b.sort();
            assert_eq!(a, b);
        }
    }
}
