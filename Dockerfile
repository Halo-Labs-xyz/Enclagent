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

ARG NODE_VERSION=24.0.2

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash ca-certificates curl libssl3 xz-utils \
    && rm -rf /var/lib/apt/lists/*

RUN arch="$(dpkg --print-architecture)" \
    && case "${arch}" in \
      amd64) node_arch="x64" ;; \
      arm64) node_arch="arm64" ;; \
      *) echo "Unsupported architecture: ${arch}" >&2; exit 1 ;; \
    esac \
    && curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${node_arch}.tar.xz" -o /tmp/node.tar.xz \
    && tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 \
    && rm -f /tmp/node.tar.xz \
    && node --version \
    && npm --version

RUN npm install -g @layr-labs/ecloud-cli@0.3.3

COPY --from=builder /app/target/release/enclagent /usr/local/bin/enclagent
COPY --from=builder /app/migrations /app/migrations
COPY scripts/provision-user-ecloud.sh /app/scripts/provision-user-ecloud.sh
COPY deploy/ecloud-instance.env /app/deploy/ecloud-instance.env

RUN chmod +x /app/scripts/provision-user-ecloud.sh

# Non-root user
RUN useradd -m -u 1000 -s /bin/bash enclagent
USER enclagent

EXPOSE 3000

ENV RUST_LOG=enclagent=info

ENTRYPOINT ["enclagent"]
