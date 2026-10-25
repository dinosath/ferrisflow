//! Script: runs a component's page script. On the web the script is a
//! wasm-bindgen snippet, which a Content Security Policy without
//! `'unsafe-eval'` allows; elsewhere it goes through `document::eval`
//! (RFC 0080).

use dioxus::document::EvalError;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Declares the module `$name` for a page script. `$source` exports
/// `async function run(dioxus)`, whose `dioxus.send` and `dioxus.recv` talk
/// to the [`Script`] that `$name::start()` returns. The calling module
/// imports `Script`. `$name` must be unique in the crate: wasm-bindgen links
/// a snippet's import by its Rust name, and two imports named alike call
/// the same snippet.
macro_rules! component_script {
    ($name:ident = $source:tt) => {
        mod $name {
            #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
            const SOURCE: &str = $source;

            #[cfg(target_arch = "wasm32")]
            #[wasm_bindgen::prelude::wasm_bindgen(inline_js = $source)]
            extern "C" {
                #[wasm_bindgen(js_name = run)]
                fn $name(dioxus: &wasm_bindgen::JsValue) -> wasm_bindgen::JsValue;
            }

            pub(super) fn start() -> super::Script {
                #[cfg(target_arch = "wasm32")]
                return super::Script::web($name);
                #[cfg(not(target_arch = "wasm32"))]
                return super::Script::eval(SOURCE);
            }

            #[cfg(test)]
            #[test]
            fn source_exports_run() {
                assert!(SOURCE.trim_start().starts_with("export async function run(dioxus) {"));
            }
        }
    };
}
pub(crate) use component_script;

/// A running page script. A value passed to `send` arrives at the script's
/// `dioxus.recv()`, and what the script passes to `dioxus.send` comes out of
/// `recv`, in order; once the script has ended, `recv` fails with
/// `EvalError::Finished`. Dropping it leaves the script running, unheard.
pub(crate) struct Script {
    #[cfg(target_arch = "wasm32")]
    channel: web::Channel,
    #[cfg(not(target_arch = "wasm32"))]
    eval: dioxus::document::Eval,
}

impl Script {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn web(run: fn(&wasm_bindgen::JsValue) -> wasm_bindgen::JsValue) -> Self {
        Self { channel: web::Channel::start(run) }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn eval(source: &str) -> Self {
        Self { eval: dioxus::document::eval(&eval_source(source)) }
    }

    pub(crate) fn send(&self, value: impl Serialize) -> Result<(), EvalError> {
        #[cfg(target_arch = "wasm32")]
        return self.channel.send(value);
        #[cfg(not(target_arch = "wasm32"))]
        return self.eval.send(value);
    }

    // Send-only scripts, such as Checkbox's, never call it.
    #[allow(dead_code)]
    // Takes `&self`, so one handle can both send and wait, as a shared
    // `Rc<Script>` does in the theme controller.
    pub(crate) async fn recv<T: DeserializeOwned>(&self) -> Result<T, EvalError> {
        #[cfg(target_arch = "wasm32")]
        return self.channel.recv().await;
        // `Eval` is a copyable handle to the same evaluator.
        #[cfg(not(target_arch = "wasm32"))]
        return { self.eval }.recv().await;
    }
}

/// What `document::eval` runs for a script: its function, then a call.
#[cfg(not(target_arch = "wasm32"))]
fn eval_source(source: &str) -> String {
    let source = source.trim_start();
    let function = source.strip_prefix("export ").unwrap_or(source);
    format!("{function}\nreturn await run(dioxus);")
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::task::{Poll, Waker};

    use dioxus::document::EvalError;
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use wasm_bindgen::prelude::*;

    // Gives a script its `dioxus` object. Values cross as JSON text; the
    // channel closes when `run` settles, and `detach` stops calls into Rust
    // once the Rust side is gone.
    #[wasm_bindgen(inline_js = r#"
export function channel(onSend, onClose) {
  const queue = [];
  let waiting = null;
  let attached = true;
  const close = () => {
    if (attached) onClose();
    attached = false;
  };
  return {
    dioxus: {
      send: (value) => {
        if (attached) onSend(JSON.stringify(value ?? null));
      },
      recv: () => new Promise((resolve) => (queue.length > 0 ? resolve(queue.shift()) : (waiting = resolve))),
    },
    push(json) {
      const value = JSON.parse(json);
      if (waiting) {
        const resolve = waiting;
        waiting = null;
        resolve(value);
      } else {
        queue.push(value);
      }
    },
    watch(running) {
      Promise.resolve(running).catch((error) => console.error(error)).finally(close);
    },
    detach() {
      attached = false;
    },
  };
}
"#)]
    extern "C" {
        type JsChannel;

        #[wasm_bindgen(js_name = channel)]
        fn js_channel(
            on_send: &Closure<dyn FnMut(String)>, on_close: &Closure<dyn FnMut()>,
        ) -> JsChannel;
        #[wasm_bindgen(method, getter)]
        fn dioxus(this: &JsChannel) -> JsValue;
        #[wasm_bindgen(method)]
        fn push(this: &JsChannel, json: &str);
        #[wasm_bindgen(method)]
        fn watch(this: &JsChannel, running: JsValue);
        #[wasm_bindgen(method)]
        fn detach(this: &JsChannel);
    }

    /// Messages from the script that `recv` has not taken yet.
    #[derive(Default)]
    struct Inbox {
        messages: VecDeque<String>,
        closed: bool,
        waker: Option<Waker>,
    }

    impl Inbox {
        fn wake(&mut self) {
            if let Some(waker) = self.waker.take() {
                waker.wake();
            }
        }
    }

    pub(super) struct Channel {
        channel: JsChannel,
        inbox: Rc<RefCell<Inbox>>,
        _on_send: Closure<dyn FnMut(String)>,
        _on_close: Closure<dyn FnMut()>,
    }

    impl Channel {
        pub(super) fn start(run: fn(&JsValue) -> JsValue) -> Self {
            let inbox = Rc::new(RefCell::new(Inbox::default()));
            let sent = inbox.clone();
            let on_send = Closure::<dyn FnMut(String)>::new(move |json: String| {
                let mut inbox = sent.borrow_mut();
                inbox.messages.push_back(json);
                inbox.wake();
            });
            let closed = inbox.clone();
            let on_close = Closure::<dyn FnMut()>::new(move || {
                let mut inbox = closed.borrow_mut();
                inbox.closed = true;
                inbox.wake();
            });
            let channel = js_channel(&on_send, &on_close);
            channel.watch(run(&channel.dioxus()));
            Self { channel, inbox, _on_send: on_send, _on_close: on_close }
        }

        pub(super) fn send(&self, value: impl Serialize) -> Result<(), EvalError> {
            let json = serde_json::to_string(&value)
                .map_err(|error| EvalError::Communication(error.to_string()))?;
            self.channel.push(&json);
            Ok(())
        }

        pub(super) async fn recv<T: DeserializeOwned>(&self) -> Result<T, EvalError> {
            let json = std::future::poll_fn(|context| {
                let mut inbox = self.inbox.borrow_mut();
                match inbox.messages.pop_front() {
                    Some(json) => Poll::Ready(Ok(json)),
                    None if inbox.closed => Poll::Ready(Err(EvalError::Finished)),
                    None => {
                        inbox.waker = Some(context.waker().clone());
                        Poll::Pending
                    }
                }
            })
            .await?;
            serde_json::from_str(&json).map_err(|error| EvalError::Communication(error.to_string()))
        }
    }

    impl Drop for Channel {
        fn drop(&mut self) {
            self.channel.detach();
        }
    }
}
