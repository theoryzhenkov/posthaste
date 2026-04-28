# PostHaste Site

Static public showcase for PostHaste.

The site is built with Astro. The home page keeps the interactive mail mock as a
React island, while editable home page copy lives in Markdown under
`src/content/home/`.

## Commands

```sh
bun install
bun --cwd=apps/site run dev
bun --cwd=apps/site run build
bun --cwd=apps/site run check
```

Edit home page text in:

```sh
src/content/home/
```

## Docker

```sh
docker build -f apps/site/Dockerfile -t posthaste-site .
docker run --rm -p 8080:80 posthaste-site
```
