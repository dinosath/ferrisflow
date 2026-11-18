use std::error::Error;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[test]
fn client_build_contains_stylesheets() -> Result<()> {
    let public_dir = match std::env::var("FERRISFLOW_WEB_PUBLIC_DIR") {
        Ok(public_dir) => public_dir,
        Err(_) => {
            eprintln!("skipping client stylesheet check without FERRISFLOW_WEB_PUBLIC_DIR");
            return Ok(());
        }
    };
    check_client_styles(Path::new(&public_dir))
}

#[tokio::test]
async fn fullstack_page_serves_stylesheets() -> Result<()> {
    let (server, public_dir) = match (
        std::env::var("FERRISFLOW_WEB_SERVER"),
        std::env::var("FERRISFLOW_WEB_PUBLIC_DIR"),
    ) {
        (Ok(server), Ok(public_dir)) => (server, public_dir),
        _ => {
            eprintln!("skipping fullstack stylesheet check without build artifact paths");
            return Ok(());
        }
    };
    check_fullstack_styles(Path::new(&server), Path::new(&public_dir)).await
}

fn check_client_styles(public_dir: &Path) -> Result<()> {
    let assets_dir = public_dir.join("assets");
    let required = [
        "tailwind",
        "main",
        "workspace",
        "workflow-editor",
        "workflow-editor-canvas",
        "type-scale",
        "workflow-document-menu",
    ];

    for stylesheet in required {
        let prefix = format!("{stylesheet}-");
        let found = fs::read_dir(&assets_dir)?.filter_map(std::result::Result::ok).any(|entry| {
            let path = entry.path();
            path.extension().is_some_and(|extension| extension == "css")
                && path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.starts_with(&prefix))
                && fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
        });
        if !found {
            return Err(format!(
                "compiled {stylesheet} stylesheet missing from {}",
                assets_dir.display()
            )
            .into());
        }
    }

    let workspace_styles =
        fs::read_dir(&assets_dir)?.filter_map(std::result::Result::ok).any(|entry| {
            let path = entry.path();
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("workspace-") && name.ends_with(".css"))
                && fs::read_to_string(path).is_ok_and(|css| css.contains(".login-screen"))
        });
    if !workspace_styles {
        return Err("compiled workspace styles are missing the login screen rules".into());
    }

    Ok(())
}

async fn check_fullstack_styles(server: &Path, public_dir: &Path) -> Result<()> {
    let server = server.canonicalize()?;
    let public_dir = public_dir.canonicalize()?;
    let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
    let temp_db = temp_database_path();
    let mut child = RunningServer::start(&server, &public_dir, &temp_db, port)?;
    let base_url = format!("http://127.0.0.1:{port}/");
    let client = reqwest::Client::new();

    let mut page = None;
    for _ in 0..60 {
        if let Ok(response) = client.get(&base_url).send().await {
            if response.status().is_success() {
                page = Some(response.text().await?);
                break;
            }
        }
        if let Some(status) = child.child.try_wait()? {
            return Err(format!("fullstack server exited before serving HTML: {status}").into());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let page = page.ok_or("fullstack server did not serve HTML within 60 seconds")?;
    if page.contains("This should be replaced by dx") {
        return Err("fullstack HTML contains Dioxus's unresolved stylesheet placeholder".into());
    }

    let stylesheets = stylesheet_links(&page);
    if stylesheets.len() < 7 {
        return Err(
            format!("expected at least 7 stylesheet links, found {}", stylesheets.len()).into()
        );
    }

    let mut has_login_styles = false;
    for stylesheet in stylesheets {
        let url = reqwest::Url::parse(&base_url)?.join(&stylesheet)?;
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(format!(
                "stylesheet request failed for {stylesheet}: {}",
                response.status()
            )
            .into());
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<missing>")
            .to_owned();
        let css = response.text().await?;
        if !content_type.starts_with("text/css") {
            return Err(format!(
                "stylesheet URL did not return text/css (got {content_type}): {stylesheet}; body starts with {:?}",
                css.chars().take(120).collect::<String>()
            )
            .into());
        }
        if css.to_ascii_lowercase().contains("<html") {
            return Err(format!("stylesheet URL returned HTML: {stylesheet}").into());
        }
        has_login_styles |= css.contains(".login-screen");
    }
    if !has_login_styles {
        return Err("fetched stylesheets are missing the login screen rules".into());
    }

    Ok(())
}

fn stylesheet_links(html: &str) -> Vec<String> {
    let mut stylesheets = Vec::new();
    let mut remaining = html;
    while let Some(start) = remaining.find("<link") {
        remaining = &remaining[start..];
        let Some(end) = remaining.find('>') else {
            break;
        };
        let tag = &remaining[..=end];
        if attribute(tag, "rel").as_deref() == Some("stylesheet") {
            if let Some(href) = attribute(tag, "href") {
                if href.contains(".css") && !stylesheets.contains(&href) {
                    stylesheets.push(href);
                }
            }
        }
        remaining = &remaining[end + 1..];
    }
    stylesheets
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_owned())
}

fn temp_database_path() -> PathBuf {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    std::env::temp_dir()
        .join(format!("ferrisflow-style-check-{}-{timestamp}.db", std::process::id()))
}

struct RunningServer {
    child: Child,
    database: PathBuf,
}

impl RunningServer {
    fn start(server: &Path, public_dir: &Path, database: &Path, port: u16) -> Result<Self> {
        let child = Command::new(server)
            .env("IP", "127.0.0.1")
            .env("PORT", port.to_string())
            .env("DATABASE_URL", format!("sqlite://{}?mode=rwc", database.display()))
            .env("DIOXUS_PUBLIC_PATH", public_dir)
            .env_remove("CARGO_MANIFEST_DIR")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self { child, database: database.to_owned() })
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.database);
        let _ = fs::remove_file(format!("{}-shm", self.database.display()));
        let _ = fs::remove_file(format!("{}-wal", self.database.display()));
    }
}
