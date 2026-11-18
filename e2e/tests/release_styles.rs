use std::fs;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct RunningApp(Child);

impl Drop for RunningApp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn release_cli_archive_serves_stylesheets() {
    let archive = std::env::var("FERRISFLOW_RELEASE_ARCHIVE")
        .expect("FERRISFLOW_RELEASE_ARCHIVE must point to the cargo-dist archive");
    let unpacked = tempfile::tempdir().expect("create release extraction directory");
    let status = Command::new("tar")
        .args(["-xJf", &archive, "-C"])
        .arg(unpacked.path())
        .status()
        .expect("run tar to extract the release archive");
    assert!(status.success(), "failed to extract release archive {archive}");

    let release_dir = fs::read_dir(unpacked.path())
        .expect("read extracted release")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("ferrisflow").is_file() && path.join("public").is_dir())
        .expect("release archive must contain the ferrisflow binary and public assets");
    let database = unpacked.path().join("test.db");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("reserve local port")
        .local_addr()
        .expect("read local port")
        .port();
    let base_url = format!("http://127.0.0.1:{port}/");
    let app = Command::new(release_dir.join("ferrisflow"))
        .current_dir(&release_dir)
        .env("IP", "127.0.0.1")
        .env("PORT", port.to_string())
        .env("DATABASE_URL", format!("sqlite://{}?mode=rwc", database.display()))
        .env_remove("CARGO_MANIFEST_DIR")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the release CLI binary");
    let _app = RunningApp(app);

    let client = reqwest::Client::new();
    let mut html = None;
    for _ in 0..60 {
        if let Ok(response) = client.get(&base_url).send().await {
            if response.status().is_success() {
                html = Some(response.text().await.expect("read release page HTML"));
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let html =
        html.unwrap_or_else(|| panic!("release CLI did not serve {base_url} within 60 seconds"));
    assert!(
        !html.contains("This should be replaced by dx"),
        "release CLI HTML contains Dioxus's unresolved stylesheet placeholder"
    );

    let stylesheets = stylesheet_links(&html);
    assert!(
        stylesheets.len() >= 7,
        "expected at least 7 release stylesheet links, found {}",
        stylesheets.len()
    );
    let mut has_login_styles = false;
    for stylesheet in stylesheets {
        let url = reqwest::Url::parse(&base_url)
            .expect("parse base URL")
            .join(&stylesheet)
            .expect("resolve release stylesheet URL");
        let response = client.get(url).send().await.expect("fetch release stylesheet");
        assert!(response.status().is_success(), "release stylesheet failed: {stylesheet}");
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(
            content_type.starts_with("text/css"),
            "release stylesheet did not return CSS ({content_type}): {stylesheet}"
        );
        has_login_styles |=
            response.text().await.expect("read release stylesheet").contains(".login-screen");
    }
    assert!(has_login_styles, "release archive is missing the login screen CSS rules");
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
