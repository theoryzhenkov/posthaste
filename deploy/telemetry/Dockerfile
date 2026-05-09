# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps/desktop ./apps/desktop
COPY vendor ./vendor

RUN cargo build --release -p posthaste-telemetry-ingest

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /nonexistent --shell /usr/sbin/nologin posthaste

COPY --from=builder /src/target/release/posthaste-telemetry-ingest /usr/local/bin/posthaste-telemetry-ingest

RUN mkdir -p /data && chown posthaste:posthaste /data
USER posthaste

ENV POSTHASTE_TELEMETRY_BIND=0.0.0.0:8080 \
    POSTHASTE_TELEMETRY_DATABASE=/data/telemetry.sqlite3

VOLUME ["/data"]
EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/posthaste-telemetry-ingest"]
