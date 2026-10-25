# The one Go service, services/gitops (CONTRACT.md §24): Argo CD's
# gitops-engine embedded behind gRPC on :50055. Build from the repository
# root:
#
#   docker build -f deploy/docker/go.Dockerfile -t sdi-gitops .
#
#   builder  -> golang, module download cached until go.sum changes, then a
#               static binary (no cgo, trimmed paths)
#   runtime  -> distroless static, non-root; the binary is its own
#               healthcheck (`sdi-gitops healthcheck` asks the gRPC health
#               endpoint), since the image has no shell

ARG GO_VERSION=1.26.10

# ---------------------------------------------------------------- builder
FROM golang:${GO_VERSION}-bookworm AS builder
WORKDIR /src/services/gitops
ENV CGO_ENABLED=0 GOTOOLCHAIN=local GOFLAGS=-trimpath
COPY services/gitops/go.mod services/gitops/go.sum ./
RUN --mount=type=cache,target=/go/pkg/mod,sharing=locked \
    go mod download
COPY services/gitops/ ./
RUN --mount=type=cache,target=/go/pkg/mod,sharing=locked \
    --mount=type=cache,target=/root/.cache/go-build,sharing=locked \
    go build -ldflags="-s -w" -o /out/sdi-gitops ./cmd/sdi-gitops

# ---------------------------------------------------------------- runtime
# distroless/static carries CA certificates (GitHub, API servers), /tmp for
# checkouts and nothing else. The engine's short-lived kubeconfig and
# manifest files go to /dev/shm, which container runtimes mount as tmpfs.
FROM gcr.io/distroless/static-debian12:nonroot AS runtime
COPY --from=builder /out/sdi-gitops /usr/local/bin/sdi-gitops
COPY NOTICE LICENSE /usr/share/doc/sdi-gitops/
USER nonroot:nonroot
ENV GITOPS_LISTEN=0.0.0.0:50055 \
    LOG_FORMAT=json
EXPOSE 50055
HEALTHCHECK --interval=10s --timeout=5s --start-period=20s --retries=6 \
    CMD ["/usr/local/bin/sdi-gitops", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/sdi-gitops"]
