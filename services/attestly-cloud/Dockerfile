# Build container for `attestly-cloud`. Multi-stage so the runtime image carries
# only the binary + the static web assets.
#
# Build context is expected to be the *repo root* (because `attestly-cloud` is its
# own workspace that depends on the in-repo `attestly-core` via a relative path).
# `docker-compose.yml` sets `context: ../..` accordingly.

FROM rust:1.95-slim AS builder

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build
# Copy the attestly-core crate (path dep) plus the attestly-cloud service.
# attestly-core inherits edition/deps from a workspace root, so a trimmed root
# manifest (members = attestly-core only) is copied in as /build/Cargo.toml.
COPY services/attestly-cloud/docker/Cargo.workspace.toml /build/Cargo.toml
COPY crates/attestly-core /build/crates/attestly-core
COPY services/attestly-cloud /build/services/attestly-cloud

WORKDIR /build/services/attestly-cloud
RUN cargo build --release --bins

# ---

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/services/attestly-cloud/target/release/attestly-cloud /usr/local/bin/attestly-cloud
COPY --from=builder /build/services/attestly-cloud/target/release/attestly-cloud-seed /usr/local/bin/attestly-cloud-seed
COPY --from=builder /build/services/attestly-cloud/web /app/web

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

CMD ["/usr/local/bin/attestly-cloud"]
