#[cfg(not(target_arch = "wasm32"))]
pub use reqwest::{Client, Response, get};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn save_basic_auth(_username: &str, _password: &str) {}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn clear_basic_auth() {}

#[cfg(target_arch = "wasm32")]
mod browser {
    const AUTH_STORAGE_KEY: &str = "ferrisflow.basic-auth";

    fn stored_auth() -> Option<String> {
        web_sys::window()?.session_storage().ok()??.get_item(AUTH_STORAGE_KEY).ok()?
    }

    fn with_auth(mut request: reqwest::RequestBuilder, url: &str) -> RequestBuilder {
        if same_origin(url) && let Some(auth) = stored_auth() {
            request = request.header(reqwest::header::AUTHORIZATION, auth);
        }
        RequestBuilder { inner: request }
    }

    fn same_origin(url: &str) -> bool {
        let Some(window) = web_sys::window() else { return false };
        let Ok(origin) = window.location().origin() else { return false };
        let Ok(origin_url) = url::Url::parse(&origin) else { return false };
        let Ok(request_url) = url::Url::parse(url) else { return false };
        request_url.origin() == origin_url.origin()
    }

    pub fn save_basic_auth(username: &str, password: &str) {
        use base64::Engine;
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        if let Some(storage) =
            web_sys::window().and_then(|window| window.session_storage().ok().flatten())
        {
            let _ = storage.set_item(AUTH_STORAGE_KEY, &format!("Basic {encoded}"));
        }
    }

    pub fn clear_basic_auth() {
        if let Some(storage) =
            web_sys::window().and_then(|window| window.session_storage().ok().flatten())
        {
            let _ = storage.remove_item(AUTH_STORAGE_KEY);
        }
    }

    pub struct Client(reqwest::Client);

    impl Client {
        pub fn new() -> Self {
            Self(reqwest::Client::new())
        }

        pub fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
            let url = url.as_ref();
            with_auth(self.0.get(url), url)
        }

        pub fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
            let url = url.as_ref();
            with_auth(self.0.post(url), url)
        }

        pub fn put(&self, url: impl AsRef<str>) -> RequestBuilder {
            let url = url.as_ref();
            with_auth(self.0.put(url), url)
        }

        pub fn delete(&self, url: impl AsRef<str>) -> RequestBuilder {
            let url = url.as_ref();
            with_auth(self.0.delete(url), url)
        }
    }

    pub struct RequestBuilder {
        inner: reqwest::RequestBuilder,
    }

    impl RequestBuilder {
        pub fn json<T: serde::Serialize + ?Sized>(mut self, value: &T) -> Self {
            self.inner = self.inner.json(value);
            self
        }

        pub async fn send(self) -> Result<reqwest::Response, reqwest::Error> {
            self.inner.send().await
        }
    }

    pub async fn get(url: impl AsRef<str>) -> Result<reqwest::Response, reqwest::Error> {
        Client::new().get(url).send().await
    }

    pub use reqwest::Response;
}

#[cfg(target_arch = "wasm32")]
pub use browser::{Client, Response, clear_basic_auth, get, save_basic_auth};
