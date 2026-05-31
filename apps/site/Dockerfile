FROM oven/bun:1.3.11-alpine AS build

WORKDIR /app

# All workspace members' manifests are needed for a frozen-lockfile install.
COPY package.json bun.lock ./
COPY apps/web/package.json apps/web/package.json
COPY apps/site/package.json apps/site/package.json
COPY apps/mcp/package.json apps/mcp/package.json
RUN bun install --frozen-lockfile

COPY apps/site apps/site
WORKDIR /app/apps/site
RUN bun run build

FROM nginx:1.29-alpine

COPY apps/site/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/apps/site/dist /usr/share/nginx/html

EXPOSE 80

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1/ >/dev/null || exit 1
