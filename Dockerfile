# syntax=docker/dockerfile:1
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
RUN apt-get update \
 && apt-get install -y --no-install-recommends libatomic1 \
 && rm -rf /var/lib/apt/lists/*
COPY CHANGELOG.md /src/CHANGELOG.md
WORKDIR /src/clients
COPY clients ./
RUN gradle --no-daemon :web:wasmJsBrowserDistribution \
 && mkdir /web \
 && cp -r web/build/dist/wasmJs/productionExecutable/. /web/

FROM node:22-alpine AS editor
RUN corepack enable
WORKDIR /src/editor
COPY editor ./
RUN pnpm install --frozen-lockfile \
 && pnpm build

FROM alpine:${ALPINE_VERSION} AS pdfjs
ARG PDFJS_VERSION=6.4.299
RUN apk add --no-cache curl unzip \
 && curl -fsSL -o /tmp/pdfjs.zip "https://github.com/mozilla/pdf.js/releases/download/v${PDFJS_VERSION}/pdfjs-${PDFJS_VERSION}-dist.zip" \
 && unzip -q /tmp/pdfjs.zip -d /pdfjs \
 && find /pdfjs -name '*.map' -delete \
 && rm -f /pdfjs/web/compressed.tracemonkey-pldi-09.pdf /pdfjs/web/debugger.*

FROM alpine:${ALPINE_VERSION}
RUN apk add --no-cache dumb-init ca-certificates typst \
 && addgroup -g 1000 mycelium && adduser -u 1000 -G mycelium -s /bin/sh -D mycelium \
 && mkdir -p /var/lib/mycelium /usr/share/mycelium/web /usr/share/mycelium/editor /usr/share/mycelium/pdfjs \
 && chown mycelium:mycelium /var/lib/mycelium
COPY --from=binary /mycelium /usr/local/bin/mycelium
COPY --from=web /web /usr/share/mycelium/web
COPY --from=editor /src/editor/dist /usr/share/mycelium/editor
COPY --from=pdfjs /pdfjs /usr/share/mycelium/pdfjs
USER mycelium
ENV MYCELIUM_LISTEN=0.0.0.0:8080 \
    MYCELIUM_DATA_DIR=/var/lib/mycelium \
    MYCELIUM_WEB_DIR=/usr/share/mycelium/web
VOLUME /var/lib/mycelium
EXPOSE 8080
CMD ["dumb-init", "mycelium", "control", "serve"]
