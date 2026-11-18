# E2E Tests (Rust)

End-to-end tests using [playwright-rs](https://github.com/nicholasguan/playwright-rust) with docker-compose orchestration and [nextest setup scripts](https://nexte.st/docs/configuration/setup-scripts/).

## Prerequisites

- Docker with docker-compose (standalone) or docker compose (plugin)
- Rust toolchain
- [cargo-nextest](https://nexte.st/)
- curl and jq (for shell scripts)

## Install cargo-nextest

```bash
cargo install cargo-nextest
```

## Running Tests

```bash
cd e2e

# Run tests (setup script will start containers automatically)
cargo nextest run

# Run with verbose output
cargo nextest run --no-capture
```

The stylesheet E2E tests run against the `ferrisflow-frontend:e2e` and
`ferrisflow-fullstack:e2e` Docker images. Build both images before running them
locally:

```bash
cd ..
docker build --target frontend -t ferrisflow-frontend:e2e .
docker build --target fullstack -t ferrisflow-fullstack:e2e .
cargo test --manifest-path e2e/Cargo.toml --test web_styles
```

The frontend test opens the client image in Chromium and checks computed login
screen styles. The fullstack test fetches the rendered page and verifies its
stylesheet URLs return CSS with the app's login screen rules. CI builds and
loads each image before running its corresponding test.

After tests complete, run teardown manually:

```bash
./scripts/teardown.sh
```

> **Note:** Standard `cargo test` won't work because it doesn't support setup scripts.
> You must use `cargo nextest run`.

## Project Structure

```
e2e/
├── .config/
│   └── nextest.toml      # Nextest configuration with setup scripts
├── scripts/
│   ├── setup.sh          # Setup script (starts containers)
│   └── teardown.sh       # Teardown script (stops containers)
├── src/
│   └── lib.rs            # Test environment and utilities
└── tests/
    └── e2e_tests.rs      # E2E test cases using Playwright
```

## How it works

1. **Setup Script (`scripts/setup.sh`)**: 
   - Executed once before any tests run
   - Detects docker-compose or docker compose
   - Starts containers with `docker-compose up -d --build`
   - Waits for `/api/health` endpoint to be ready
   - Writes `E2E_BASE_URL` to nextest's environment file

2. **Tests**: 
   - Read `E2E_BASE_URL` from environment
   - Run Playwright browser tests against the running app

3. **Teardown Script (`scripts/teardown.sh`)**:
   - Run manually after tests complete
   - Runs `docker-compose down -v --remove-orphans`
   - Cleans up state file

## Configuration

### Environment Variables

- `E2E_BASE_URL` - Set by setup script, used by tests

### Nextest Configuration

See `.config/nextest.toml` for test runner settings:
- `experimental = ["setup-scripts"]` - Enable setup scripts feature
- Setup script: `start-containers` runs before tests
- Sequential test execution (1 thread)
- 120 second test timeout
- 300 second setup timeout

## Testcontainers

The tests use [Testcontainers](https://testcontainers.com/) to manage Docker containers:

1. **Auto port mapping** - Containers use random available ports to avoid conflicts
2. **Health checks** - Waits for containers to be healthy before running tests
3. **Automatic cleanup** - Containers are removed after tests complete
4. **Parallel-safe** - Multiple test runs can execute simultaneously

## Test Reports

After running tests, view the HTML report:

```bash
npm run report
```
