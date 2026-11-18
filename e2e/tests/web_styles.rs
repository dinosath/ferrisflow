use std::time::Duration;

use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

async fn start_image(image_env: &str, default_image: &str) -> (ContainerAsync<GenericImage>, String) {
    let image_ref = std::env::var(image_env).unwrap_or_else(|_| default_image.to_owned());
    let (image_name, tag) = image_ref.rsplit_once(':').unwrap_or((image_ref.as_str(), "latest"));
    // The frontend normally resolves this Compose service at startup; point it
    // at loopback when testing the static client image by itself.
    let container = GenericImage::new(image_name, tag)
        .with_exposed_port(8080.tcp())
        .with_env_var("FERRIS_WORKFLOW_API_URL", "http://127.0.0.1:8080")
        .start()
        .await
        .unwrap_or_else(|error| panic!("failed to start Docker image {image_ref}: {error}"));
    let host = container.get_host().await.expect("get container host");
    let port = container.get_host_port_ipv4(8080).await.expect("get container port");
    (container, format!("http://{host}:{port}/"))
}

async fn fetch_page(base_url: &str) -> String {
    let client = reqwest::Client::new();
    for _ in 0..60 {
        if let Ok(response) = client.get(base_url).send().await {
            if response.status().is_success() {
                return response.text().await.expect("read page HTML");
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("Docker image did not serve {base_url} within 60 seconds");
}

#[tokio::test]
async fn frontend_client_docker_image_applies_styles() {
    let (_container, base_url) =
        start_image("FERRISFLOW_FRONTEND_IMAGE", "ferrisflow-frontend:e2e").await;
    let _ = fetch_page(&base_url).await;

    playwright_rs::install_browsers(Some(&["chromium"]))
        .await
        .expect("install Playwright Chromium");
    let playwright = playwright_rs::Playwright::launch().await.expect("launch Playwright");
    let browser = playwright.chromium().launch().await.expect("launch Chromium");
    let page = browser.new_page().await.expect("create browser page");
    page.goto(&base_url, None).await.expect("open frontend image");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut styles = serde_json::Value::Null;
    while tokio::time::Instant::now() < deadline {
        styles = page
            .evaluate::<(), serde_json::Value>(
                r#"() => {
                    const screen = document.querySelector('.login-screen');
                    if (!screen) return null;
                    const computed = getComputedStyle(screen);
                    return {
                        display: computed.display,
                        padding: computed.padding,
                        background: computed.backgroundColor,
                        stylesheets: document.styleSheets.length,
                    };
                }"#,
                None,
            )
            .await
            .expect("read computed login screen styles");
        if styles["display"] == "grid" && styles["padding"] == "32px" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    assert_eq!(styles["display"], "grid", "login screen CSS was not applied: {styles}");
    assert_eq!(styles["padding"], "32px", "login screen spacing is missing: {styles}");
    assert_ne!(
        styles["background"], "rgba(0, 0, 0, 0)",
        "login screen background is transparent: {styles}"
    );
    assert!(styles["stylesheets"].as_u64().unwrap_or_default() > 0);

    browser.close().await.expect("close Chromium");
}

#[tokio::test]
async fn fullstack_docker_image_serves_stylesheets() {
    let (_container, base_url) =
        start_image("FERRISFLOW_FULLSTACK_IMAGE", "ferrisflow-fullstack:e2e").await;
    let html = fetch_page(&base_url).await;
    assert!(
        !html.contains("This should be replaced by dx"),
        "server HTML contains Dioxus's unresolved stylesheet placeholder"
    );

    let stylesheets = stylesheet_links(&html);
    assert!(
        stylesheets.len() >= 7,
        "expected at least 7 stylesheet links, found {}",
        stylesheets.len()
    );

    let client = reqwest::Client::new();
    let mut has_login_styles = false;
    for stylesheet in stylesheets {
        let url = reqwest::Url::parse(&base_url)
            .expect("parse base URL")
            .join(&stylesheet)
            .expect("resolve stylesheet URL");
        let response = client.get(url).send().await.expect("fetch stylesheet");
        assert!(response.status().is_success(), "stylesheet failed: {stylesheet}");
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(
            content_type.starts_with("text/css"),
            "stylesheet did not return CSS ({content_type}): {stylesheet}"
        );
        let css = response.text().await.expect("read stylesheet");
        assert!(
            !css.to_ascii_lowercase().contains("<html"),
            "stylesheet returned HTML: {stylesheet}"
        );
        has_login_styles |= css.contains(".login-screen");
    }
    assert!(has_login_styles, "downloaded styles are missing the login screen rules");
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
