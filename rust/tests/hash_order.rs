//! The byte order the two components agree on for a txid.
//!
//! This is half of a cross-language pin. The other half is
//! `python/tests/test_hashes.py::test_hash12_the_stored_order_matches_the_rust_side`,
//! which asserts the same two literals from the Python side. Neither can change
//! its convention without the other failing.
//!
//! Why it needs pinning: the Rust service binds `Hash256.0` — internal order —
//! into the `bytea` columns, while the REST API speaks the reversed, display
//! order conventional for a Bitcoin txid. Before the PostgreSQL migration the
//! database held `Hash256::encode()`, which is display order, so the stored
//! representation is now the reverse of what it was. Getting this wrong is
//! silent: the wrong bytes against a `bytea` column match no rows and raise
//! nothing.

use chain_gang::util::Hash256;

/// A txid as the REST API states it.
const DISPLAY: &str = "00000000000000000545267003727771023c9822756f187cbee83a5329ffecd8";
/// The same txid as the database stores it.
const STORED: &str = "d8ecff29533ae8be7c186f7522983c0271777203702645050000000000000000";

#[test]
fn hashorder01_the_bytes_bound_into_bytea_are_internal_order() {
    let hash = Hash256::decode(DISPLAY).expect("a valid txid decodes");
    // `.0` is what every insert in the crate binds; see database.rs and
    // collection.rs, where `encode()` appears only in log messages.
    assert_eq!(
        hex::encode(hash.0),
        STORED,
        "the stored order must stay the reverse of the display order"
    );
}

#[test]
fn hashorder02_the_display_order_survives_a_round_trip() {
    let hash = Hash256::decode(DISPLAY).expect("a valid txid decodes");
    assert_eq!(hash.encode(), DISPLAY);
}

#[test]
fn hashorder03_the_two_orders_actually_differ() {
    // Guards the assertions above against a palindromic fixture, which would
    // let a missing reversal pass both of them.
    assert_ne!(DISPLAY, STORED);
}
