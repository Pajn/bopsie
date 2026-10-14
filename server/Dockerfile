# The server, with ffmpeg and cliairplay, as one image.
#
#   docker build -t bopsie-server server
#   docker run --network host -v bopsie-data:/data -v /path/to/music:/music:ro bopsie-server
#
# AirPlay and mDNS need the host's network. The image runs as an unprivileged user; cliairplay
# carries CAP_NET_BIND_SERVICE so that its PTP daemon can bind UDP ports 319 and 320, which
# needs the container to keep that capability (Docker and Podman grant it by default).

FROM docker.io/library/rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p bopsie-server

FROM docker.io/library/debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl ffmpeg libcap2-bin \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/fetch-cliairplay.sh /tmp/fetch-cliairplay.sh
RUN /tmp/fetch-cliairplay.sh /usr/local/bin/cliairplay \
    && setcap cap_net_bind_service=+ep /usr/local/bin/cliairplay \
    && rm /tmp/fetch-cliairplay.sh \
    && apt-get purge -y curl libcap2-bin \
    && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --home-dir /data bopsie \
    && mkdir -p /data /music /etc/bopsie \
    && chown bopsie:bopsie /data
COPY --from=build /src/target/release/bopsie-server /usr/local/bin/bopsie-server
COPY packaging/container.toml /etc/bopsie/config.toml
USER bopsie
VOLUME /data
EXPOSE 8095 8927
ENTRYPOINT ["bopsie-server", "--config", "/etc/bopsie/config.toml"]
