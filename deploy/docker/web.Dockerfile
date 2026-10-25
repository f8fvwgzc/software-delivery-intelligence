# Builds the React app and serves it from an unprivileged nginx that also
# fronts the gateway replicas (deploy/nginx). Build context is the repo root.

ARG NODE_VERSION=22
ARG NGINX_VERSION=1.28
# web/package.json has no `packageManager` field yet, so corepack is told
# which pnpm to activate; keep in sync with the lockfile generator.
ARG PNPM_VERSION=10.18.1

# ---------------------------------------------------------------- build
FROM node:${NODE_VERSION}-alpine AS build
ARG PNPM_VERSION
ENV PNPM_HOME=/pnpm \
    PATH=/pnpm:$PATH \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0 \
    CI=true
RUN corepack enable && corepack prepare "pnpm@${PNPM_VERSION}" --activate
# The repository's layout: the trust page imports TRUST.md and PRIVACY.md
# from the root (web/src/features/auth/pages/trust-doc-page.tsx).
WORKDIR /src/web

# Lockfile first so the install layer survives source edits.
COPY web/package.json web/pnpm-lock.yaml ./
RUN --mount=type=cache,id=pnpm-store,target=/pnpm/store \
    pnpm install --frozen-lockfile

COPY TRUST.md PRIVACY.md /src/
COPY web/ ./
RUN pnpm build

# ---------------------------------------------------------------- runtime
FROM nginxinc/nginx-unprivileged:${NGINX_VERSION}-alpine AS runtime
# The image runs as uid 101 and listens on 8080; nginx.conf keeps its pid and
# temp files under /tmp so the root filesystem can be mounted read-only.
COPY deploy/nginx/ /etc/nginx/
COPY --from=build /src/web/dist /usr/share/nginx/html

EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
  CMD ["wget", "-q", "--spider", "http://127.0.0.1:8080/healthz"]
