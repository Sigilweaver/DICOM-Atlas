# DICOM-Atlas docs site

[Docusaurus](https://docusaurus.io/) site for DICOM-Atlas, deploying
to [https://sigilweaver.app/dicom-atlas/docs/](https://sigilweaver.app/dicom-atlas/docs/)
via Cloudflare Workers (managed by the Cloudflare GitHub App on
push to `main`).

## Develop

```sh
bun install
bun run dev          # http://localhost:25817/dicom-atlas/docs/
```

## Build (verify locally)

```sh
bun run build:cloudflare
```
