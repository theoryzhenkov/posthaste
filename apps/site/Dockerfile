FROM oven/bun:1.3.11-alpine AS build

WORKDIR /app

# All workspace members' manifests are needed for a frozen-lockfile install.
# The site itself depends on no other workspace package, but the root
# package.json declares the full workspace set, so every member's manifest
# must be present for `bun install --frozen-lockfile` to resolve.
COPY package.json bun.lock ./
COPY apps/site/package.json apps/site/package.json
COPY apps/client/frontend/package.json apps/client/frontend/package.json
COPY apps/client/protocol/package.json apps/client/protocol/package.json
COPY apps/tools/package.json apps/tools/package.json
RUN bun install --frozen-lockfile

COPY apps/site apps/site
# The Starlight docs collection reads the canonical, @spec-referenced technical
# specs in place from the repo-root `docs/` tree (content.config.ts glob, base
# `../../`) — so the image must include them, not just apps/site. eph/issues are
# excluded from the published collection by the glob; they only ride along in
# this discarded build layer (the final nginx image ships `dist/` only).
COPY docs docs
WORKDIR /app/apps/site
RUN bun run build

FROM nginx:1.29-alpine

COPY apps/site/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/apps/site/dist /usr/share/nginx/html

EXPOSE 80

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1/ >/dev/null || exit 1
