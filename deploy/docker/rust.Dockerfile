# One image recipe for every Rust service. `--build-arg SERVICE=<name>` picks
# the binary; the dependency layers are identical for all four, so they are
# compiled once and shared.
#
#   planner  -> cargo-chef recipe (what to compile, without the sources)
#   cacher   -> compiles only dependencies, cached until Cargo.lock changes
#   builder  -> compiles the one service binary on top of that cache
#   extras   -> per-service additions (only execution gets the Docker CLI)
#   runtime  -> debian slim, non-root, binary + healthcheck (+ extras)

ARG RUST_VERSION=1.99
ARG DEBIAN_RELEASE=bookworm
# Docker CLI version shipped in the execution image (official `docker:<v>-cli`
# image; the binaries are static Go, so they run on debian unchanged).
ARG DOCKER_CLI_VERSION=29
# Global so the stage selector `FROM extras-${SERVICE}` below can expand it;
# the stages that read it still redeclare it, as Dockerfile scoping requires.
ARG SERVICE

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

# ---------------------------------------------------------------- extras
# What a service needs beyond its binary, chosen by stage name so the COPY in
# `runtime` stays unconditional. Only `execution` gets anything: the Docker
# CLI and buildx, which its `docker` adapter runs against the daemon socket
# the compose `local-exec` profile provides (CONTRACT.md §5.4). buildx is
# included because the daemon's legacy builder is deprecated and cannot build
# Dockerfiles that use BuildKit features such as `RUN --mount`, which the
# generated Dockerfiles and this one do.
FROM docker:${DOCKER_CLI_VERSION}-cli AS docker-cli

FROM debian:${DEBIAN_RELEASE}-slim AS extras-none
RUN mkdir -p /out

FROM extras-none AS extras-gateway
FROM extras-none AS extras-knowledge
FROM extras-none AS extras-intelligence

FROM extras-none AS extras-execution
COPY --from=docker-cli /usr/local/bin/docker /out/usr/local/bin/docker
COPY --from=docker-cli /usr/local/libexec/docker/cli-plugins/docker-buildx \
     /out/usr/local/lib/docker/cli-plugins/docker-buildx

# `COPY --from` cannot expand a variable, but a FROM can.
FROM extras-${SERVICE} AS extras

# ---------------------------------------------------------------- runtime
FROM debian:${DEBIAN_RELEASE}-slim AS runtime
ARG SERVICE
# ca-certificates: knowledge talks to api.github.com and intelligence to the
# Anthropic API over TLS (rustls, no OpenSSL needed). bash is already in slim
# and gives us /dev/tcp for the healthcheck.
# /var/lib/sdi/run is SDI_RUN_DIR in compose (the docker adapter's checkouts
# and build contexts); created here, owned by the service user, so the named
# volume mounted over it inherits that ownership on first use.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 10001 sdi \
 && useradd --system --uid 10001 --gid sdi --no-create-home --shell /usr/sbin/nologin sdi \
 && mkdir -p /var/lib/sdi/run \
 && chown -R 10001:10001 /var/lib/sdi

COPY --from=extras /out/ /
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
