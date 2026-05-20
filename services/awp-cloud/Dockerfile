# Build container for `awp-cloud`. Multi-stage so the runtime image carries
# only the binary + the static web assets.
#
# Build context is expected to be the *repo root* (because `awp-cloud` is its
# own workspace that depends on the in-repo `awp-core` via a relative path).
# `docker-compose.yml` sets `context: ../..` accordingly.

FROM rust:1.95-slim AS builder

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build
# Copy the awp-core crate (path dep) plus the awp-cloud service.
# awp-core inherits edition/deps from a workspace root, so a trimmed root
# manifest (members = awp-core only) is copied in as /build/Cargo.toml.
COPY services/awp-cloud/docker/Cargo.workspace.toml /build/Cargo.toml
COPY crates/awp-core /build/crates/awp-core
COPY services/awp-cloud /build/services/awp-cloud

WORKDIR /build/services/awp-cloud
RUN cargo build --release --bins

# ---

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/services/awp-cloud/target/release/awp-cloud /usr/local/bin/awp-cloud
COPY --from=builder /build/services/awp-cloud/target/release/awp-cloud-seed /usr/local/bin/awp-cloud-seed
COPY --from=builder /build/services/awp-cloud/web /app/web

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

CMD ["/usr/local/bin/awp-cloud"]
