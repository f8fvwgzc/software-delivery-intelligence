# Developer entry points. `make dev` runs everything on the host against the
# Docker Postgres; `make up` runs the whole stack in Docker behind nginx.

.PHONY: dev deps-up deps-down services web up down build check test fmt lint api-contract clean

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
	docker compose up --build -d --wait
	@echo "open http://localhost"

down:
	docker compose down

build:
	cargo build --workspace --release
	cd web && pnpm install --frozen-lockfile && pnpm build

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cd web && pnpm typecheck && pnpm lint

test:
	cargo test --workspace

fmt:
	cargo fmt --all

api-contract:
	./scripts/api-contract.sh

clean:
	cargo clean
	rm -rf web/dist web/node_modules
