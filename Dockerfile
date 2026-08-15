FROM rust:1.85-bookworm AS builder
WORKDIR /app

COPY Cargo.toml rust-toolchain.toml ./
COPY crates ./crates

RUN cargo build --workspace --release

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 wsc
WORKDIR /home/wsc
COPY --from=builder /app/target/release/wsc /usr/local/bin/wsc
USER wsc
ENTRYPOINT ["wsc"]
