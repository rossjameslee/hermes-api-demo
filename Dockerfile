# Multi-stage build for Hermes API (Rust)
FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY docs ./docs
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/hermes-api-rs /usr/local/bin/hermes-api-rs
ENV PORT=8000
EXPOSE 8000
CMD ["hermes-api-rs"]

