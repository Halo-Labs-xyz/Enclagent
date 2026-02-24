# Multi-stage Dockerfile for the Enclagent agent (cloud deployment).
#
# Build:
#   docker build --platform linux/amd64 -t enclagent:latest .
#
# Run:
#   docker run --env-file .env -p 3000:3000 enclagent:latest

# Stage 1: Build
FROM rust:1.92-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake gcc g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first for layer caching
COPY Cargo.toml Cargo.lock ./

# Copy source and build artifacts
COPY src/ src/
COPY migrations/ migrations/
COPY wit/ wit/
COPY benchmarks/ benchmarks/

RUN cargo build --release --bin enclagent

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/enclagent /usr/local/bin/enclagent
COPY --from=builder /app/migrations /app/migrations

# Non-root user
RUN useradd -m -u 1000 -s /bin/bash enclagent
USER enclagent

EXPOSE 3000

ENV RUST_LOG=enclagent=info

ENTRYPOINT ["enclagent"]
