# Ferris Workflow

Ferris Workflow is a Rust/axum service for storing and editing workflows that
use the Open Workflow Specification (OWS) / Open Workflow DSL.

## Run locally

Start the Dioxus fullstack server with the web UI, REST API, and default local
SQLite database:

```sh
mise run dev
```

Open `http://localhost:8080`. Dioxus serves the web UI and the existing REST API
from the same Axum server. The database file is `ferris-workflow.db` in the project root.

You can also start it directly:

```sh
dx serve --fullstack --features fullstack
```

The Cargo `server` feature is API-only and deliberately does not enable the UI.
For a standalone API server, run:

```sh
cargo run --no-default-features --features server
```

For a single process serving both the Dioxus UI and REST API, use the explicit
`fullstack` feature:

```sh
cargo run --no-default-features --features fullstack
```

For Android or desktop-native UI development, run the API server separately
with `server`, then launch the UI with the appropriate Dioxus CLI target and
the `mobile` feature.

## Run on Android

Android Studio is not required. Install the Android command-line tools, an SDK
platform, Platform Tools, the NDK (side by side), and CMake with `sdkmanager`.
Install the Rust target and configure the Android tools in your shell. For
example, on macOS:

```sh
rustup target add aarch64-linux-android
export JAVA_HOME="$(/usr/libexec/java_home -v 17)"
export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/30.0.16248370"
export NDK_HOME="$ANDROID_NDK_HOME"
export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
```

Enable **Developer options** and **USB debugging** on the device, connect it,
accept the debugging prompt, and verify that ADB sees it:

```sh
dx doctor
adb devices
```

Start the backend in one terminal:

```sh
mise run backend-dev
```

In another terminal, forward the backend port to the device and serve the mobile
app:

```sh
adb reverse tcp:8080 tcp:8080
dx serve --android --no-default-features --features mobile
```

The port forward lets the Android app reach the backend at
`http://127.0.0.1:8080`. The Android build uses the `mobile` Cargo feature;
`--no-default-features` prevents the default web renderer from being enabled.

The UI is written in Dioxus. Its page components live in `src/pages/`, and the
app configuration and assets live in `Dioxus.toml` and `assets/`. The UI uses
the plain `/api/workflows` REST endpoints. Set `DATABASE_URL` to use a different
database, including PostgreSQL.

Configure the workflow editor's AI chat overlay in **Settings → AI integration**.
Add an OpenAI, OpenRouter, Ollama, or custom OpenAI-compatible provider, then
set its model and API key. Provider records are stored in the configured app
database; API responses expose only whether a key is saved, never the key. The
key is currently stored unencrypted at rest in that database.

## API

The service exposes the existing CRUD contract:

```text
GET    /api/workflows
POST   /api/workflows
GET    /api/workflows/:id
PUT    /api/workflows/:id
DELETE /api/workflows/:id
```

### Expose a workflow as a REST endpoint

Open the workflow's **REST API settings**, enable **Expose via REST API**, and
choose an HTTP method and path under `/api/` (for example, `POST /api/orders`).
Save the workflow and make sure it is active. Requests to the endpoint execute
the workflow synchronously and return a JSON execution result.

The request is passed to the workflow as `request`, with `method`, `path`, the
raw `query` string, `headers`, and parsed JSON `body` fields. For example:

```sh
curl -X POST http://localhost:8080/api/orders \
  -H 'Content-Type: application/json' \
  -d '{"customer_id":42,"items":[{"sku":"A-1","quantity":2}]}'
```

Custom endpoints use the same `/api/` namespace as the service API. Built-in
API paths are reserved. The workflow's OWS metadata stores the exposure under
`document.metadata.rest_api`.

The same operations are available over ConnectRPC at
`/entities.v1.EntityService/Execute` using `entity: "Workflow"`. ConnectRPC
dispatches in-process to these handlers, so OWS validation and persistence are
identical across both protocols.

### Container images

CI builds three images and publishes them to GitHub Container Registry on
`main` and version tags:

```text
ghcr.io/dinosath/ferris-workflow-server    # API server only
ghcr.io/dinosath/ferris-workflow-frontend  # Static web app with /api proxy
ghcr.io/dinosath/ferris-workflow-fullstack # Web app and API in one server
```

The server and fullstack images use `/data` for the default SQLite database;
mount persistent storage there or set `DATABASE_URL` for an external database.
The frontend image proxies `/api/` to `FERRIS_WORKFLOW_API_URL`, which defaults
to `http://server:8080` for a Docker Compose service named `server`.

CI builds all three images for every pull request and branch push. It publishes
them to GitHub Container Registry on `main` and on `v*` version tags. The same
CI run tests and builds the Rust server variants and packages the Helm chart as
a downloadable workflow artifact.

Tagged releases also include a cargo-dist Linux archive and shell installer.
The archive contains the `ferris-workflow` server executable and the Dioxus
frontend files in a sibling `public/` directory; run the executable from the
extracted directory to serve both the UI and API. The release build targets
`x86_64-unknown-linux-gnu`.

The cargo-dist workflow at `.github/workflows/release.yml` is generated from
`dist-workspace.toml`. After changing the distribution configuration, run
`dist generate --mode ci` to update it, then `dist generate --mode ci --check`
to verify it is current. It runs the release plan on pull requests and builds,
hosts, and announces artifacts when a version tag is pushed.

The moving image tags are `edge` for `main`, `stable` and `latest` for stable
version tags, and `beta` for prerelease tags such as `v0.2.0-rc.1`. GitHub
prereleases older than 30 days are removed weekly along with their tags.

Build an individual image locally with BuildKit:

```sh
docker build --target server -t ferris-workflow-server .
docker build --target frontend -t ferris-workflow-frontend .
docker build --target fullstack -t ferris-workflow-fullstack .
```

The server and fullstack targets use Docker Hardened Images when credentials are
available. Authenticate with `docker login dhi.io` before building locally.
Configure the repository secrets `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` for
CI to use DHI; otherwise CI uses the distroless static runtime.

### Releases

Pull request commits must follow Conventional Commits, for example
`feat: add a node editor` or `fix(api): handle empty input`. Commits that start
with `feat`, `fix`, `perf`, `refactor`, or `revert` prepare a version and
changelog update through release-plz. The generated release pull request runs
the same CI checks as other pull requests. After it is merged, release-plz
creates a version tag and GitHub release. Releases are git-only; no Cargo
registry publishing is configured.

To publish a temporary prerelease, set the version in the release PR to include
a SemVer suffix such as `-alpha.1`, `-beta.1`, or `-rc.1`. release-plz marks
that GitHub release as a prerelease. Temporary prereleases are retained for 30
days before scheduled cleanup removes them.

The release workflow needs a repository secret named `RELEASE_PLZ_TOKEN`. It
must be a token with permission to create and update pull requests and push
release tags. A personal access token or GitHub App token is needed so the
release pull request and tags trigger their normal CI workflows. Configure the
`main` branch ruleset to require the `Rust checks`, `Package Helm chart`,
`Build server image`, `Build frontend image`, `Build fullstack image`, and
`Conventional Commits` checks before merging.

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
