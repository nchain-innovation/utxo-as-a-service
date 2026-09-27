# A real candidate export, produced by the real scanner

Not hand-written. These three files came out of `bitcoin_cdbwrapper`'s
`scan_chainstate` run over that repository's own synthetic fixture database:

```
make_fixture_db chainstate
scan_chainstate --db chainstate --out-dir out --patterns-file data/scan_patterns.toml
```

That matters. A fixture written by hand to match this reader would pass
whatever the reader misunderstood about the format — the two would agree
because one was copied from the other. These bytes were written by the
producer, so a test that reads them is a test of the contract rather than of
my reading of it.

Eight chainstate records, three of them carrying the 1SAT ordinal envelope
`0063036f7264`. The tip hash is 32 bytes of `0x11`, which is the fixture
database's `B` record and not a real block.

Regenerate with the commands above if the format changes, and change
`SUPPORTED_FORMAT_VERSION` in `src/candidate_export.rs` at the same time.
