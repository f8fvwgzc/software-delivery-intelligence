# One image recipe for every Rust service. `--build-arg SERVICE=<name>` picks
# the binary; the dependency layers are identical for all four, so they are
# compiled once and shared.
#
#   planner  -> cargo-chef recipe (what to compile, without the sources)
#   cacher   -> compiles only dependencies, cached until Cargo.lock changes
#   builder  -> compiles the one service binary on top of that cache
#   runtime  -> debian slim, non-root, binary + healthcheck only

ARG RUST_VERSION=1.99
ARG DEBIAN_RELEASE=bookworm

# ---------------------------------------------------------------- chef
FROM rust:${RUST_VERSION}-slim-${DEBIAN_RELEASE} AS chef
# protoc: crates/proto compiles the .proto files in its build script.
# libprotobuf-dev carries the well-known types (google/protobuf/*.proto)
# that the contracts import; Debian ships them separately from protoc.
RUN apt-get update \
 && apt-get install -y --no-install-recommends protobuf-compiler libprotobuf-dev \
 && rm -rf /var/lib/apt/lists/*
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo install cargo-chef --locked --version ^0.1
WORKDIR /app

# ---------------------------------------------------------------- planner
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---------------------------------------------------------------- cacher
FROM chef AS cacher
COPY --from=planner /app/recipe.json recipe.json
# No `-p` filter on purpose: cooking the whole workspace keeps this layer
# byte-identical across SERVICE values, so it is built once.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json

# ---------------------------------------------------------------- builder
FROM chef AS builder
ARG SERVICE
RUN test -n "${SERVICE}" || (echo "SERVICE build arg is required" && exit 1)
COPY --from=cacher /app/target target
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo build --release --locked -p sdi-${SERVICE} \
 && install -m 0755 target/release/sdi-${SERVICE} /usr/local/bin/sdi-${SERVICE}

# ---------------------------------------------------------------- runtime
FROM debian:${DEBIAN_RELEASE}-slim AS runtime
ARG SERVICE
# ca-certificates: knowledge talks to api.github.com and intelligence to the
# Anthropic API over TLS (rustls, no OpenSSL needed). bash is already in slim
# and gives us /dev/tcp for the healthcheck.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 10001 sdi \
 && useradd --system --uid 10001 --gid sdi --no-create-home --shell /usr/sbin/nologin sdi

COPY --from=builder /usr/local/bin/sdi-${SERVICE} /usr/local/bin/sdi-${SERVICE}
COPY --chmod=0755 deploy/docker/healthcheck.sh /usr/local/bin/healthcheck
# Fixed entrypoint name so ENTRYPOINT can stay in exec form (no ARG expansion).
RUN ln -s /usr/local/bin/sdi-${SERVICE} /usr/local/bin/sdi-service

ENV SDI_SERVICE=${SERVICE} \
    LOG_FORMAT=json \
    RUST_LOG=info,sqlx=warn

# Numeric so orchestrators can verify runAsNonRoot without the image's passwd.
USER 10001:10001
# gateway 8080, knowledge 50051, intelligence 50052, execution 50053
EXPOSE 8080 50051 50052 50053
HEALTHCHECK --interval=10s --timeout=3s --start-period=20s --retries=3 \
  CMD ["healthcheck"]
STOPSIGNAL SIGTERM
ENTRYPOINT ["sdi-service"]
