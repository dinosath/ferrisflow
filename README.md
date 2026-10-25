# Ferris Workflow

Ferris Workflow is a Rust/axum service for storing and editing workflows that
use the Open Workflow Specification (OWS) / Open Workflow DSL.

## Run locally

Start the Dioxus fullstack server with the default local SQLite database:

```sh
mise run dev
```

Open `http://localhost:8080`. Dioxus serves the web UI and the existing REST API
from the same Axum server. The database file is `ferris-workflow.db` in the project root.

The UI is written in Dioxus. Its page components live in `src/pages/`, and the
app configuration and assets live in `Dioxus.toml` and `assets/`. The UI uses
the plain `/api/workflows` REST endpoints. Set `DATABASE_URL` to use a different
database, including PostgreSQL.

```sh
dx serve --fullstack
```

## API

The service exposes the existing CRUD contract:

```text
GET    /api/workflows
POST   /api/workflows
GET    /api/workflows/:id
PUT    /api/workflows/:id
DELETE /api/workflows/:id
```

The same operations are available over ConnectRPC at
`/entities.v1.EntityService/Execute` using `entity: "Workflow"`. ConnectRPC
dispatches in-process to these handlers, so OWS validation and persistence are
identical across both protocols.

`definition_json` must contain a canonical OWS document. The service accepts
JSON (and YAML input for normalization), parses it with the official
`serverless_workflow_core` model, and stores normalized JSON. A non-persisting
validation endpoint is also available:

```text
POST /api/workflows/validate
{ "definition_json": "{ ... }" }
```

An OWS definition has `document` metadata and an ordered `do` task list. For
example:

```json
{
  "document": {
    "dsl": "1.0.3",
    "namespace": "default",
    "name": "hello",
    "version": "1.0.0"
  },
  "do": [
    { "greet": { "set": { "message": "hello" } } }
  ]
}
```

The editor currently supports adding and editing common OWS task forms such
as `set`, `call`, `wait`, `emit`, `switch`, `for`, `do`, and `try`, while the
JSON view remains available for the full specification surface.

## Verification

```sh
cargo check --offline
cargo test --offline --lib ows
mise run dev
```
