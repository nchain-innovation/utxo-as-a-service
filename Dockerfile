FROM rust:1.60.0 AS build_rust

WORKDIR /app
COPY rust .
RUN cargo build --release


CMD ["./target/release/bnar"]