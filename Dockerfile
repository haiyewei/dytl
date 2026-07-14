# syntax=docker/dockerfile:1.7

ARG ALPINE_VERSION=3.22
ARG AMAGI_VERSION=v0.1.6

FROM rust:1-alpine AS builder

WORKDIR /src
RUN apk add --no-cache musl-dev

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked

FROM alpine:${ALPINE_VERSION} AS amagi

ARG AMAGI_VERSION
ARG TARGETARCH

RUN apk add --no-cache ca-certificates curl tar

WORKDIR /tmp
RUN set -eux; \
    build_arch="${TARGETARCH:-$(uname -m)}"; \
    case "${build_arch}" in \
      amd64|x86_64) \
        amagi_target="x86_64-unknown-linux-musl"; \
        amagi_sha256="3cd109212be2fc3c5fc22fccb19c000fd7595d9e81760245c66176ca3ac1d2ca"; \
        ;; \
      arm64|aarch64) \
        amagi_target="aarch64-unknown-linux-musl"; \
        amagi_sha256="26acd6027529b11fecdbceffc389fca3fc1ed9b0d620fefcccb221043c515141"; \
        ;; \
      *) \
        echo "Unsupported target architecture: ${build_arch}" >&2; \
        exit 1; \
        ;; \
    esac; \
    curl -fsSL \
      "https://github.com/bandange/amagi-rs/releases/download/${AMAGI_VERSION}/amagi-${amagi_target}.tar.gz" \
      -o amagi.tar.gz; \
    echo "${amagi_sha256}  amagi.tar.gz" | sha256sum -c -; \
    tar -xzf amagi.tar.gz; \
    install -D -m 0755 amagi /out/amagi

FROM alpine:${ALPINE_VERSION}

ARG APP_UID=1000
ARG APP_GID=1000

RUN apk add --no-cache ca-certificates ffmpeg tzdata \
    && addgroup -g "${APP_GID}" dytl \
    && adduser -D -u "${APP_UID}" -G dytl -h /app dytl

COPY --from=builder /src/target/release/dytl /usr/local/bin/dytl
COPY --from=amagi /out/amagi /usr/local/bin/amagi

WORKDIR /app
RUN mkdir -p /app/content && chown -R dytl:dytl /app

USER dytl
VOLUME ["/app/content"]
STOPSIGNAL SIGINT

ENTRYPOINT ["dytl"]
CMD ["--config", "/app/config.yaml", "monitor"]
