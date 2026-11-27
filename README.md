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

### OAuth 2.0 API authentication

The API can validate OAuth 2.0 bearer access tokens with an RFC 7662 token
introspection endpoint. API authentication is required by default. Configure
`OAUTH_INTROSPECTION_URL`, `OAUTH_CLIENT_ID`, and
`OAUTH_CLIENT_SECRET`. The introspection client authenticates with HTTP Basic
and sends each access token in the request body. Remote introspection URLs must
use HTTPS; HTTP is accepted only for loopback development. Redirects are not
followed. `/api/health` remains public.

When authentication is enabled, requests without an active bearer token receive
`401`; if the identity provider cannot be reached, requests receive `503`.
Authenticated requests carry the provider's `sub`, optional `username`, and
space-delimited `scope` values as an authorization principal. The
`GET /api/auth/me` endpoint returns that principal. When authentication is
required, GET-like requests require `ferrisflow:read`, while POST, PUT, PATCH,
and DELETE require `ferrisflow:write`. Set `OAUTH_READ_SCOPE` or
`OAUTH_WRITE_SCOPE` to match the names issued by your provider. Set
`AUTH_REQUIRED=false` only for trusted local development or isolated tests;
user administration remains admin-only in every mode.

### OIDC browser sign-in

Set `OIDC_ENABLED=true` to show the single sign-on button and enable the
authorization-code callback. Ferrisflow uses the WebGates Axum OAuth2 gate for
authorization code, PKCE, state cookies, and signed session cookies. It requests
the `openid`, `profile`, and `email` scopes. Ferrisflow validates the returned ID
token's signature, issuer, audience, expiry, and per-login nonce against the
provider's JWKS, then introspects the access token and requires its subject to
match before mapping Ferrisflow scopes into a local session. Set
`OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, and the exact callback URI in
`OIDC_REDIRECT_URI` (default `http://localhost:8080/auth/oidc/callback`). The
authorization and token endpoints default to the standard
`{OIDC_ISSUER}/protocol/openid-connect/{auth,token}` paths; set `OIDC_AUTH_URL`
and `OIDC_TOKEN_URL` for providers that use different paths. The signing-key
endpoint defaults to `{OIDC_ISSUER}/protocol/openid-connect/jwks.json`; set
`OIDC_JWKS_URL` for providers that use a different JWKS path. API requests verify
the signed local session cookie without contacting the identity provider.
`OIDC_ENABLED` defaults to false. The existing password login remains available.

WebGates requires a Ferrisflow session signing key pair when OIDC is enabled.
Generate one for local development and keep both files private:

```sh
openssl ecparam -name secp384r1 -genkey -noout -out oidc-session-private.pem
openssl pkey -in oidc-session-private.pem -pubout -out oidc-session-public.pem
```

The generated key files are ignored by Git. Use persistent keys in deployments
so sessions remain verifiable across restarts and app instances.

For a local FerrisKey provider, copy `.env.example` to `.env`, set the client
secret after creating a client, then start FerrisKey and its PostgreSQL database:

```sh
docker compose --profile oidc up -d postgres ferriskey-postgres ferriskey-migrate ferriskey ferriskey-webapp
```

Podman users can run the same profile with `podman compose`. Open
`http://localhost:5555` (admin/admin), create a `ferrisflow` realm, and create
an enabled confidential OpenID Connect client with PKCE required and redirect
URI `http://localhost:8080/auth/oidc/callback`. Assign the `openid`, `profile`,
and `email` scopes, and create the Ferrisflow API scopes used by this service
(`ferrisflow:read`, `ferrisflow:write`, and `ferrisflow:admin`) if the API is to
enforce those permissions. Copy the client secret into both OIDC and OAuth
client-secret variables in `.env` and set `OIDC_ENABLED=true`. Run the
Ferrisflow web app on the host with the `.env` values exported:

```sh
set -a
. ./.env
set +a
dx serve --fullstack --features fullstack
```

The provider issuer is `http://localhost:3333/realms/ferrisflow`; its introspection URL is
`http://localhost:3333/realms/ferrisflow/protocol/openid-connect/token/introspect`.
The default compose profile is unchanged and does not start FerrisKey.

### User administration

The service includes admin-only user CRUD endpoints: `GET` and `POST
/api/users`, plus `GET`, `PUT`, and `DELETE /api/users/:id`. OAuth access tokens
must include `ferrisflow:admin` (or the configured `OAUTH_ADMIN_SCOPE`). The
local admin can use HTTP Basic authentication with username `admin` and the
generated password. On first startup, Ferrisflow creates a 256-bit random
password, saves an Argon2 verifier in the `app_settings` config table, and
prints the password once as a warning. Legacy SHA-256 verifiers are upgraded
to Argon2 after a successful login. Set `ADMIN_PASSWORD` to configure or
rotate it explicitly; use a long random value for this override. The browser UI
prompts for a username and password. Admin-created users can sign in when a
password of at least 12 characters is supplied in the user create or update
request; passwords are stored as Argon2 hashes. Existing profile-only users can
be given a password through `PUT /api/users/:id`. User objects have `username`,
optional `email`, and `display_name` fields. Use HTTPS when sending credentials
over a network.

### Plugin-defined content types

Content type screens, REST routes, and CRUD workflow definitions are declared
by the built-in Content types plugin in
`plugins/ferrisflow.content-types.json`. Its OWS workflows expose the schema
registry at `/api/content-type-registry` and records at `/api/content/{uid}`.
The plugin view lists its declared workflows and opens the standard workflow
runner; its UI and API declarations do not require a dedicated page or route
controller.

### Plugins and workspace primitives

Plugins are declarative packages. A manifest can include database content
types, workflow definitions, menu items, saved views, and references to other
plugins. Plugin files are JSON; resource arrays can include `{"$file":"..."}`
entries that point into the package's `files` map. File references are relative
to the package and cannot traverse parent folders. URL dependencies are fetched
over HTTPS, checked against private/reserved network addresses, and composed
into the parent package.

Every inline or expanded plugin workflow is validated as an OWS workflow when
the package is created, updated, or imported. The built-in Content types plugin
ships real OWS CRUD workflows; their `call` tasks execute database operations
through Ferrisflow's content database functions.

The same package can be stored as one manifest:

```json
{
  "schemaVersion": 1,
  "id": "acme.inventory",
  "name": "Inventory",
  "version": "1.0.0",
  "plugins": [{"file": "plugins/audit.json"}],
  "database": {"contentTypes": [{"$file": "database/products.json"}]},
  "workflows": [{"$file": "workflows/reorder.json"}],
  "menuItems": [{"label": "Products", "path": "/content-types", "icon": "table"}],
  "views": [{"$file": "views/products.json"}]
}
```

Or as a bundle with a manifest file and resource files:

```json
{
  "entry": "plugin.json",
  "files": {
    "plugin.json": "{\"id\":\"acme.inventory\",\"name\":\"Inventory\",\"version\":\"1.0.0\",\"database\":{\"contentTypes\":[{\"$file\":\"database/products.json\"}]},\"workflows\":[],\"menuItems\":[],\"views\":[]}",
    "database/products.json": "[{\"uid\":\"products\",\"display_name\":\"Products\",\"schema\":{\"fields\":[{\"name\":\"name\",\"label\":\"Name\",\"type\":\"text\",\"required\":true}]}}]"
  }
}
```

Manage packages through **Plugins** or the REST API:

```text
GET/POST       /api/plugins
GET/PUT/DELETE /api/plugins/:id
PUT            /api/plugins/:id/enabled
POST           /api/plugins/import      {"url":"https://…/plugin.json"}
GET            /api/plugins/menu-items
```

Enabled plugin menu items open the view declared by that plugin. Plugin
packages are stored as declarative bundles; their content types, workflows,
and views remain available in the package manifest for inspection and
composition.

### Duplicate workflows

Use the copy button beside a workflow in the workflow list to create a
`Copy of …` workflow from its definition. The copy starts inactive, so any
scheduled or database triggers must be reviewed before enabling it.

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
ghcr.io/dinosath/ferrisflow           # API server only
ghcr.io/dinosath/ferrisflow-frontend  # Static web app with /api proxy
ghcr.io/dinosath/ferrisflow-fullstack # Web app and API in one server
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
The archive contains the `ferrisflow` server executable and the Dioxus
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
docker build --target server -t ferrisflow .
docker build --target frontend -t ferrisflow-frontend .
docker build --target fullstack -t ferrisflow-fullstack .
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
