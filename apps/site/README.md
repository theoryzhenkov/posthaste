# PostHaste Site

Static public showcase for PostHaste.

The site is built with Astro. The home page keeps the interactive mail mock as a
React island, while editable home page copy lives in Markdown under
`src/content/home/`.

## Commands

```sh
bun install
bun run dev
bun run build
bun run check
```

Edit home page text in:

```sh
src/content/home/
```

## Docker

```sh
docker build -t posthaste-site apps/site
docker run --rm -p 8080:80 posthaste-site
```
