# Examples

## `basic-server`

Axum service with user/auth API routes and an optional Svelte UI baked in at
compile time.

### Prerequisites

- Rust (workspace uses the repo root `Cargo.toml`).
- PostgreSQL reachable via **`DATABASE_URL`** (standard `sqlx` URL, e.g.
  `postgres://USER:PASS@localhost:5432/DBNAME`).
- Optional: **`DB_MAX_CONNECTIONS`** if you want to cap the pool (otherwise
  defaults from the `saps` pool setup).

### Run from the repository root

```bash
cd /path/to/sap
export DATABASE_URL='postgres://…'
```

**API only** (no static site or SPA routes):

```bash
cargo run -p basic-server
```

Listens on **`http://127.0.0.1:3000`** (binds `0.0.0.0:3000`). Use
**`GET /health`** to verify the process.

**API + embedded web UI** (static files from
`examples/basic-server/frontend/web/public` compiled into the binary):

1. Build the frontend once (that directory must exist when Rust compiles, because
   `include_dir!` embeds it):

   ```bash
   cd examples/basic-server/frontend/web
   npm ci
   npm run build
   ```

2. Run the server with the **`embed`** crate feature (enables `include_dir` and
   the `ingress` module):

   ```bash
   cd /path/to/sap
   cargo run -p basic-server --features embed
   ```

After a UI change, run `npm run build` again, then rebuild/re-run the Rust
binary.

**How it fits together:** `examples/basic-server/src/ingress.rs` defines a
`include_dir!` static for `frontend/web/public`, builds a `saps::frontend::Frontend`
from it, and calls `Frontend::attach_to_router(app)` so `GET /` and the
static-or-SPA fallback are added **without** `axum::extract::State`—the combined
router stays `Router<()>`, matching the API stack in `main.rs`.

### Generate the `frontend/` tree (from scratch)

To create `examples/basic-server/frontend/web` the same way this repo does
(Vite + Svelte + Storybook, build output under `frontend/web/public`), run from
the **repository root**:

```bash
./scripts/scaffold_frontend.sh examples/basic-server [app-name]
```

- **`examples/basic-server`** is the service directory relative to the repo root
  (the script creates `that-directory/frontend/web`).
- **`app-name`** is optional; it becomes the `package.json` `name` (default:
  `examples/basic-server-app` if omitted—often you want an explicit name such as
  `basic-app`).

The target `examples/basic-server/frontend/web` must be **empty or missing**; the
script exits if it already has files.

### Frontend development (hot reload)

The Vite app lives under `examples/basic-server/frontend/web`. With the API on
port 3000:

```bash
cd examples/basic-server/frontend/web
npm run dev
```

Vite’s dev server uses its own port (see `vite.config.ts`); point the browser at
that URL for HMR, and keep `cargo run -p basic-server` running for API calls to
`http://localhost:3000`.
