# Network-role image: Bootstrap + Index + Relay behind an external proxy
# (Coolify / Traefik / Caddy). Do not run the VPS Caddy script in this
# container — the platform owns TLS.
FROM rust:bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY llms.txt ./
RUN cargo build --release --locked -p stumble-cli

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --home /data --shell /usr/sbin/nologin stumble
COPY --from=build /src/target/release/stumble /usr/local/bin/stumble
COPY --from=build /src/target/release/stumble-api /usr/local/bin/stumble-api
COPY scripts/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod 0755 /usr/local/bin/docker-entrypoint.sh
ENV STUMBLE_DATA_DIR=/data/node \
    STUMBLE_CREDENTIAL_STORE_DIR=/data/credentials
VOLUME /data
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
