# E2E Tests (Rust)

The stylesheet tests use Testcontainers to start the built application images
and verify that the frontend and fullstack variants serve their compiled CSS.
They work with Docker or a Docker-compatible Podman socket.

## Run stylesheet image tests

Build the images with Podman:

```bash
podman build --target frontend -t ferrisflow-frontend:e2e .
podman build --target fullstack -t ferrisflow-fullstack:e2e .
```

Start Podman's user socket and point Testcontainers to it:

```bash
systemctl --user start podman.socket
export DOCKER_HOST="unix:///run/user/$(id -u)/podman/podman.sock"
```

Run the tests from the repository root:

```bash
cargo test --manifest-path e2e/Cargo.toml --test web_styles
```

The frontend test follows the client JavaScript and WASM asset references,
fetches each compiled stylesheet from the image, and checks for the login
screen rules. The fullstack test fetches the rendered page and verifies its
stylesheet URLs return CSS with those rules. CI builds and loads each image
before running its corresponding test.

The release workflow also runs `release_styles` against the extracted
cargo-dist archive after replacing its raw Cargo binary with the Dioxus CLI
fullstack binary. This checks the executable and the `public/` assets that ship
together in the release package.
