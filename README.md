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
dx serve --fullstack
```

To run only the server and REST API (for example, while developing a mobile
client), use a separate terminal:

```sh
mise run backend-dev
# or: cargo run --features server
```

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
