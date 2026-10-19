# Nue web

Next.js UI and API routes for Nue: upload (`/api/upload`), job status (`/api/jobs/:id`) and output delivery with HTTP Range support (`/api/output/:file`). Rendering is done by the Rust worker in `../muscle`, which watches the shared `data/` directory.

```bash
npm ci
npm run dev          # http://localhost:3000, uses ../data in development
```

In another terminal run the worker against the same data directory:

```bash
cd ../muscle && NUE_DATA_DIR=../data cargo run -- worker
```

`NUE_DATA_DIR` overrides the data directory for both processes.
