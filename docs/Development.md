
# Rust Development
As noted in the main README.md  the best way to install Rust is to use `rustup`.
This code was developed using rustc `1.60.0`.

Once installed update rust toolset using:
```bash
rustup update
```

To run unit tests:
```bash
cd rust
cargo test
```

To format the code:
```bash
cd rust
cargo fmt
```

For Rust hints:
```bash
cd rust
cargo clippy
```

# Python Development
To lint the source code use the following command line script from the project root directory:
```
$ ./lint.sh
```
This requires `flake8` and `mypy` to be installed to perform the static code analysis.

# Background Links
Details of the messages and the Bitcoin SV peer to peer protocol can be found in the following links:

* https://wiki.bitcoinsv.io/index.php/Peer-To-Peer_Protocol
* https://developer.bitcoin.org/reference/p2p_networking.html

# Peer Thread Status States
The peer thread works through the following states:

![States](diagrams/threadstates.png)

