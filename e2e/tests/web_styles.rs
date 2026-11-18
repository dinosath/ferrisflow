use std::time::Duration;

use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

async fn start_image(
    image_env: &str, default_image: &str,
) -> (ContainerAsync<GenericImage>, String) {
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
async fn frontend_client_docker_image_serves_stylesheets() {
    let (_container, base_url) =
        start_image("FERRISFLOW_FRONTEND_IMAGE", "ferrisflow-frontend:e2e").await;
    let html = fetch_page(&base_url).await;
    let script_path = attribute(&html, "src").expect("client JS path in index HTML");
    let client = reqwest::Client::new();
    let base = reqwest::Url::parse(&base_url).expect("parse base URL");
    let script_url = base.join(&script_path).expect("resolve client JS URL");
    let script = client
        .get(script_url)
        .send()
        .await
        .expect("fetch client JS")
        .text()
        .await
        .expect("read client JS");
    let wasm_path = script_asset_path(&script, "module_or_path:").expect("WASM path in client JS");
    let wasm_url = base.join(&wasm_path).expect("resolve client WASM URL");
    let wasm = client
        .get(wasm_url)
        .send()
        .await
        .expect("fetch client WASM")
        .bytes()
        .await
        .expect("read client WASM");
    let stylesheets = stylesheet_assets(&String::from_utf8_lossy(&wasm));
    assert_eq!(stylesheets.len(), 7, "unexpected client stylesheet manifest: {stylesheets:?}");

    let mut has_login_styles = false;
    for stylesheet in stylesheets {
        let url =
            base.join(&format!("/assets/{stylesheet}")).expect("resolve client stylesheet URL");
        let response = client.get(url).send().await.expect("fetch client stylesheet");
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
        let css = response.text().await.expect("read client stylesheet");
        has_login_styles |= css.contains(".login-screen");
    }
    assert!(has_login_styles, "client image styles are missing the login screen rules");
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

fn stylesheet_assets(wasm: &str) -> Vec<String> {
    let required = [
        "tailwind",
        "main",
        "workspace",
        "workflow-editor",
        "workflow-editor-canvas",
        "type-scale",
        "workflow-document-menu",
    ];
    required
        .into_iter()
        .map(|name| {
            let prefix = format!("{name}-");
            let mut search_from = 0;
            loop {
                let start = wasm[search_from..]
                    .find(&prefix)
                    .map(|offset| search_from + offset)
                    .unwrap_or_else(|| {
                        panic!("client WASM is missing the bundled {name} stylesheet asset")
                    });
                let end = wasm[start..]
                    .find(".css")
                    .map(|offset| start + offset + ".css".len())
                    .expect("stylesheet filename in WASM");
                let asset = &wasm[start..end];
                if asset.contains("-dxh") {
                    break asset.to_owned();
                }
                search_from = start + prefix.len();
            }
        })
        .collect()
}

fn script_asset_path(script: &str, marker: &str) -> Option<String> {
    let start = script.rfind(marker)? + marker.len();
    let value = &script[start..];
    let quote_offset = value.find(['"', '\''])?;
    let quote = value[quote_offset..].chars().next()?;
    let value = &value[quote_offset + quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(value[..end].to_owned())
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_owned())
}
