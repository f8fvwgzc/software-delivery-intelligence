# Developer entry points. `make dev` runs everything on the host against the
# Docker Postgres; `make up` runs the whole stack in Docker behind nginx.

.PHONY: doctor dev deps-up deps-down services web up up-local-exec down build check test fmt lint api-contract e2e loadcheck deploy-check clean

# `make up UP_FLAGS=` starts the stack from images already in the local
# store (CI builds them first, from the shared layer cache).
UP_FLAGS ?= --build

# Is this machine ready? Checks tools, GitHub access, the model settings,
# disk and ports, and prints the command that fixes each problem.
doctor:
	./scripts/doctor.sh

deps-up:
	docker compose -f docker-compose.dev.yml up -d --wait

deps-down:
	docker compose -f docker-compose.dev.yml down

dev: deps-up
	./scripts/dev.sh

services: deps-up
	./scripts/dev.sh --no-web

web:
	cd web && pnpm install --frozen-lockfile && pnpm dev

up:
	docker compose up $(UP_FLAGS) -d --wait
	@echo "open http://localhost"

# Same stack plus the docker-socket relay: the execution service can then run
# the `docker` adapter against this host's daemon. Root-equivalent on this
# machine; read docs/adapters.md before using it.
up-local-exec:
	docker compose --profile local-exec up $(UP_FLAGS) -d --wait
	@echo "open http://localhost   (docker adapter enabled)"

# The profile flag makes `down` also remove the relay when it was started.
down:
	docker compose --profile local-exec down

build:
	cargo build --workspace --release
	cd web && pnpm install --frozen-lockfile && pnpm build

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cd services/gitops && GOTOOLCHAIN=auto go vet ./...
	cd web && pnpm typecheck && pnpm lint

test:
	cargo test --workspace
	cd services/gitops && GOTOOLCHAIN=auto go test ./... -count=1

fmt:
	cargo fmt --all

api-contract:
	./scripts/api-contract.sh

# Playwright against a running stack (`make up` first). SDI_E2E_GITHUB=0
# skips the specs that need a real GitHub account.
e2e:
	cd web && pnpm install --frozen-lockfile && pnpm e2e

# 60 s through nginx at 200 clients with 50 SSE streams held: p50/p95/p99,
# error share and service RSS before/after (scripts/loadcheck.sh; results
# in docs/performance.md).
loadcheck:
	./scripts/loadcheck.sh

# Static validation of everything under deploy/ and the compose file, both
# with and without the local-exec profile; the same checks CI runs.
deploy-check:
	docker compose config --quiet
	docker compose --profile local-exec config --quiet
	docker run --rm -v "$(CURDIR)/deploy/nginx:/etc/nginx:ro" nginxinc/nginx-unprivileged:1.28-alpine nginx -t
	bash -n scripts/*.sh deploy/docker/healthcheck.sh
	cp -n deploy/k8s/secrets.example.env deploy/k8s/secrets.env 2>/dev/null || true
	kubectl kustomize deploy/k8s > /dev/null

clean:
	cargo clean
	rm -rf web/dist web/node_modules

# Go stubs of the gitops service (CONTRACT.md §24.1), committed under
# services/gitops/gen: common, identity, knowledge and gitops. Needs protoc
# with its include directory plus the two Go plugins:
#   go install google.golang.org/protobuf/cmd/protoc-gen-go@v1.36.12
#   go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@v1.6.2
GO_PROTOS := proto/sdi/common/v1/common.proto proto/sdi/identity/v1/identity.proto \
	proto/sdi/knowledge/v1/knowledge.proto proto/sdi/gitops/v1/gitops.proto
GO_MODULE := github.com/f8fvwgzc/software-delivery-intelligence/services/gitops
.PHONY: gen-go
gen-go:
	rm -rf services/gitops/gen
	PATH="$$(go env GOPATH)/bin:$$PATH" protoc -I proto \
		--go_out=services/gitops --go_opt=module=$(GO_MODULE) \
		--go-grpc_out=services/gitops --go-grpc_opt=module=$(GO_MODULE) \
		$(GO_PROTOS)
