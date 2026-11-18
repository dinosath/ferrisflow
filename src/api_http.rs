#[cfg(not(feature = "local-app"))]
pub use reqwest::{Client, Response, get};

#[cfg(all(feature = "local-app", not(target_arch = "wasm32")))]
mod local {
    use std::sync::OnceLock;

    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use tower::ServiceExt;

    static API_ROUTER: OnceLock<Router> = OnceLock::new();

    pub fn install_router(router: Router) -> Result<(), Router> {
        API_ROUTER.set(router)
    }

    pub struct Client;

    impl Client {
        pub fn new() -> Self {
            Self
        }

        pub fn get(&self, url: impl Into<String>) -> RequestBuilder {
            RequestBuilder::new(Method::GET, url.into())
        }

        pub fn post(&self, url: impl Into<String>) -> RequestBuilder {
            RequestBuilder::new(Method::POST, url.into())
        }

        pub fn put(&self, url: impl Into<String>) -> RequestBuilder {
            RequestBuilder::new(Method::PUT, url.into())
        }

        pub fn delete(&self, url: impl Into<String>) -> RequestBuilder {
            RequestBuilder::new(Method::DELETE, url.into())
        }
    }

    pub async fn get(url: impl Into<String>) -> Result<Response, String> {
        Client::new().get(url).send().await
    }

    pub struct RequestBuilder {
        method: Method,
        url: String,
        body: Vec<u8>,
        content_type: Option<&'static str>,
        serialization_error: Option<String>,
    }

    impl RequestBuilder {
        fn new(method: Method, url: String) -> Self {
            Self { method, url, body: Vec::new(), content_type: None, serialization_error: None }
        }

        pub fn json<T: Serialize + ?Sized>(mut self, value: &T) -> Self {
            self.content_type = Some("application/json");
            match serde_json::to_vec(value) {
                Ok(body) => self.body = body,
                Err(error) => self.serialization_error = Some(error.to_string()),
            }
            self
        }

        pub async fn send(self) -> Result<Response, String> {
            if let Some(error) = self.serialization_error {
                return Err(error);
            }
            let uri = if self.url.starts_with('/') {
                self.url
            } else {
                let url = url::Url::parse(&self.url).map_err(|error| error.to_string())?;
                format!(
                    "{}{}",
                    url.path(),
                    url.query().map(|query| format!("?{query}")).unwrap_or_default()
                )
            };
            let mut request = Request::builder().method(self.method).uri(uri);
            if let Some(content_type) = self.content_type {
                request = request.header(axum::http::header::CONTENT_TYPE, content_type);
            }
            let request = request.body(Body::from(self.body)).map_err(|error| error.to_string())?;
            let router = API_ROUTER
                .get()
                .ok_or_else(|| "The local application API is not initialized".to_owned())?
                .clone();
            let response = router.oneshot(request).await.unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .map_err(|error| error.to_string())?
                .to_vec();
            Ok(Response { status, body })
        }
    }

    pub struct Response {
        status: StatusCode,
        body: Vec<u8>,
    }

    impl Response {
        pub fn status(&self) -> StatusCode {
            self.status
        }

        pub async fn json<T: DeserializeOwned>(&self) -> Result<T, String> {
            serde_json::from_slice(&self.body).map_err(|error| error.to_string())
        }

        pub async fn text(self) -> Result<String, String> {
            String::from_utf8(self.body).map_err(|error| error.to_string())
        }
    }
}

#[cfg(all(feature = "local-app", not(target_arch = "wasm32")))]
pub use local::{Client, Response, get};

#[cfg(all(feature = "local-app", not(target_arch = "wasm32")))]
pub(crate) use local::install_router;
