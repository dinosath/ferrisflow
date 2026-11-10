
use playwright_rs::{Browser, Playwright};

use rstest::fixture;
use std::path::PathBuf;
use std::sync::Arc;
use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, OnceCell as AsyncOnceCell};

/// Shared test environment that manages container lifecycle
pub struct TestEnvironment {
    /// The Postgres container
    _postgres: ContainerAsync<Postgres>,
    /// The App container
    _app: ContainerAsync<GenericImage>,
    /// Base URL for the REST application under test
    pub base_url: String,


}


/// Playwright browser wrapper for UI tests
pub struct TestBrowser {
    pub browser: Browser,
    _playwright: Playwright,
}

impl TestBrowser {
    pub async fn new() -> Self {
        let _ = playwright_rs::install_browsers().await;
        let playwright = Playwright::launch().await.expect("Failed to launch Playwright");
        let browser = playwright
            .chromium()
            .launch()
            .await
            .expect("Failed to launch browser");
        Self {
            browser,
            _playwright: playwright,
        }
    }
}


/// Global singleton for test environment - uses tokio's AsyncOnceCell for proper async initialization
static TEST_ENV: AsyncOnceCell<Arc<Mutex<TestEnvironment>>> = AsyncOnceCell::const_new();

impl TestEnvironment {
    /// Get or initialize the shared test environment (guaranteed single initialization)
    pub async fn get_or_init() -> Arc<Mutex<TestEnvironment>> {
        TEST_ENV
            .get_or_init(|| async {
                let env = Self::init().await.expect("Failed to initialize test environment");
                Arc::new(Mutex::new(env))
            })
            .await
            .clone()
    }

    async fn init() -> Result<TestEnvironment, Box<dyn std::error::Error + Send + Sync>> {
        // Initialize logging for testcontainers (only once, ignore if already initialized)
        let _ = env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .filter_module("testcontainers", log::LevelFilter::Debug)
            .is_test(true)
            .try_init();

        let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("e2e crate should have parent directory")
            .to_path_buf();

        println!("Project directory: {:?}", project_dir);

        // Start Postgres container
        println!("Starting Postgres container...");
        let postgres = Postgres::default()
            .with_db_name("testdb")
            .with_user("postgres")
            .with_password("postgres");

        let postgres_container = postgres.start().await?;
        
        let pg_host = postgres_container.get_host().await?;
        let pg_port = postgres_container.get_host_port_ipv4(5432).await?;
        let database_url = format!(
            "postgres://postgres:postgres@{}:{}/testdb",
            pg_host, pg_port
        );
        println!("Postgres available at: {}", database_url);

        // For container-to-container communication on Podman (macOS), use host.containers.internal
        let database_url_for_app = format!(
            "postgres://postgres:postgres@host.containers.internal:{}/testdb",
            pg_port
        );
        println!("DATABASE_URL for app: {}", database_url_for_app);

        println!("Starting app container...");
        let app_image = GenericImage::new("ferrisflow", "latest")
            .with_exposed_port(8080.tcp())

            .with_env_var("DATABASE_URL", &database_url_for_app)
            .with_env_var("RUST_LOG", "debug");
        
        let app_container: ContainerAsync<GenericImage> = app_image.start().await?;

        let app_host = app_container.get_host().await?;
        let app_port = app_container.get_host_port_ipv4(8080).await?;
        let base_url = format!("http://{}:{}", app_host, app_port);

        println!("App service available at: {}", base_url);

        // Wait for healthy with log streaming
        Self::wait_for_healthy_with_logs(&base_url, app_container.id()).await?;



        Ok(TestEnvironment {
            _postgres: postgres_container,
            _app: app_container,
            base_url,


        })
    }

    /// Fetch container logs using podman/docker CLI
    async fn fetch_container_logs(container_id: &str) {
        // Try podman first, then docker
        let output = tokio::process::Command::new("podman")
            .args(["logs", "--tail", "50", container_id])
            .output()
            .await
            .or_else(|_| {
                std::process::Command::new("docker")
                    .args(["logs", "--tail", "50", container_id])
                    .output()
            });
        
        match output {
            Ok(output) => {
                if !output.stdout.is_empty() {
                    println!("[STDOUT]\n{}", String::from_utf8_lossy(&output.stdout));
                }
                if !output.stderr.is_empty() {
                    println!("[STDERR]\n{}", String::from_utf8_lossy(&output.stderr));
                }
            }
            Err(e) => println!("Failed to fetch logs: {}", e),
        }
    }

    /// Wait for the application to be healthy while streaming logs
    async fn wait_for_healthy_with_logs(
        base_url: &str,
        container_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = reqwest::Client::new();
        let health_url = format!("{}/api/health", base_url);

        for attempt in 1..=30 {
            // Check health first
            match client.get(&health_url).send().await {
                Ok(response) if response.status().is_success() => {
                    println!("App is healthy after {} attempts", attempt);
                    return Ok(());
                }
                Ok(response) => {
                    println!(
                        "Attempt {}: Health check returned status {}",
                        attempt,
                        response.status()
                    );
                }
                Err(e) => {
                    println!("Attempt {}: Health check failed: {}", attempt, e);
                }
            }
            
            // Print logs every 5 attempts or on final attempt
            if attempt % 5 == 0 || attempt == 30 {
                println!("=== Container logs (attempt {}) ===", attempt);
                Self::fetch_container_logs(container_id).await;
                println!("=== End logs ===");
            }
            
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }

        // Final log dump before failing
        println!("=== Final container logs ===");
        Self::fetch_container_logs(container_id).await;
        println!("=== End final logs ===");

        Err("App failed to become healthy within timeout".into())
    }
}

/// Rstest fixture for the test environment (containers)
#[fixture]
pub async fn test_env() -> Arc<Mutex<TestEnvironment>> {
    TestEnvironment::get_or_init().await
}

/// Rstest fixture for the base URL
#[fixture]
pub async fn base_url(#[future] test_env: Arc<Mutex<TestEnvironment>>) -> String {
    let env = test_env.await;
    let guard = env.lock().await;
    guard.base_url.clone()
}


/// Rstest fixture for Playwright browser (UI tests)
#[fixture]
pub async fn browser() -> TestBrowser {
    TestBrowser::new().await
}




/// Rstest fixture for HTTP client (API tests)
#[fixture]
pub fn http_client() -> reqwest::Client {
    reqwest::Client::new()
}



