ARG ALPINE_VERSION=3.23

FROM rust:1.96.0-alpine${ALPINE_VERSION} AS binary
ARG MYCELIUM_VERSION=
ENV MYCELIUM_VERSION=${MYCELIUM_VERSION}
RUN apk add --no-cache build-base
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --package mycelium-cli \
 && cp target/release/mycelium /mycelium

FROM gradle:9.8.2-jdk21 AS web
WORKDIR /src/clients
COPY clients ./
RUN gradle --no-daemon :web:wasmJsBrowserDistribution \
 && mkdir /web \
 && cp -r web/build/dist/wasmJs/productionExecutable/. /web/

FROM alpine:${ALPINE_VERSION}
RUN apk add --no-cache dumb-init ca-certificates \
 && addgroup -g 1000 mycelium && adduser -u 1000 -G mycelium -s /bin/sh -D mycelium \
 && mkdir -p /var/lib/mycelium /usr/share/mycelium/web \
 && chown mycelium:mycelium /var/lib/mycelium
COPY --from=binary /mycelium /usr/local/bin/mycelium
COPY --from=web /web /usr/share/mycelium/web
USER mycelium
ENV MYCELIUM_LISTEN=0.0.0.0:8080 \
    MYCELIUM_DATA_DIR=/var/lib/mycelium \
    MYCELIUM_WEB_DIR=/usr/share/mycelium/web
VOLUME /var/lib/mycelium
EXPOSE 8080
CMD ["dumb-init", "mycelium", "control", "serve"]
